package com.jjongjjongs.wiemobile;

import android.media.AudioAttributes;
import android.media.AudioFormat;
import android.media.AudioManager;
import android.media.AudioTrack;
import android.os.Build;
import android.os.Process;
import android.util.Log;

/**
 * Pulls the synthesiser's output on a dedicated thread, clocked by its own
 * AudioTrack, the way the reference player does.
 *
 * <p>The emulator used to push audio in bursts from the game loop - one chunk
 * per tick, tens of milliseconds apart at a low frame rate - and the stream
 * broke up between the bursts. Here a thread of its own asks the native side for
 * the next chunk ({@link NativeBridge#nativeRenderAudio}) and writes it with a
 * blocking write: the AudioTrack only accepts data as fast as it plays it, so
 * the write itself paces the render at real time, independent of how fast or
 * slow the game is running.
 */
final class MmfAudioPump {
    private static final String TAG = "WIE-MmfPump";

    private static final int RATE = 44100;
    private static final int CHANNELS = 2;
    private static final int FRAME_BYTES = CHANNELS * 2;
    /** Frames asked for per pull (~23 ms); small enough to stay responsive. */
    private static final int CHUNK_FRAMES = 1024;
    /** Track buffer, matching the reference's ~120 ms with headroom. */
    private static final int TRACK_BUFFER_MS = 180;
    /** Buffer filled before playback starts, so the first writes cannot drain
     *  the track before the pace settles. Below the buffer so a stopped track
     *  never blocks a write forever. */
    private static final int PREFILL_MS = 90;

    private static Thread thread;
    private static volatile boolean running;
    private static volatile boolean paused;

    private MmfAudioPump() {}

    static synchronized void start() {
        paused = false;
        if (thread != null && thread.isAlive()) {
            return;
        }
        running = true;
        thread = new Thread(MmfAudioPump::run, "WIE MMF audio");
        thread.setDaemon(true);
        thread.start();
    }

    static void pause() {
        paused = true;
    }

    static void resume() {
        paused = false;
    }

    static synchronized void release() {
        running = false;
        Thread old = thread;
        thread = null;
        if (old != null) {
            old.interrupt();
            try {
                old.join(250);
            } catch (InterruptedException ignored) {
            }
        }
    }

    private static void run() {
        Process.setThreadPriority(Process.THREAD_PRIORITY_URGENT_AUDIO);
        AudioTrack track = null;
        int prefillFrames = 0;
        boolean playing = false;
        try {
            while (running) {
                if (paused) {
                    if (track != null && playing && track.getPlayState() == AudioTrack.PLAYSTATE_PLAYING) {
                        try {
                            track.pause();
                        } catch (RuntimeException ignored) {
                        }
                    }
                    playing = false;
                    sleep(20);
                    continue;
                }

                byte[] pcm;
                try {
                    pcm = NativeBridge.nativeRenderAudio(CHUNK_FRAMES);
                } catch (Throwable t) {
                    pcm = null;
                }

                if (pcm == null || pcm.length == 0) {
                    // Nothing is sounding; idle briefly and keep the track ready.
                    sleep(5);
                    continue;
                }

                if (track == null) {
                    track = openTrack();
                    prefillFrames = 0;
                    playing = false;
                    if (track == null) {
                        sleep(20);
                        continue;
                    }
                }

                // Blocking write: this is the clock. It returns only as the track
                // drains, so the render keeps pace with playback rather than the
                // game loop.
                int offset = 0;
                boolean ok = true;
                while (offset < pcm.length) {
                    int written = track.write(pcm, offset, pcm.length - offset, AudioTrack.WRITE_BLOCKING);
                    if (written <= 0) {
                        ok = false;
                        break;
                    }
                    offset += written;
                }
                if (!ok) {
                    closeTrack(track);
                    track = null;
                    playing = false;
                    continue;
                }

                if (!playing) {
                    prefillFrames += pcm.length / FRAME_BYTES;
                    if (prefillFrames >= RATE * PREFILL_MS / 1000) {
                        try {
                            track.play();
                            playing = true;
                        } catch (RuntimeException ignored) {
                        }
                    }
                }
            }
        } catch (RuntimeException e) {
            Log.e(TAG, "audio pump failed", e);
        } finally {
            closeTrack(track);
        }
    }

    private static AudioTrack openTrack() {
        int mask = AudioFormat.CHANNEL_OUT_STEREO;
        int minimum = AudioTrack.getMinBufferSize(RATE, mask, AudioFormat.ENCODING_PCM_16BIT);
        if (minimum <= 0) {
            return null;
        }
        int bufferBytes = Math.max(minimum, RATE * FRAME_BYTES * TRACK_BUFFER_MS / 1000);
        try {
            AudioTrack track;
            if (Build.VERSION.SDK_INT >= 26) {
                track = new AudioTrack.Builder()
                        .setAudioAttributes(new AudioAttributes.Builder()
                                .setUsage(AudioAttributes.USAGE_GAME)
                                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                                .build())
                        .setAudioFormat(new AudioFormat.Builder()
                                .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                                .setSampleRate(RATE)
                                .setChannelMask(mask)
                                .build())
                        .setBufferSizeInBytes(bufferBytes)
                        .setTransferMode(AudioTrack.MODE_STREAM)
                        .build();
            } else {
                track = new AudioTrack(AudioManager.STREAM_MUSIC, RATE, mask,
                        AudioFormat.ENCODING_PCM_16BIT, bufferBytes, AudioTrack.MODE_STREAM);
            }
            if (track.getState() != AudioTrack.STATE_INITIALIZED) {
                track.release();
                return null;
            }
            Log.i(TAG, "opened " + RATE + "Hz stereo buffer=" + bufferBytes
                    + " frames=" + track.getBufferSizeInFrames());
            return track;
        } catch (RuntimeException e) {
            Log.e(TAG, "could not open AudioTrack", e);
            return null;
        }
    }

    private static void closeTrack(AudioTrack track) {
        if (track == null) {
            return;
        }
        try {
            track.stop();
        } catch (RuntimeException ignored) {
        }
        track.release();
    }

    private static void sleep(long millis) {
        try {
            Thread.sleep(millis);
        } catch (InterruptedException ignored) {
        }
    }
}
