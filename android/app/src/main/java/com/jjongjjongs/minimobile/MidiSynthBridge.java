package com.jjongjjongs.minimobile;

final class MidiSynthBridge {
    static {
        System.loadLibrary("wie_midi");
    }

    private MidiSynthBridge() {
    }

    static byte[] handle(byte[] command) {
        return nativeHandle(command);
    }

    static byte[] render(int milliseconds) {
        return nativeRender(milliseconds);
    }

    static void reset() {
        nativeReset();
    }

    private static native byte[] nativeHandle(byte[] command);
    private static native byte[] nativeRender(int milliseconds);
    private static native void nativeReset();
    static native boolean nativeAvailable();
}
