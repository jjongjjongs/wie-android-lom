package com.jjongjjongs.wiemobile;

import android.media.AudioAttributes;
import android.media.AudioFormat;
import android.media.AudioManager;
import android.media.AudioTrack;
import android.os.Build;
import android.os.Process;
import android.util.Log;

import java.util.ArrayDeque;

/** Keeps emulator PCM writes off the emulation thread and absorbs scheduler jitter. */
public final class PcmStreamWriter {
    private static final String TAG = "WIE-PcmWriter";
    private static final int HEADER_BYTES = 10;
    private static final int MAX_QUEUED_BYTES = 44100 * 2 * 2 / 2; // 500 ms stereo PCM16
    private static final Object LOCK = new Object();
    private static final ArrayDeque<Packet> QUEUE = new ArrayDeque<>();

    private static Thread thread;
    private static AudioTrack track;
    private static int rate;
    private static int channels;
    private static int queuedBytes;
    private static boolean paused;
    private static boolean stopping;
    private static boolean wasSilence;
    private static short lastLeft;
    private static short lastRight;

    private PcmStreamWriter() {}

    public static void enqueue(byte[] command) {
        if (command == null || command.length <= HEADER_BYTES || (command[0] & 0xff) != 2) return;
        int packetChannels = command[1] & 0xff;
        int packetRate = readInt(command, 2);
        int sampleCount = readInt(command, 6);
        int byteCount = Math.min(sampleCount * 2, command.length - HEADER_BYTES);
        if (packetChannels < 1 || packetChannels > 2 || packetRate < 4000 || packetRate > 192000 || byteCount <= 0) return;

        byte[] pcm = new byte[byteCount];
        System.arraycopy(command, HEADER_BYTES, pcm, 0, byteCount);
        synchronized (LOCK) {
            ensureThreadLocked();
            if (rate != 0 && (rate != packetRate || channels != packetChannels)) {
                QUEUE.clear();
                queuedBytes = 0;
                closeTrackLocked();
            }
            rate = packetRate;
            channels = packetChannels;
            while (queuedBytes + byteCount > MAX_QUEUED_BYTES && !QUEUE.isEmpty()) {
                queuedBytes -= QUEUE.removeFirst().pcm.length;
                Log.w(TAG, "dropping stale PCM to cap latency");
            }
            QUEUE.addLast(new Packet(pcm, packetRate, packetChannels));
            queuedBytes += byteCount;
            LOCK.notifyAll();
        }
    }

    public static void pause() {
        synchronized (LOCK) {
            paused = true;
            QUEUE.clear();
            queuedBytes = 0;
            if (track != null) {
                try { track.pause(); track.flush(); } catch (RuntimeException ignored) {}
            }
        }
    }

    public static void resume() {
        synchronized (LOCK) {
            paused = false;
            LOCK.notifyAll();
        }
    }

    public static void release() {
        Thread oldThread;
        synchronized (LOCK) {
            stopping = true;
            QUEUE.clear();
            queuedBytes = 0;
            oldThread = thread;
            thread = null;
            LOCK.notifyAll();
        }
        if (oldThread != null) {
            oldThread.interrupt();
            try { oldThread.join(250); } catch (InterruptedException ignored) {}
        }
        synchronized (LOCK) {
            closeTrackLocked();
            wasSilence = false;
            lastLeft = 0;
            lastRight = 0;
        }
    }

    private static void ensureThreadLocked() {
        if (thread != null && thread.isAlive()) return;
        stopping = false;
        thread = new Thread(PcmStreamWriter::runWriter, "WIE PCM writer");
        thread.start();
    }

