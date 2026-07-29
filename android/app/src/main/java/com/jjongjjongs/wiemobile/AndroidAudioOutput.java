package com.jjongjjongs.wiemobile;

import android.content.Context;
import android.media.AudioAttributes;
import android.media.AudioFormat;
import android.media.AudioManager;
import android.media.AudioTrack;
import android.os.Build;
import android.os.VibrationEffect;
import android.os.Vibrator;

import java.util.ArrayList;
import java.util.Iterator;
import java.util.List;

/**
 * Decodes the byte commands produced by the native audio sink.
 *
 * <p>Each command starts with a one byte opcode; all multi-byte fields are
 * little endian.
 *
 * <table>
 *   <tr><td>1</td><td>{@code channel:u8, sampleRate:u32, sampleCount:u32, samples:i16[]}</td></tr>
 *   <tr><td>2</td><td>{@code pad:u8, sampleRate:u32, sampleCount:u32, samples:i16[]}</td></tr>
 *   <tr><td>8</td><td>{@code intensity:u8, durationMs:u64}</td></tr>
 * </table>
 *
 * <p>Opcode 1 is a clip: a track is fired at it and forgotten. Opcode 2 is the
 * synthesiser's continuous output, which goes to one track that stays open —
 * a track per chunk would put a click at every seam.
 */
final class AndroidAudioOutput {
    private static final int OPCODE_PLAY_WAVE = 1;
    private static final int OPCODE_STREAM = 2;
    private static final int OPCODE_VIBRATE = 8;

    /** Buffer for the streaming track, as a multiple of the device minimum. */
    private static final int STREAM_BUFFER_FACTOR = 4;

    /** Header length shared by both commands. */
    private static final int HEADER_LEN = 10;

    private static final int MIN_SAMPLE_RATE = 4000;
    private static final int MAX_SAMPLE_RATE = 192000;

    private final List<AudioTrack> tracks = new ArrayList<>();
    private final Vibrator vibrator;

    private AudioTrack stream;
    private int streamRate;

    AndroidAudioOutput(Context context) {
        this.vibrator = (Vibrator) context.getSystemService(Context.VIBRATOR_SERVICE);
    }

    synchronized void handle(byte[] command) {
        if (command == null || command.length == 0) {
            return;
        }

        cleanupFinished();

        switch (command[0] & 0xFF) {
            case OPCODE_PLAY_WAVE:
                playWave(command);
                break;
            case OPCODE_STREAM:
                writeStream(command);
                break;
            case OPCODE_VIBRATE:
                vibrate(command);
                break;
            default:
                break;
        }
    }

    synchronized void release() {
        if (stream != null) {
            try {
                stream.stop();
            } catch (RuntimeException ignored) {
                // Never started, or already stopped.
            }
            stream.release();
            stream = null;
        }

        for (AudioTrack track : tracks) {
            try {
                track.stop();
            } catch (RuntimeException ignored) {
                // Already stopped or never started; releasing is what matters.
            }
            track.release();
        }
        tracks.clear();
    }

    /**
     * Short clips are fired and forgotten, so finished tracks are reaped here
     * rather than tracked individually.
     */
    private void cleanupFinished() {
        Iterator<AudioTrack> iterator = tracks.iterator();
        while (iterator.hasNext()) {
            AudioTrack track = iterator.next();
            if (track.getPlayState() != AudioTrack.PLAYSTATE_PLAYING) {
                track.release();
                iterator.remove();
            }
        }
    }

