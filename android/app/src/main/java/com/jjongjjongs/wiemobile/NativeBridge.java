package com.jjongjjongs.wiemobile;

/**
 * Entry points implemented by the {@code wie_android} crate.
 *
 * <p>{@link #nativeStart}, {@link #nativeTick}, {@link #nativeFrame} and
 * {@link #nativePollOutput} are called from the emulator thread.
 * {@link #nativeKey} and {@link #nativeStop} are called from the UI thread;
 * the native side queues input rather than touching the emulator directly, so
 * a touch never blocks behind a tick.
 */
final class NativeBridge {
    static {
        System.loadLibrary("wie_android");
    }

    private NativeBridge() {
    }

    /**
     * Loads an archive and starts the emulator.
     *
     * @param archive    the whole .zip/.jar/.apk file
     * @param runtimeDir app-private directory for save data
     * @return empty on success, otherwise the message to show in the player
     */
    static native String nativeStart(byte[] archive, String runtimeDir);

    /**
     * Runs the emulator for up to {@code budgetMs}.
     *
     * @return empty while the game is healthy, otherwise the message that
     *         stopped it
     */
    static native String nativeTick(int budgetMs);

    /** Tears the emulator down. Safe to call when nothing is running. */
    static native void nativeStop();

    /** Non-zero while a game is loaded. */
    static native int nativeRunning();

    /** The message that stopped the last run, or empty. */
    static native String nativeLastError();

    /**
     * @param index   key index, matching the keypad laid out in MainActivity
     * @param pressed 1 for down, 0 for up
     */
    static native void nativeKey(int index, int pressed);

    /**
     * @return {@code null} when nothing new was painted, otherwise
     *         {@code {width, height, RGB565 pixels...}}
     */
    static native short[] nativeFrame();

    /**
     * @return the next queued audio or vibration command, or {@code null}
     * @see AndroidAudioOutput for the encoding
     */
    static native byte[] nativePollOutput();

    /** Returns a pending handset backlight mode, or zero when unchanged. */
    static native int nativePollBacklightMode();

    /** Describes an archive without running it. */
    static native String nativeInspect(byte[] archive);

    /**
     * Where an archive's saved data sits under the runtime directory. Only the
     * loader knows how an archive names itself, and the two names differ:
     * record stores go under the product id, written files under the
     * application id.
     *
     * @return {@code "<record store id>\n<filesystem id>"}, or empty
     * @see SaveExporter
     */
    static native String nativeSaveIds(byte[] archive);

    /**
     * Everything logged since the running game started. Reading does not clear
     * it, so it can be taken more than once during a long run.
     */
    static native String nativeLog();

    /** Version of the native library. */
    static native String nativeVersion();
}
