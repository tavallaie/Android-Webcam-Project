#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VARIANT="${1:-debug}"
DEVICE_ID="${2:-}"
PACKAGE_NAME="com.soubhagyajit.awa"
ACTIVITY="${PACKAGE_NAME}/.MainActivity"
APK="$ROOT/release/android/AWA-$VARIANT.apk"

if [[ "$VARIANT" != "debug" && "$VARIANT" != "release" ]]; then
  echo "Usage: $0 [debug|release] [device-id]" >&2
  exit 2
fi

if ! command -v adb >/dev/null 2>&1; then
  echo "adb was not found. Install Android platform-tools and add adb to PATH." >&2
  exit 1
fi

if [[ ! -f "$APK" ]]; then
  echo "APK not found at $APK"
  echo "Building Android $VARIANT APK..."
  "$ROOT/scripts/build-android.sh" "$VARIANT"
fi

mapfile -t DEVICES < <(adb devices | awk '$2 == "device" { print $1 }')

if [[ -z "$DEVICE_ID" ]]; then
  if (( ${#DEVICES[@]} == 0 )); then
    echo "No authorized Android device is connected." >&2
    adb devices
    exit 1
  fi
  if (( ${#DEVICES[@]} > 1 )); then
    echo "Multiple Android devices are connected. Specify one:" >&2
    printf '  %s\n' "${DEVICES[@]}" >&2
    echo "Usage: $0 [$VARIANT] <device-id>" >&2
    exit 2
  fi
  DEVICE_ID="${DEVICES[0]}"
fi

if ! adb -s "$DEVICE_ID" get-state >/dev/null 2>&1; then
  echo "Android device is not available: $DEVICE_ID" >&2
  adb devices
  exit 1
fi

echo "Installing $APK on $DEVICE_ID..."
adb -s "$DEVICE_ID" install -r "$APK"
adb -s "$DEVICE_ID" shell am force-stop "$PACKAGE_NAME"
adb -s "$DEVICE_ID" shell am start -W -n "$ACTIVITY"

echo "Android app installed and launched on $DEVICE_ID."
