# The native library resolves these by their JNI symbol names, so nothing here
# may be renamed or stripped even though no Java code calls them reflectively.
-keepclasseswithmembernames class com.jjongjjongs.minimobile.NativeBridge {
    native <methods>;
}

-keepclasseswithmembernames class com.jjongjjongs.minimobile.NativeWaveBridge {
    native <methods>;
}

-keepclasseswithmembernames class com.jjongjjongs.minimobile.MidiSynthBridge {
    native <methods>;
}
