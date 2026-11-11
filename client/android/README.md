# Vista Android client

A native Kotlin/Jetpack Compose client. It speaks the same Vista v1 control
protocol as the Rust host and the Swift `VistaCore` client; it is not a
transpiled copy of the SwiftUI views.

**Status (2026-11-11):** pairing, the encrypted control record layer, the live
window catalog, the capability snapshot and semantic `action.invoke` work. The
debug APK builds from a clean tree, its wire core passes the shared
Rust/Swift pairing vector as JVM tests, and it installs and launches on an
Android 36 x86_64 emulator. **Live host media, native input, audio and
clipboard are not wired yet.**

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
        VistaAndroidRoot.kt        # pairing form, catalog and action UI
        HostControlClient.kt       # sockets, pairing, framed AEAD session
        VistaWire.kt               # protocol primitives, JVM-testable
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

```sh
# Control-only listener (catalog + actions; no media yet)
vista-host protocol-serve --confirm --listen 0.0.0.0:7443 \
  --allow-remote --window WINDOW_ID --allow-window-actions --catalog --json

# Emulator with host port forwarding instead of a LAN address
adb reverse tcp:7443 tcp:7443
adb install -r client/android/Android/app/build/outputs/apk/debug/app-debug.apk
adb shell am start -n dev.vista.client/vista.module.MainActivity
```

Enter the host address (or `127.0.0.1` with `adb reverse`), port and the
64-character pairing secret from the listener invite.

For a physical phone, put phone and host on the same LAN, allow TCP 7443 through
the host firewall, and use the host's LAN address. Wi-Fi client isolation blocks
this path.

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

- No media receive path: the `media_offer`/media handshake, AEAD media records
  and `MediaCodec` decoding to a `SurfaceView` are the next increment.
- No touch, pointer or keyboard input; the host already accepts one portable
  pointer event through the transport.
- No retry/reconnect policy beyond reporting the failure, and no discovery.
- The activity is control-only; there is no stream screen yet.
