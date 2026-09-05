package com.jjongjjongs.minimobile;

import android.os.SystemClock;
import android.util.Log;

final class NativeWaveBridge {
    private static final String TAG = "WIE-WaveHook";
    private static boolean libraryLoaded;
    private static boolean installed;
    private static long lastInstallAttemptMs;

    private NativeWaveBridge() {}

    static synchronized void install() {
        long now = SystemClock.uptimeMillis();
        if (installed && now - lastInstallAttemptMs < 1000) return;
        lastInstallAttemptMs = now;
        try {
            if (!libraryLoaded) {
                System.loadLibrary("wie_audio_hook");
                libraryLoaded = true;
            }
            installed = nativeInstall();
            Log.i(TAG, "install result=" + installed);
        } catch (Throwable error) {
            Log.e(TAG, "hook unavailable", error);
        }
    }

    private static native boolean nativeInstall();

    // Called synchronously before the baseline MA-3 synthesizer queues a wave.
    private static boolean onWave(int sampleRate, int sampleCount, long fingerprint, short[] pcm) {
        return ZenoniaAudioOverride.handleNativeWave(sampleRate, sampleCount, fingerprint, pcm);
    }
}
