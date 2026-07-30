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

## Sound

A `.mmf` carries PCM waves for effects and a MIDI-like sequence for the music.
Android has no synthesiser that takes live MIDI events, so `wie_android/src/ma3/`
renders the sequence to PCM here and sends it as a stereo stream Java keeps one
track open for. Without it, a title whose music is entirely sequenced plays
silently - which is most of them.

`ma3` is the Yamaha MA-3 a handset had, in software: four operator FM, with the
waveforms, envelope rates, key scaling, detune, low frequency oscillator and
output attenuation taken from the chip's own tables (`ma3/data/`, extracted
from a handset ROM). The instruments are the ones the file itself carries, sent
as system exclusive in its setup chunk and sometimes part way through the
sequence; `ma3/tone.rs` reads them.

Two things a handset kept in ROM are not reproduced, and get stand-in patches
instead, marked as such where they are built:

- the melodic bank a file falls back on when it defines no voice of its own;
- the recorded drums behind twenty one of the kit's 128 keys. The other forty
  are voices, and are played properly.

To listen to a change rather than only measure it:

```
WIE_MMF=music.mmf WIE_WAV=out.wav \
  cargo test -p wie_android --lib render_a_file -- --ignored --nocapture
```

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
app-private storage. Tapping a row starts it; long-pressing offers to get its
saves out or to drop it from the library. The back key leaves the player.

The format is detected from the archive contents rather than the filename -
KTF (`__adf__`), LGT (`app_info`), SKT (`*.msd`), then jar - so files renamed
by a download manager still load.

### The keypad

One `View` draws and handles the whole keypad, because a grid of `Button`s
cannot take two fingers: the first view to accept a touch owns the gesture, so
a second finger on another button is delivered to the first one. A direction
held while a number is tapped, or SEED 2 asking for `0` and `#` together,
needs the single view.

The width is split in half - directions on the left, numbers on the right -
under a function row that keeps the same split: the two soft keys over the pad,
save and back over the numbers. `wie_android/src/runner.rs`'s `key_code` maps
each key's index to a `KeyCode`; the two have to be changed together.

### Saved data

Save data lives under `getFilesDir()/runtime`: record stores in
`db/<product id>`, files a game wrote itself in `fs/<application id>`. Both ids
come from the archive descriptor and are usually different. Jars carry no
descriptor, so their id is derived from the file contents instead:
re-importing the same jar keeps its saves, and two different jars never share
them.

`SaveExporter` zips both directories into Downloads on a long press. It asks
`NativeBridge.nativeSaveIds` where to look, because only the loader knows how
an archive names itself - `runner.rs`'s `save_ids` has to keep agreeing with
what the emulators pass to `System::new`, or an export comes back empty.

## Logs

Native diagnostics go to logcat under the `WIE` tag:

```
adb logcat -s WIE
```

`RUST_LOG` is honoured when set in the process environment; the default is
`info`.

A phone with no `adb` attached gets the same thing from the player: the "로그"
button beside the status line writes it to Downloads as `<title> 로그.txt`.
`logging.rs` keeps a copy of every line as well as sending it to logcat, and
`nativeStart` clears it, so the file covers one run of one game. It can be
taken while the game is still going, which is what a title that hangs rather
than stops needs; the file also carries `nativeLastError` in full, since the
status line only has room for one truncated line of it.

The copy is bounded at 20,000 lines or 2MB, dropping oldest first, and says so
at the top when it has dropped any.

## LGT status

`.github/workflows/lom-diagnostic.yml` runs Legend of Master under `wie_cli`
and uploads a trace, which is how the LGT runtime is being reverse
engineered.

LGT titles come in two kinds, and `app_info`'s `MClass` says which.

`MClass:Clet` is a native Clet: it registers through `clet_register`, runs as
a `net/wie/CletWrapper` Jlet, and **renders**. Nine of the twelve archives in
the batch run this way, which is what `screen_capture`'s `capture_archives`
is for:

```
WIE_ARCHIVES=/path/to/archives cargo test -p wie_lgt --test screen_capture \
    capture_archives -- --ignored --nocapture
```

Anything else names a class that LGT's toolchain compiled ahead of time into
`binary.mod`, Legend of Master (`MClass:Lm`) among them. Those run too - Legend
of Master draws its notice screen and runs its game loop. See `docs/lgt.md`.

Set `WIE_CAPTURE_DIR` alongside `WIE_ARCHIVES` to have each title's busiest
frame written out as a PPM, which is how you tell a title that draws from one
that only paints.

An archive stripped of its `app_info` is still a title. `binary.mod` inside a
bare jar is detected and run the same way, so a game that arrives as just its
jar loads.