    private static void runWriter() {
        Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO);
        while (true) {
            Packet packet;
            synchronized (LOCK) {
                while (!stopping && paused) {
                    try { LOCK.wait(); } catch (InterruptedException ignored) {}
                }
                if (stopping) return;
                if (QUEUE.isEmpty() && track == null) {
                    try { LOCK.wait(); } catch (InterruptedException ignored) {}
                    continue;
                }
                if (QUEUE.isEmpty()) {
                    // Give the producer a short chance to arrive, then keep AudioTrack alive.
                    try { LOCK.wait(8); } catch (InterruptedException ignored) {}
                }
                if (paused || stopping) continue;
                if (QUEUE.isEmpty()) {
                    int silenceBytes = Math.max(4, rate * channels * 2 / 100); // 10 ms
                    packet = new Packet(new byte[silenceBytes], rate, channels, true);
                } else {
                    packet = QUEUE.removeFirst();
                    queuedBytes -= packet.pcm.length;
                }
                if (track == null) track = openTrack(packet.rate, packet.channels);
            }
            smoothBoundary(packet);
            AudioTrack current;
            synchronized (LOCK) { current = track; }
            if (current == null) continue;
            try {
                int offset = 0;
                while (offset < packet.pcm.length) {
                    int written = current.write(packet.pcm, offset, packet.pcm.length - offset, AudioTrack.WRITE_BLOCKING);
                    if (written <= 0) throw new IllegalStateException("AudioTrack write=" + written);
                    offset += written;
                }
                if (current.getPlayState() != AudioTrack.PLAYSTATE_PLAYING) current.play();
            } catch (RuntimeException e) {
                Log.e(TAG, "PCM output failed", e);
                synchronized (LOCK) { closeTrackLocked(); }
            }
        }
    }

    private static void smoothBoundary(Packet packet) {
        int frameBytes = packet.channels * 2;
        int frames = packet.pcm.length / frameBytes;
        int fadeFrames = Math.min(frames, Math.max(1, packet.rate / 200)); // 5 ms
        if (packet.silence && !wasSilence) {
            for (int frame = 0; frame < fadeFrames; frame++) {
                float gain = 1.0f - (frame + 1.0f) / fadeFrames;
                putSample(packet.pcm, frame * frameBytes, (short)(lastLeft * gain));
                if (packet.channels == 2) putSample(packet.pcm, frame * frameBytes + 2, (short)(lastRight * gain));
            }
        } else if (!packet.silence && wasSilence) {
            for (int frame = 0; frame < fadeFrames; frame++) {
                float gain = (frame + 1.0f) / fadeFrames;
                int offset = frame * frameBytes;
                putSample(packet.pcm, offset, (short)(getSample(packet.pcm, offset) * gain));
                if (packet.channels == 2) putSample(packet.pcm, offset + 2, (short)(getSample(packet.pcm, offset + 2) * gain));
            }
        }
        if (!packet.silence && frames > 0) {
            int offset = (frames - 1) * frameBytes;
            lastLeft = getSample(packet.pcm, offset);
            lastRight = packet.channels == 2 ? getSample(packet.pcm, offset + 2) : lastLeft;
        }
        wasSilence = packet.silence;
    }

    private static short getSample(byte[] pcm, int offset) {
        return (short)((pcm[offset] & 0xff) | (pcm[offset + 1] << 8));
    }

    private static void putSample(byte[] pcm, int offset, short value) {
        pcm[offset] = (byte)value;
        pcm[offset + 1] = (byte)(value >> 8);
    }

    private static AudioTrack openTrack(int sampleRate, int channelCount) {
        int mask = channelCount == 2 ? AudioFormat.CHANNEL_OUT_STEREO : AudioFormat.CHANNEL_OUT_MONO;
        int minimum = AudioTrack.getMinBufferSize(sampleRate, mask, AudioFormat.ENCODING_PCM_16BIT);
        if (minimum <= 0) return null;
        int bufferBytes = minimum;
        try {
            AudioTrack result;
            if (Build.VERSION.SDK_INT >= 26) {
                result = new AudioTrack.Builder()
                        .setAudioAttributes(new AudioAttributes.Builder()
                                .setUsage(AudioAttributes.USAGE_GAME)
                                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                                .build())
                        .setAudioFormat(new AudioFormat.Builder()
                                .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                                .setSampleRate(sampleRate)
                                .setChannelMask(mask)
                                .build())
                        .setBufferSizeInBytes(bufferBytes)
                        .setTransferMode(AudioTrack.MODE_STREAM)
                        .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
                        .build();
            } else {
                result = new AudioTrack(AudioManager.STREAM_MUSIC, sampleRate, mask,
                        AudioFormat.ENCODING_PCM_16BIT, bufferBytes, AudioTrack.MODE_STREAM);
            }
            if (result.getState() != AudioTrack.STATE_INITIALIZED) {
                result.release();
                return null;
            }
            if (Build.VERSION.SDK_INT >= 23) {
                result.setBufferSizeInFrames(Math.max(256, sampleRate / 25)); // target 40 ms
            }
            Log.i(TAG, "opened " + sampleRate + "Hz channels=" + channelCount
                    + " buffer=" + bufferBytes + " frames=" + result.getBufferSizeInFrames());
            return result;
        } catch (RuntimeException e) {
            Log.e(TAG, "could not open AudioTrack", e);
            return null;
        }
    }

    private static void closeTrackLocked() {
        if (track == null) return;
        try { track.stop(); } catch (RuntimeException ignored) {}
        track.release();
        track = null;
    }

    private static int readInt(byte[] data, int offset) {
        return (data[offset] & 0xff)
                | ((data[offset + 1] & 0xff) << 8)
                | ((data[offset + 2] & 0xff) << 16)
                | ((data[offset + 3] & 0xff) << 24);
    }

    private static final class Packet {
        final byte[] pcm;
        final int rate;
        final int channels;
        final boolean silence;

        Packet(byte[] pcm, int rate, int channels) {
            this(pcm, rate, channels, false);
        }

        Packet(byte[] pcm, int rate, int channels, boolean silence) {
            this.pcm = pcm;
            this.rate = rate;
            this.channels = channels;
            this.silence = silence;
        }
    }
}
