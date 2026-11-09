# Vista Android client (Skip Lite)

Experimental Android port built from the same SwiftUI views and `VistaCore`
models as the iOS client, using [Skip](https://skip.dev) Lite mode. Skip
transpiles Swift/SwiftUI into Kotlin/Jetpack Compose at build time; the shipped
app contains no Skip runtime.

**Status:** the native Android entry screen has a working encrypted control
path. It pairs with a Vista host, receives the capability snapshot, displays
the live action policy and sends authorized semantic actions. The control path
has been exercised against the Rust host on an Android 16 emulator. Media,
touch/keyboard input and clipboard remain separate increments.

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

## Physical Android prototype

The debug APK can run on a physical Android phone. Build it, install it with
ADB, and make sure the phone can reach the host over the same LAN:

```sh
./build-android.sh
adb install -r .build/Android/app/outputs/apk/debug/app-debug.apk
adb shell am start -n dev.vista.client/vista.module.MainActivity
```

Start a host listener on a reachable address with an explicit policy. For a
real Niri window, replace `WINDOW_ID` with an existing window ID:

```sh
vista-host protocol-serve --confirm --listen 0.0.0.0:7443 \
  --allow-remote --window WINDOW_ID --allow-window-actions --catalog --json
```

Allow TCP port 7443 through the host firewall, then enter the host IP, port
and 64-character pairing secret shown by the listener. Wi-Fi networks with
client isolation must allow phone-to-host traffic.

## Known gaps

- Unsupported SF Symbols and accessibility modifiers are omitted or replaced on
  Android (`#if SKIP`); the visual design follows the iOS tokens but Android
  uses Material navigation and system conventions.
- Host pairing, encrypted control records, capability snapshots and semantic
  actions are implemented. Host live media transport, Android video decoding,
  native input injection, touch/keyboard UI, audio and clipboard are not yet
  wired.
- The `Darwin`/Xcode side of the Skip project is not included; the native iOS
  app remains `client/ios`.
