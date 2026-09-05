# WIE Android 변경사항 커밋, 푸시 및 APK 빌드

이 문서는 현재 검증된 제노니아 1/2/3 오디오 보완 버전을 GitHub에 올리고,
새 작업 폴더에서도 같은 Android APK를 빌드하는 절차를 정리한다.

## 1. 저장소에 포함되는 것과 포함되지 않는 것

저장소에 포함되는 항목:

- Android 오디오 출력 및 효과음 교체 코드
- MIDI 재생용 `wie_midi` 크레이트
- ARM 실행 최적화 코드
- JNI/CMake 오디오 브리지
- 로컬 음원 가져오기 및 검증 스크립트

저장소에 포함되지 않는 로컬 파일:

- `android/app/src/main/assets/zenonia1/`
- `android/app/src/main/assets/zenonia2/`
- `android/app/src/main/assets/zenonia3/`
- `wie_midi/soundfont.sf2`

위 파일들은 `.gitignore`에 등록되어 있다. 커밋 전에 아래 명령으로 실수로
스테이징되지 않았는지 반드시 확인한다.

```powershell
git status --short --ignored
git diff --cached --name-only
```

## 2. 로컬 음원 백업 구조

저장소 밖에 다음 구조로 원본을 보관한다.

```text
D:\WIE-Private-Audio\
|-- zenonia1\     # 정확히 51개 파일
|-- zenonia2\     # 정확히 16개 파일
|-- zenonia3\     # 정확히 17개 파일
`-- soundfont.sf2
```

이 폴더를 보관하지 않으면 새 PC나 새 clone에서 현재와 같은 오디오 포함 APK를
다시 만들 수 없다. 현재 프로젝트의 무시된 음원 폴더도 별도로 백업한다.

휴대폰에 보관한 `WIE-개인용-빌드-음원.zip`을 새 PC로 복사해 압축을 풀면 위
구조가 그대로 만들어진다. 따라서 특정 PC에 종속되는 방식은 아니다. 필요한 것은
GitHub 저장소와 이 개인용 음원 ZIP 두 가지다.

## 3. 현재 변경사항 커밋 및 푸시

현재 작업 폴더가 detached HEAD 상태일 수 있으므로 먼저 작업 브랜치를 만든다.

```powershell
cd <저장소를 받은 경로>\wie-android-lom
git switch -c claude/zenonia-audio-integration
git status --short
git add .
git diff --cached --name-only
git diff --cached --check
git commit -m "Integrate Android audio fixes and ARM optimizations"
git push -u origin claude/zenonia-audio-integration
```

`git diff --cached --name-only` 출력에 `zenonia1`, `zenonia2`, `zenonia3` 또는
`soundfont.sf2`가 보이면 커밋을 중단하고 해당 파일을 스테이징에서 제외한다.

```powershell
git restore --staged android/app/src/main/assets/zenonia1
git restore --staged android/app/src/main/assets/zenonia2
git restore --staged android/app/src/main/assets/zenonia3
git restore --staged wie_midi/soundfont.sf2
```

검토 후 GitHub에서 `claude/zenonia-audio-integration` 브랜치를 `main`으로 병합한다.

## 4. 새 clone에서 로컬 음원 가져오기

```powershell
git clone https://github.com/jjongjjongs/wie-android-lom.git
cd wie-android-lom
git switch claude/zenonia-audio-integration

# 휴대폰에서 복사한 WIE-개인용-빌드-음원.zip을 먼저 압축 해제한다.

.\android\import-local-audio.ps1 `
  -AudioRoot "D:\WIE-Private-Audio" `
  -SoundFont "D:\WIE-Private-Audio\soundfont.sf2"
```

스크립트는 각 게임의 파일 수를 검사하고 다음 위치로 복사한다.

- 게임 음원: `android/app/src/main/assets/zenonia*/`
- SoundFont: `wie_midi/soundfont.sf2`

## 5. 빌드 환경 준비

필수 도구:

- JDK 17
- Android SDK Platform 35
- Android NDK
- CMake 3.22.1
- Rust stable
- Rust Android ARM64 타깃
- `cargo-ndk`

최초 한 번 실행한다.

```powershell
rustup target add aarch64-linux-android
cargo install cargo-ndk --locked
```

Android Studio 기본 설치 경로를 사용하는 예:

```powershell
$env:JAVA_HOME = "C:\Program Files\Android\Android Studio\jbr"
$env:ANDROID_HOME = "$env:LOCALAPPDATA\Android\Sdk"
$env:ANDROID_NDK_HOME = Get-ChildItem "$env:ANDROID_HOME\ndk" -Directory |
  Sort-Object Name -Descending |
  Select-Object -First 1 -ExpandProperty FullName