    private void playWave(byte[] command) {
        if (command.length < HEADER_LEN) {
            return;
        }

        int sampleRate = readInt(command, 2);
        int byteCount = Math.min(readInt(command, 6) * 2, command.length - HEADER_LEN);

        if (sampleRate < MIN_SAMPLE_RATE || sampleRate > MAX_SAMPLE_RATE || byteCount <= 0) {
            return;
        }

        try {
            AudioTrack track;
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
                track = new AudioTrack(AudioManager.STREAM_MUSIC, sampleRate, AudioFormat.CHANNEL_OUT_MONO,
                        AudioFormat.ENCODING_PCM_16BIT, byteCount, AudioTrack.MODE_STATIC);
            } else {
                track = new AudioTrack.Builder()
                        .setAudioAttributes(new AudioAttributes.Builder()
                                .setUsage(AudioAttributes.USAGE_GAME)
                                .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                                .build())
                        .setAudioFormat(new AudioFormat.Builder()
                                .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                                .setSampleRate(sampleRate)
                                .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                                .build())
                        .setBufferSizeInBytes(byteCount)
                        .setTransferMode(AudioTrack.MODE_STATIC)
                        .build();
            }

            if (track.getState() != AudioTrack.STATE_INITIALIZED) {
                track.release();
                return;
            }

            track.write(command, HEADER_LEN, byteCount, AudioTrack.WRITE_BLOCKING);
            track.play();
            tracks.add(track);
        } catch (IllegalArgumentException | IllegalStateException | UnsupportedOperationException e) {
            // A rate or buffer size the device will not take; skipping the clip
            // is better than losing the game.
        }
    }

    /**
     * Appends a chunk to the synthesiser's track, opening it on the first
     * chunk. Writing is non-blocking: if the buffer is full the emulator has
     * run ahead, and dropping the overflow keeps playback level rather than
     * stalling the thread that is also running the game.
     */
    private void writeStream(byte[] command) {
        if (command.length < HEADER_LEN) {
            return;
        }

        int sampleRate = readInt(command, 2);
        int byteCount = Math.min(readInt(command, 6) * 2, command.length - HEADER_LEN);

        if (sampleRate < MIN_SAMPLE_RATE || sampleRate > MAX_SAMPLE_RATE || byteCount <= 0) {
            return;
        }

        if (stream != null && streamRate != sampleRate) {
            stream.release();
            stream = null;
        }

        if (stream == null) {
            stream = openStream(sampleRate);
            if (stream == null) {
                return;
            }
            streamRate = sampleRate;
        }

        try {
            stream.write(command, HEADER_LEN, byteCount, AudioTrack.WRITE_NON_BLOCKING);

            if (stream.getPlayState() != AudioTrack.PLAYSTATE_PLAYING) {
                stream.play();
            }
        } catch (IllegalStateException e) {
            stream.release();
            stream = null;
        }
    }

    private AudioTrack openStream(int sampleRate) {
        int minimum = AudioTrack.getMinBufferSize(sampleRate, AudioFormat.CHANNEL_OUT_MONO, AudioFormat.ENCODING_PCM_16BIT);
        if (minimum <= 0) {
            return null;
        }

        int bufferSize = minimum * STREAM_BUFFER_FACTOR;

        try {
            AudioTrack track;
            if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
                track = new AudioTrack(AudioManager.STREAM_MUSIC, sampleRate, AudioFormat.CHANNEL_OUT_MONO,
                        AudioFormat.ENCODING_PCM_16BIT, bufferSize, AudioTrack.MODE_STREAM);
            } else {
                track = new AudioTrack.Builder()
                        .setAudioAttributes(new AudioAttributes.Builder()
                                .setUsage(AudioAttributes.USAGE_GAME)
                                .setContentType(AudioAttributes.CONTENT_TYPE_MUSIC)
                                .build())
                        .setAudioFormat(new AudioFormat.Builder()
                                .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
                                .setSampleRate(sampleRate)
                                .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
                                .build())
                        .setBufferSizeInBytes(bufferSize)
                        .setTransferMode(AudioTrack.MODE_STREAM)
                        .build();
            }

            if (track.getState() != AudioTrack.STATE_INITIALIZED) {
                track.release();
                return null;
            }

            return track;
        } catch (IllegalArgumentException | IllegalStateException | UnsupportedOperationException e) {
            return null;
        }
    }

    private void vibrate(byte[] command) {
        if (vibrator == null || command.length < HEADER_LEN) {
            return;
        }

        long durationMs = readLong(command, 2);
        if (durationMs <= 0) {
            return;
        }

        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) {
            vibrator.vibrate(durationMs);
        } else {
            int amplitude = Math.max(1, Math.min(255, command[1] & 0xFF));
            vibrator.vibrate(VibrationEffect.createOneShot(durationMs, amplitude));
        }
    }

    private static int readInt(byte[] buffer, int offset) {
        return (buffer[offset] & 0xFF)
                | ((buffer[offset + 1] & 0xFF) << 8)
                | ((buffer[offset + 2] & 0xFF) << 16)
                | ((buffer[offset + 3] & 0xFF) << 24);
    }

    private static long readLong(byte[] buffer, int offset) {
        long value = 0;
        for (int i = 0; i < 8; i++) {
            value |= ((long) (buffer[offset + i] & 0xFF)) << (i * 8);
        }
        return value;
    }
}
