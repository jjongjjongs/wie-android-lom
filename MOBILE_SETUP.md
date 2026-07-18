# Mobile setup

Repository: jjongjjongs/wie-android-lom

This first workflow builds WIE on GitHub Actions and runs Legend of Master for 45 seconds under Xvfb. It uploads stderr/stdout logs so the first missing LGT Java/import function can be identified before creating the Android arm64 JNI library.

The workflow is `.github/workflows/lom-diagnostic.yml`.
