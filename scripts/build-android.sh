#!/usr/bin/env bash
set -euo pipefail

# Build the Android client from the repository root.
# Usage:
#   ./scripts/build-android.sh
#   ./scripts/build-android.sh debug
#   ./scripts/build-android.sh release

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
ANDROID_DIR="$ROOT/android"
VARIANT="${1:-debug}"

case "$VARIANT" in
  debug|release)
    ;;
  *)
    echo "Usage: $0 [debug|release]" >&2
    exit 2
    ;;
esac

command -v java >/dev/null 2>&1 || {
  echo "Java is required. Install JDK 21 or set PATH." >&2
  exit 1
}

[[ -f "$ANDROID_DIR/gradlew" ]] || {
  echo "Android Gradle project not found: $ANDROID_DIR" >&2
  exit 1
}

if [[ -z "${ANDROID_HOME:-}" && -z "${ANDROID_SDK_ROOT:-}" ]]; then
  echo "ANDROID_HOME or ANDROID_SDK_ROOT must point to the Android SDK." >&2
  exit 1
fi

TASK="assemble${VARIANT^}"

echo "Building Android $VARIANT APK..."
(
  cd "$ANDROID_DIR"
  ./gradlew ":app:$TASK" --no-daemon
)

shopt -s nullglob
APKS=("$ANDROID_DIR/app/build/outputs/apk/$VARIANT"/app-"$VARIANT"*.apk)
if (( ${#APKS[@]} > 0 )); then
  APK="${APKS[0]}"
  echo
  echo "APK: $APK"
else
  echo "Build completed, but no APK was found under: $ANDROID_DIR/app/build/outputs/apk/$VARIANT" >&2
  exit 1
fi
