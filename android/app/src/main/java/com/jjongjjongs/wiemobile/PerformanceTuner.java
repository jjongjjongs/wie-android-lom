package com.jjongjjongs.wiemobile;

import android.content.Context;
import android.os.Build;
import android.os.PerformanceHintManager;
import android.os.Process;
import android.util.Log;

final class PerformanceTuner {
    private static final String TAG = "WIE-Performance";
    private static final long TARGET_FRAME_NS = 16_666_667L;

    private static PerformanceHintManager hintManager;
    private static final ThreadLocal<State> STATE = new ThreadLocal<>();

    private PerformanceTuner() {}

    static void initialize(Context context) {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S || hintManager != null) {
            return;
        }
        try {
            hintManager = context.getSystemService(PerformanceHintManager.class);
        } catch (RuntimeException error) {
            Log.w(TAG, "performance hint manager unavailable", error);
        }
    }

    static void beforeNativeTick() {
        State state = STATE.get();
        if (state == null) {
            state = new State();
            STATE.set(state);
            try {
                Process.setThreadPriority(Process.THREAD_PRIORITY_DISPLAY);
                Log.i(TAG, "emulator thread priority=" + Process.getThreadPriority(Process.myTid()));
            } catch (RuntimeException error) {
                Log.w(TAG, "could not raise emulator thread priority", error);
            }

            PerformanceHintManager manager = hintManager;
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S && manager != null) {
                try {
                    state.session = manager.createHintSession(
                            new int[] { Process.myTid() }, TARGET_FRAME_NS);
                    Log.i(TAG, "performance hint session opened tid=" + Process.myTid());
                } catch (RuntimeException error) {
                    Log.w(TAG, "could not open performance hint session", error);
                }
            }
        }
        state.startedNs = System.nanoTime();
    }

    static void afterNativeTick() {
        State state = STATE.get();
        if (state == null || state.session == null || state.startedNs == 0L) {
            return;
        }
        long durationNs = Math.max(1L, System.nanoTime() - state.startedNs);
        try {
            state.session.reportActualWorkDuration(durationNs);
        } catch (RuntimeException error) {
            Log.w(TAG, "performance hint report failed", error);
            state.session.close();
            state.session = null;
        }
    }

    private static final class State {
        long startedNs;
        PerformanceHintManager.Session session;
    }
}
