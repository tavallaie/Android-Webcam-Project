#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
APPIMAGE_DIR="$ROOT/desktop/src-tauri/target/release/bundle/appimage"
APPDIR="$APPIMAGE_DIR/AWC.AppDir"
APP_RUN="$APPDIR/AppRun"
target_appimage="$(find "$APPIMAGE_DIR" -maxdepth 1 -type f -name '*.AppImage' -print -quit)"

[[ -f "$APP_RUN" ]] || exit 0

if ! grep -q 'AWC AppImage profile isolation' "$APP_RUN"; then
  sed -i '/^this_dir=/i\
# AWC AppImage profile isolation: its bundled WebKit must not share state with the DEB.\
AWC_PROFILE_ROOT="${AWC_PROFILE_ROOT:-${XDG_DATA_HOME:-$HOME/.local/share}/awc-appimage}"\
export XDG_DATA_HOME="$AWC_PROFILE_ROOT/data"\
export XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-$HOME/.config}/awc-appimage"\
export XDG_CACHE_HOME="${XDG_CACHE_HOME:-$HOME/.cache}/awc-appimage"' "$APP_RUN"
fi

appimage_tool=""
for cache_dir in "${XDG_CACHE_HOME:-$HOME/.cache}/tauri" /tmp/.cache/tauri; do
  if [[ -z "$appimage_tool" && -f "$cache_dir/linuxdeploy-plugin-appimage.AppImage" ]]; then
    appimage_tool="$cache_dir/linuxdeploy-plugin-appimage.AppImage"
  fi
done

[[ -n "$appimage_tool" ]] || {
  echo "Could not find linuxdeploy AppImage repacker." >&2
  exit 1
}

"$appimage_tool" --appimage-extract-and-run --appdir "$APPDIR"

generated_appimage="$ROOT/AWC-x86_64.AppImage"
if [[ -f "$generated_appimage" && -n "$target_appimage" ]]; then
  mv "$generated_appimage" "$target_appimage"
fi
