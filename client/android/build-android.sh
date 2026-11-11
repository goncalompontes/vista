#!/usr/bin/env bash
# Build the native Vista Android client (Kotlin + Jetpack Compose).
#
# Requirements: JDK 17+, Android SDK 36 (platform + build-tools) and a
# reachable x86_64 emulator or physical device for installation.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

export ANDROID_HOME="${ANDROID_HOME:-$HOME/Android/Sdk}"
export PATH="$ANDROID_HOME/platform-tools:$ANDROID_HOME/emulator:$PATH"
if [ -z "${JAVA_HOME:-}" ]; then
    JAVA_HOME="$(dirname "$(dirname "$(readlink -f "$(command -v java)")")")"
    export JAVA_HOME
fi

cd "$here/Android"
if [ -x ./gradlew ]; then
    ./gradlew :app:assembleDebug "$@"
else
    gradle :app:assembleDebug "$@"
fi

apk="$here/Android/app/build/outputs/apk/debug/app-debug.apk"
if [ ! -s "$apk" ]; then
    echo "Gradle reported success but the expected APK is missing: $apk" >&2
    exit 1
fi
echo "APK: $apk"
echo "Install: adb install -r '$apk' && adb shell am start -n dev.vista.client/vista.module.MainActivity"
