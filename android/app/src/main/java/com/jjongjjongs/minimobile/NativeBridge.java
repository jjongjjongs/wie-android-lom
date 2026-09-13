package com.jjongjjongs.minimobile;

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
    static native String nativeStart(
            byte[] archive,
            String runtimeDir,
            String phoneModel);

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

    /**
     * Renders up to {@code frames} stereo frames from the synthesiser as
     * little-endian 16-bit PCM, or an empty array when nothing is sounding.
     * Called from the audio thread, clocked by its AudioTrack, rather than the
     * emulator thread, so it does not take the emulator lock.
     */
    static native byte[] nativeRenderAudio(int frames);

    /** Returns a pending handset backlight mode, or zero when unchanged. */
    static native int nativePollBacklightMode();

    /** Returns a pending phone-call number, or null when there is no request. */
    static native String nativePollPhoneCall();

    /** Returns a pending URL to open in the host browser, or null when there is no request. */
    static native String nativePollBrowserUrl();

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

    /** The {@code tracing} filter directive the log capture is running now. */
    static native String nativeLogFilter();

    /**
     * Swaps the live log filter without a rebuild, so a module's debug/trace
     * detail can be captured on the spot.
     *
     * @param directive a {@code RUST_LOG}-style filter, e.g. {@code "wie_lgt=trace"}
     * @return empty on success, otherwise why the directive was rejected
     */
    static native String nativeSetLogFilter(String directive);

    /** What the probe is watching now, in the syntax {@link #nativeSetProbeWatches} takes. */
    static native String nativeProbeWatches();

    /**
     * Arms the probe from a specification typed on the handset, so a question
     * about any title stops needing a build of its own.
     *
     * <p>Comma-separated: {@code pc:<hex>} traces branches from an address
     * (optionally {@code /<count>}), {@code w:<hex>} reports writes to one. An
     * empty specification clears every watch.
     *
     * @return empty on success, otherwise why it was rejected
     */
    static native String nativeSetProbeWatches(String spec);

    /**
     * Opens a log collection window: throws away what is held, turns every area
     * on, and records from here until {@link #nativeStopLogCollect()}.
     *
     * <p>The always-on capture the crash auto-save reads is the same one, so a
     * game that dies inside a window still leaves its log behind - the window's
     * log, which is the more useful of the two.
     *
     * @return empty on success, otherwise why it could not start
     */
    static native String nativeStartLogCollect();

    /**
     * Closes the window and puts the filter back. The log is left as it stands
     * for {@link #nativeLog()} to read, so what gets saved is what happened
     * between the two presses.
     *
     * @return empty on success, otherwise why it could not stop
     */
    static native String nativeStopLogCollect();

    /** Whether a collection window is open. */
    static native int nativeLogCollecting();

    /** Version of the native library. */
    static native String nativeVersion();
}
