# Mobile setup

The Android app (`com.jjongjjongs.wiemobile`) wraps the emulator core so
feature phone archives can be run on a handset. It is a thin shell: a library
of imported .zip/.jar files, and a player with an on-screen keypad.

## Layout

| Path                            | What it is                                        |
|---------------------------------|---------------------------------------------------|
| `wie_android/`                  | JNI bridge, Android `Platform` impl, emulator loop |
| `android/`                      | Gradle project producing the apk                   |
| `.github/workflows/android.yml` | CI build, uploads `app-debug.apk`                  |

Java never talks to the emulator directly. `NativeBridge` exposes ten calls;
`wie_android/src/lib.rs` documents each one and `wie_android/src/audio.rs`
documents the byte encoding used for audio and vibration.

## Building

```
rustup target add aarch64-linux-android
cargo install cargo-ndk

cd android
gradle assembleDebug
```

`gradle` runs `cargo ndk` first and packages the resulting
`libwie_android.so`. `ANDROID_NDK_HOME` must point at an installed NDK. Pass
`-PskipCargo` to package a `.so` you built separately into
`android/app/build/jniLibs/<abi>/`.

Only `arm64-v8a` is built. Add ABIs to `abis` in `android/app/build.gradle`
and to the matching `rustup target add`.

## Running a game

Import a `.zip` (or `.jar`/`.apk`) with "APK/ZIP 가져오기"; it is copied into
app-private storage. Tapping a row starts it, long-pressing deletes it. The
back key leaves the player.

The format is detected from the archive contents rather than the filename -
KTF (`__adf__`), LGT (`app_info`), SKT (`*.msd`), then jar - so files renamed
by a download manager still load.

Save data lives under `getFilesDir()/runtime`, keyed by the app id from the
archive descriptor. Jars carry no descriptor, so their key is derived from the
file contents instead: re-importing the same jar keeps its saves, and two
different jars never share them.

## Logs

Native diagnostics go to logcat under the `WIE` tag:

```
adb logcat -s WIE
```

`RUST_LOG` is honoured when set in the process environment; the default is
`info`.

## LGT status

`.github/workflows/lom-diagnostic.yml` runs Legend of Master under `wie_cli`
and uploads a trace, which is how the LGT runtime is being reverse
engineered.

LGT titles come in two kinds, and `app_info`'s `MClass` says which.

`MClass:Clet` is a native Clet: it registers through `clet_register`, runs as
a `net/wie/CletWrapper` Jlet, and **renders**. Eight of eleven retail archives
tested run this way. These are the LGT titles worth running today.

Anything else names a class that LGT's toolchain compiled ahead of time into
`binary.mod`, Legend of Master (`MClass:Lm`) among them. Those now boot -
`startApp` completes and the `paint` override runs every redraw - but draw
nothing, because the application is waiting on state whose source has not been
found. See `docs/lgt.md`.
