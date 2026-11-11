# Vista Android client

A native Kotlin/Jetpack Compose client. It speaks the same Vista v1 control
protocol as the Rust host and the Swift `VistaCore` client; it is not a
transpiled copy of the SwiftUI views.

**Status (2026-11-11):** pairing, the encrypted control record layer, the live
window catalog, the capability snapshot and semantic `action.invoke` work, and
the client now plays live host H.264 through `MediaCodec`, forwards touch as
portable pointer events, maps hardware key events and commits typed text. The debug APK builds from a clean tree, its wire core
passes the shared Rust/Swift pairing vector as JVM tests, and a headless
Android 36 x86_64 emulator streamed 1920x1080 from the real host while the host
accepted 12 touch-derived input events. See
[Android verification](../../docs/android-verification.md). **Drag/scroll with a held button,
text commit as a typed run and audio/clipboard are not wired or exercised yet;
no physical device has been tested.**

Earlier revisions transpiled the SwiftUI client with Skip Lite. That path is
retired: the shared `VistaCore` sources are iOS-only, the clean transpile did
not type-check (issue #96), and media decode has to be Kotlin anyway. The iOS
client remains a separate SwiftUI app under `client/ios`.

## Structure

```text
client/android/
  README.md
  build-android.sh                 # SDK/Java environment + Gradle build
  Android/
    settings.gradle.kts
    build.gradle.kts
    gradle/libs.versions.toml      # AGP, Kotlin, Compose BOM, SDK levels
    app/
      build.gradle.kts
      src/main/AndroidManifest.xml
      src/main/kotlin/vista/module/
        MainActivity.kt            # native Compose entry point
        VistaAndroidRoot.kt        # connection form, video surface, catalog/actions
        HostControlClient.kt       # control + media connections, input, UI callbacks
        VistaSession.kt            # pairing handshake and secure/record framing
        VistaWire.kt               # protocol primitives, JVM-testable
        MediaRecord.kt             # media record + Annex-B helpers, JVM-testable
        MediaDecoder.kt            # MediaCodec -> SurfaceView
      src/test/kotlin/...          # shared-vector conformance tests
```

Canonical protocol ownership stays with the Rust host and the fixtures in
`docs/fixtures/`. `VistaWire.kt` is a mirror, and `VistaWireTest` checks it
against `docs/fixtures/pairing-v1.json`; protocol changes must land in the
host, the Swift core and this file together.

## Build

Requirements: JDK 17+ (21 recommended), Android SDK 36 with build-tools 36.0.0,
and network access to the Gradle/Maven repositories on the first run.

```sh
export ANDROID_HOME="$HOME/Android/Sdk"
client/android/build-android.sh          # :app:assembleDebug
client/android/Android/gradlew -p client/android/Android :app:testDebugUnitTest
```

`build-android.sh` only prepares `ANDROID_HOME`/`PATH`/`JAVA_HOME` and invokes
the Gradle wrapper; it does not call `xcrun`, Xcode or Skip.

The debug APK is written to
`client/android/Android/app/build/outputs/apk/debug/app-debug.apk`.

## Run against a host

The client needs the live service (`serve`), not the control-only
`protocol-serve` listener: only `serve` announces a media endpoint.

```sh
# Live desktop capture with a media endpoint; --allow-input enables touch
vista-host serve --confirm --listen 0.0.0.0:7443 \
  --secret-hex <32-byte hex> --allow-remote --allow-input --once --json

adb install -r client/android/Android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n dev.vista.client/vista.module.MainActivity
```

The emulator reaches the host through `10.0.2.2` (host loopback). `adb reverse`
does not cover the media port, because the host announces it dynamically, so use
`10.0.2.2` rather than `127.0.0.1` on an emulator.

For a physical phone, put phone and host on the same LAN, allow TCP 7443 and the
announced media port through the host firewall, and use the host's LAN address.
Wi-Fi client isolation blocks this path.

Enter the host address, port and the 64-character pairing secret from the invite
JSON. After pairing the client selects the `desktop` target automatically; the
catalog row or the `Desktop` button switches targets, and touch on the video
area sends absolute pointer events when the host granted input.

Emulator creation (already present on the development host as `VistaPixel`):

```sh
sdkmanager 'platform-tools' 'emulator' 'platforms;android-36' \
  'build-tools;36.0.0' 'system-images;android-36;google_apis;x86_64'
echo no | avdmanager create avd -n VistaPixel \
  -k 'system-images;android-36;google_apis;x86_64' -d pixel_7
emulator -avd VistaPixel -no-snapshot -no-audio -gpu swiftshader_indirect -no-window
```

## CI

`Portable client checks` runs two jobs on Ubuntu:

- **Android build and wire tests:** `./gradlew :app:testDebugUnitTest
  :app:assembleDebug` on JDK 21 with Android SDK 36.
- **VistaCore tests:** `swift test --package-path client/core` in a Swift 6.0
  container.

The Android job does not start an emulator; device/emulator playback evidence is
recorded separately.

## Known gaps

- Drag with a button held across a gesture, scroll and long-press/secondary
  click are not wired; the client sends absolute pointer position, the primary
  button, hardware key presses (letters, digits, arrows, modifiers and common
  editing keys) and committed text. Only Shift has been exercised end to end.
- No retry/reconnect policy beyond reporting the failure, and no discovery.
- Media decode is verified only on an x86_64 emulator; hardware-decoder
  behaviour on a phone is untested.
- Audio and clipboard are not implemented.
- The host capture path still records the whole output (a selected window is
  focused and fullscreened) and the service is single-client with no TLS.
