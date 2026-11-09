#!/usr/bin/env bash
# Build the Vista Skip (SwiftUI -> Jetpack Compose) Android app on Linux.
#
# Requirements: Swift 6.4, Skip 1.9.20, Gradle, JDK 17+, Android SDK 36.
# The script expects `skip`, `gradle`, `adb` and the Android SDK on PATH or in
# the conventional locations below.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

export PATH="$HOME/.local/share/swiftly/bin:$HOME/.local/bin:$PATH"
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export PATH="$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$PATH"
if [ -z "${JAVA_HOME:-}" ]; then
    JAVA_HOME="$(dirname "$(dirname "$(readlink -f "$(command -v java)")")")"
    export JAVA_HOME
fi

cd "$here"
# Transpile the SwiftUI module to Kotlin/Compose. Skip's export then tries a
# Darwin/xcodebuild step, which does not exist on Linux; the transpiled output
# is complete by then, so a non-zero exit is expected.
if ! skip export --debug --host-build transpile --plain; then
    if ! find .build/plugins/outputs -path '*/Vista/src/main/kotlin' -type d | grep -q .; then
        echo "Skip transpilation failed before producing Kotlin sources" >&2
        exit 1
    fi
    echo "skip export stopped at the unavailable xcodebuild step; using the transpiled Kotlin" >&2
fi

cd Android
gradle assembleDebug --no-daemon --console=plain
apk="$(find "$here/.build/Android/app/outputs/apk/debug" -name '*.apk' | head -1)"
echo "APK: $apk"
echo "Install: adb install -r '$apk' && adb shell am start -n dev.vista.client/vista.module.MainActivity"