```

## 6. 깨끗한 상태에서 APK 빌드

먼저 로컬 음원이 모두 준비됐는지 검사한다.

```powershell
cd android
.\gradlew.bat :app:verifyLocalAudio
```

그다음 Rust 코어, MIDI 라이브러리, CMake JNI 브리지를 모두 소스에서 다시
빌드하여 디버그 APK를 만든다.

```powershell
.\gradlew.bat clean :app:assembleDebug
```

완성 파일:

```text
android/app/build/outputs/apk/debug/app-debug.apk
```

테스트폰에 설치:

```powershell
& "$env:ANDROID_HOME\platform-tools\adb.exe" devices
& "$env:ANDROID_HOME\platform-tools\adb.exe" install -r `
  ".\app\build\outputs\apk\debug\app-debug.apk"
```

앱 데이터까지 초기화해야 할 때만 기존 앱을 제거한 뒤 설치한다. 일반적인
재검증에서는 `install -r`을 사용해 게임과 저장 데이터를 유지한다.

## 7. 릴리스 APK

현재 프로젝트에는 공개 저장소용 릴리스 개인키가 등록되어 있지 않다.
따라서 다음 명령의 결과는 기본적으로 서명되지 않은 릴리스 APK다.

```powershell
.\gradlew.bat :app:assembleRelease
```

출력 위치:

```text
android/app/build/outputs/apk/release/app-release-unsigned.apk
```

실제 배포용 APK는 기존 개인키를 안전한 로컬 경로에 보관하고 Gradle signing
configuration 또는 Android SDK의 `apksigner`로 서명해야 한다. 개인키와 암호는
GitHub 저장소에 커밋하지 않는다. 이후 업데이트도 반드시 같은 개인키로
서명해야 기존 앱 위에 설치할 수 있다.

## 8. GitHub Actions 주의사항

현재 `.github/workflows/android.yml`은 push 시 Android 빌드를 실행한다. 하지만
GitHub runner에는 위 로컬 음원과 `soundfont.sf2`가 없으므로 `verifyLocalAudio`에서
실패하는 것이 정상이다.

현재 오디오 포함 APK는 로컬 PC에서 빌드한다. 자동 빌드가 필요하면 다음 중
하나를 먼저 구성해야 한다.

- 음원을 합법적으로 제공할 수 있는 별도 비공개 저장소/아티팩트에서 인증 후 받기
- 음원이 준비된 self-hosted GitHub Actions runner 사용

음원 준비 단계 없이 `-PskipCargo`, `-PskipCmake` 또는
`-PprecompiledJava` 옵션으로 통과시키지 않는다. 이 옵션들은 개발 중 부분 검증을
위한 것이며, 깨끗한 정식 소스 빌드를 대신하지 않는다.

## 9. 기능 검증 기준

설치 후 최소한 다음을 직접 확인한다.

1. 제노니아 1: 공격, 대시, 피격, 상자 파괴 효과음
2. 제노니아 2: 공격 및 대시 효과음
3. 제노니아 3: 공격 및 대시 효과음과 프레임 진행 상태
4. 메뉴 이동음과 문 여는 효과음의 지연 여부
5. 배경음의 잡음, 끊김 및 화면 전환 후 재생 상태
6. 앱 종료 후 다시 실행했을 때 게임 목록과 저장 데이터 유지 여부

문제가 있으면 다음 로그를 저장한다.

```powershell
& "$env:ANDROID_HOME\platform-tools\adb.exe" logcat -c
& "$env:ANDROID_HOME\platform-tools\adb.exe" logcat |
  Select-String "WIE-|AndroidRuntime|FATAL EXCEPTION|UnsatisfiedLinkError"
```

현재 폰에서 검증된 로그에는 `WIE-PcmWriter`, `stable play_wave callback installed`,
`WAV effect` 및 `native WAV replacement`가 나타난다.
