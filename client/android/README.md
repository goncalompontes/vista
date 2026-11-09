# Vista Android client (Skip Lite)

Experimental Android port built from the same SwiftUI views and `VistaCore`
models as the iOS client, using [Skip](https://skip.dev) Lite mode. Skip
transpiles Swift/SwiftUI into Kotlin/Jetpack Compose at build time; the shipped
app contains no Skip runtime.

**Status:** the welcome screen runs on an Android 16 emulator. This is a UI
port, not a connected client: discovery, pairing, media and input are not
implemented yet.

## Source of truth

The module sources are generated from the canonical iOS sources:

```sh
./sync-sources.sh
```

- `client/ios/Vista/Views/*.swift` -> `Sources/Vista/`
- `client/core/Sources/VistaCore/*.swift` -> `Sources/Vista/`
- `Sources/Vista/SkipEntry.swift` is Skip-specific (Android entry point).
- `#if SKIP` / `#if !SKIP` guards in the canonical sources keep the shared code
  buildable on both platforms. Transport-only code (`JSONValue`,
  `CapabilitySession`) is excluded from Android for now.

## Linux build

Skip's conventional Gradle pre-build invokes `xcrun`/Xcode. On Linux,
`Android/settings.gradle.kts` calls `skip plugin --no-prebuild` instead, and
`build-android.sh` runs the transpiler directly. `skip export` then stops at its
Darwin/xcodebuild step; the script detects the completed Kotlin output and
continues to Gradle.

Requirements: Swift 6.4, Skip 1.9.20 (Linux artifact), Gradle 9.x, JDK 17+,
Android SDK 36 and an x86_64 emulator or device.

```sh
export ANDROID_HOME="$HOME/Android/Sdk"
./build-android.sh
adb install -r .build/Android/app/outputs/apk/debug/app-debug.apk
adb shell am start -n dev.vista.client/vista.module.MainActivity
```

Create an emulator if needed:

```sh
sdkmanager 'platform-tools' 'emulator' 'platforms;android-36' \
  'build-tools;36.0.0' 'system-images;android-36;google_apis;x86_64'
echo no | avdmanager create avd -n VistaPixel \
  -k 'system-images;android-36;google_apis;x86_64' -d pixel_7
emulator -avd VistaPixel -no-snapshot -no-audio -gpu swiftshader_indirect -no-window
```

## Known gaps

- Unsupported SF Symbols and accessibility modifiers are omitted or replaced on
  Android (`#if SKIP`); the visual design follows the iOS tokens but Android
  uses Material navigation and system conventions.
- No host protocol, video decoder, input injection or clipboard support yet.
- The `Darwin`/Xcode side of the Skip project is not included; the native iOS
  app remains `client/ios`.
