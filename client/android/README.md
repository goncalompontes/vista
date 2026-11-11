# Vista Android client

Native Kotlin/Jetpack Compose client for Vista's authenticated v1 host. Android
is the MVP verification platform; the separate SwiftUI iOS app is not an Android
build dependency. Merged PR [#103](https://github.com/goncalompontes/vista/pull/103)
retired the earlier Skip Lite preview.

The client provides pairing, an authenticated source library and responsive
portrait/landscape streaming screens. The UI uses Sill as its reference, with an
independent Compose implementation; see [Android usability](../../docs/android-usability.md).
It receives H.264 through MediaCodec, exposes granted capabilities, and supports
primary tap, held-button drag, long-press secondary click, two-finger scroll,
physical HID keys and explicitly committed text. Control starts off and stays
off after backgrounding, recovery and source changes. Display capture and
shared-desktop input are labelled explicitly, including for window selections.

Runtime evidence and its limits are in
[Android verification](../../docs/android-verification.md).

## Structure

- `Android/app/src/main/kotlin/vista/module/`: native production sources.
- `ClientState.kt`: immutable state and reducer with connection, selection,
  geometry and input ownership generations.
- `VistaAndroidRoot.kt`, `VistaTheme.kt`: pairing, library, stream and controls.
- `HostControlClient.kt`: authenticated control/media adapters and ordered sender.
- `VideoSurface.kt`, `VideoGeometry.kt`, `RemoteInput.kt`: fitted video, inverse
  coordinate mapping, gesture ownership and guarded input.
- `MediaDecoder.kt`: MediaCodec surface lifecycle; actual rendered frames
  establish live playback.
- `VistaSession.kt`, `VistaWire.kt`, `MediaRecord.kt`: v1 crypto and framing.
- `Android/app/src/test/`: JVM protocol, reducer, geometry and gesture tests.
- `Android/app/src/androidTest/`: opt-in real-host instrumented UI tests.
- `testing/`: disposable GTK target, runner and native-effect validator.

Canonical protocol ownership stays with the Rust host and `docs/fixtures/`.
Rust, Swift and Android consume the shared pairing and stream-scope fixtures.
Design scenarios in `docs/design/client-state-fixtures.json` are semantic
expectations; Android covers the supported v1 subset and does not implement the
future enrollment, clipboard or independent-session cases.

## Build

Requirements: JDK 17+ (21 recommended), Android SDK 36 with build-tools 36.0.0,
and access to Gradle/Maven repositories on the first run.

```sh
export ANDROID_HOME="$HOME/Android/Sdk"
client/android/build-android.sh :app:testDebugUnitTest :app:assembleDebug \
  :app:assembleDebugAndroidTest
adb install -r client/android/Android/app/build/outputs/apk/debug/app-debug.apk
```

The wrapper prepares SDK/Java paths and invokes Gradle. It does not call Xcode
or Skip. CI builds the debug app and test APK and runs JVM tests; it does not
start a compositor or emulator.

## Connect to a host

Use the live `serve` listener, which advertises a media endpoint. Loopback is
sufficient for the Android emulator:

```sh
vista-host serve --confirm --listen 127.0.0.1:7443 --allow-input --json
adb shell am start -n dev.vista.client/vista.module.MainActivity
```

Enter `10.0.2.2`, port `7443` and the generated 64-character pairing secret.
`adb reverse` of 7443 alone does not forward the dynamic media port. The secret
is masked and never saved in activity state or persistent storage.

Choose a source, wait for a rendered frame, then explicitly enable control.
The Keyboard dialog sends committed text only on request; it keeps local text
editing separate from physical remote keys. Backgrounding, cancelling a gesture,
changing video geometry/source or disconnecting releases held input. A source
change creates fresh control/media connections because v1 media records have no
target identity; reconnect restores the selected source with control off.

For a physical phone, use a trusted LAN and an explicit remote listener
(`--listen 0.0.0.0:7443 --allow-remote`). Allow both the listener and announced
media port. Physical Android hardware has not been verified.

## Opt-in emulator check

Boot an Android 36 emulator, install the app and instrumentation APK, build
`vista-host`, and have a working Niri capture/input environment with GTK4 Python
bindings. Port 7443 must be free. This run focuses a disposable test window and
injects input: leave host focus there until it exits.

```sh
adb install -r client/android/Android/app/build/outputs/apk/androidTest/debug/app-debug-androidTest.apk
XDG_RUNTIME_DIR=/run/user/1000 python3 client/android/testing/run-smoke.py \
  --niri-socket "$NIRI_SOCKET"
```

The runner creates private temporary artifacts, transfers a generated invitation
to the emulator, checks UI/lifecycle/rotation/source switching/host restart and
validates actual native drag, secondary click, scroll, exact text and modifier
release. It cleans its own host, injector daemon and test window. Screenshots
and host logs can contain private desktop content or secrets; do not publish
them. `--playback-only` checks video recovery without input and is not input
evidence.

## Remaining work

Audio, clipboard, discovery, independent sessions, production transport/security
migration and physical-decoder validation remain separate milestones. The host
is single-client; it captures the current full output, focuses/fullscreens a
selected window and injects into the shared desktop. Window selection does not
establish isolated window capture or input. Reconnect starts a new authenticated
session rather than resuming the old one.
