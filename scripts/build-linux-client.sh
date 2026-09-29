#!/usr/bin/env bash
set -euo pipefail

# Build the Tauri desktop client for Linux (.deb and AppImage).
# Usage:
#   ./scripts/build-linux-client.sh
#   ./scripts/build-linux-client.sh appimage
#   ./scripts/build-linux-client.sh deb

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLIENT="$ROOT/desktop"
IMAGE="${AWC_LINUX_IMAGE:-awc-linux-build:local}"
BUNDLES="${1:-deb,appimage}"

# Check whether a command is available on PATH.
need_cmd() {
  command -v "$1" >/dev/null 2>&1
}

# Return whether the host has all native Linux build prerequisites.
host_can_build() {
  need_cmd pnpm && need_cmd cargo && need_cmd pkg-config || return 1
  pkg-config --exists webkit2gtk-4.1 gtk+-3.0 libavcodec libv4l2 || return 1
  local major
  major="$(pkg-config --modversion libavcodec | cut -d. -f1)"
  # FFmpeg 8 ships libavcodec 62.
  [[ "$major" -ge 62 ]]
}

# Remove generated bundle files from a previous build.
clean_bundle_outputs() {
  find "$CLIENT/src-tauri/target/release/bundle/deb" \
    "$CLIENT/src-tauri/target/release/bundle/appimage" \
    -maxdepth 1 -type f \( -name '*.deb' -o -name '*.AppImage' \) -delete 2>/dev/null || true
}

# Remove stale Rust artifacts that reference the old workspace path.
clean_stale_rust_target() {
  local target="$CLIENT/src-tauri/target"
  [[ -d "$target" ]] || return 0

  if rg -a -l -m 1 'CLIENT/tauri-client|/work/CLIENT' "$target" >/dev/null 2>&1; then
    echo "Removing stale Rust build cache from the old client path."
    cargo clean --manifest-path "$CLIENT/src-tauri/Cargo.toml"
  fi
}

# Build Linux bundles directly on the host.
run_native() {
  local build_bundles="$BUNDLES"
  [[ "$BUNDLES" == "deb" ]] && build_bundles="deb,appimage"
  echo "Building on the host with pnpm."
  clean_bundle_outputs
  clean_stale_rust_target
  pnpm --dir "$CLIENT" install --frozen-lockfile
  pnpm --dir "$CLIENT" tauri build --bundles "$build_bundles"
  "$ROOT/scripts/isolate-appimage-profile.sh"
}

# Build Linux bundles inside the reproducible Docker environment.
run_docker() {
  local build_bundles="$BUNDLES"
  [[ "$BUNDLES" == "deb" ]] && build_bundles="deb,appimage"
  need_cmd docker || {
    echo "Install Docker, or install GTK, WebKit, v4l, and FFmpeg 8 headers, then rerun." >&2
    exit 1
  }
  need_cmd node && need_cmd pnpm && need_cmd rustc && need_cmd cargo || {
    echo "This Docker build uses host Node, pnpm, and Rust. Install those tools first." >&2
    exit 1
  }

  clean_stale_rust_target
  clean_bundle_outputs

  local node_dir pnpm_dir cargo_home rustup_home
  node_dir="$(dirname "$(command -v node)")"
  pnpm_dir="$(dirname "$(command -v pnpm)")"
  cargo_home="${CARGO_HOME:-$HOME/.cargo}"
  rustup_home="${RUSTUP_HOME:-$HOME/.rustup}"

  echo "Building Docker image $IMAGE"
  docker build -t "$IMAGE" -f "$CLIENT/Dockerfile.linux" "$CLIENT"

  echo "Building Linux bundles ($BUNDLES) in Docker."
  docker run --rm \
    --user "$(id -u):$(id -g)" \
    -e HOME=/tmp \
    -e USER="${USER:-awc}" \
    -e PATH="/opt/pnpm:/opt/node/bin:/opt/cargo/bin:/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    -e CARGO_HOME=/opt/cargo \
    -e RUSTUP_HOME=/opt/rustup \
    -e CARGO_TERM_COLOR=always \
    -e CI=true \
    -v "$ROOT":/work \
    -v "$cargo_home":/opt/cargo \
    -v "$rustup_home":/opt/rustup \
    -v "$node_dir":/opt/node/bin \
    -v "$pnpm_dir":/opt/pnpm \
    -w /work \
    "$IMAGE" \
    bash -lc "pnpm --dir desktop install --frozen-lockfile --store-dir /tmp/pnpm-store && pnpm --dir desktop tauri build --bundles '$build_bundles' && bash scripts/isolate-appimage-profile.sh"
}

# Verify that extracted package symlinks resolve within the extracted filesystem.
check_no_dangling_symlinks() {
  local root="$1" runtime_file runtime_target
  while IFS= read -r -d '' runtime_file; do
    runtime_target="$(readlink "$runtime_file")"
    if [[ "$runtime_target" == /* ]]; then
      runtime_target="$root$runtime_target"
    else
      runtime_target="$(dirname "$runtime_file")/$runtime_target"
    fi
    if [[ ! -e "$runtime_target" ]]; then
      echo "Dangling runtime symlink in Debian package: $runtime_file" >&2
      return 1
    fi
  done < <(find "$root/usr/lib/awc" -type l -print0)
}

# Bundle FFmpeg 8 and its private runtime closure into the Debian package.
bundle_ffmpeg_in_deb() {
  local deb resolved_runtime_file verify_root
  local app_lib="$CLIENT/src-tauri/target/release/bundle/appimage/AWC.AppDir/usr/lib"
  local temp_root ffmpeg_lib runtime_file ffmpeg_dependency
  local -a ffmpeg_files=(
    libavcodec.so.62
    libavdevice.so.62
    libavfilter.so.11
    libavformat.so.62
    libavutil.so.60
    libswresample.so.6
    libswscale.so.9
  )
  local -a runtime_files=()

  [[ ",$BUNDLES," == *,deb,* ]] || return 0
  local -a deb_files=("$CLIENT/src-tauri/target/release/bundle/deb/"*.deb)
  (( ${#deb_files[@]} == 1 )) || {
    echo "Expected exactly one Debian bundle, found ${#deb_files[@]}." >&2
    exit 1
  }
  deb="${deb_files[0]}"
  if [[ ! -e "$app_lib/${ffmpeg_files[0]}" ]]; then
    ffmpeg_lib="$(ldconfig -p | awk '/libavcodec.so.62 / {print $NF; exit}' | xargs -r dirname)"
    app_lib="$ffmpeg_lib"
  fi
  [[ -d "$app_lib" ]] || {
    echo "FFmpeg 8 libraries were not found for the Debian package." >&2
    exit 1
  }

  for ffmpeg_file in "${ffmpeg_files[@]}"; do
    [[ -e "$app_lib/$ffmpeg_file" ]] || {
      echo "Missing FFmpeg 8 library: $app_lib/$ffmpeg_file" >&2
      exit 1
    }
    runtime_files+=("$app_lib/$ffmpeg_file")
    while IFS= read -r ffmpeg_dependency; do
      runtime_files+=("$ffmpeg_dependency")
    done < <(LD_LIBRARY_PATH="$app_lib" ldd "$app_lib/$ffmpeg_file" | awk -v prefix="$app_lib/" 'index($3, prefix) == 1 {print $3}')
  done

  temp_root="$(mktemp -d)"
  dpkg-deb -x "$deb" "$temp_root/root"
  dpkg-deb -e "$deb" "$temp_root/control"
  mkdir "$temp_root/root/DEBIAN"
  cp "$temp_root/control"/* "$temp_root/root/DEBIAN/"
  mkdir -p "$temp_root/root/usr/lib/awc"
  for runtime_file in "${runtime_files[@]}"; do
    resolved_runtime_file="$(readlink -f "$runtime_file")"
    [[ -f "$resolved_runtime_file" ]] || {
      echo "Runtime library target is missing: $runtime_file" >&2
      exit 1
    }
    cp -a "$runtime_file" "$temp_root/root/usr/lib/awc/"
    if [[ "$resolved_runtime_file" != "$runtime_file" ]]; then
      cp -a "$resolved_runtime_file" "$temp_root/root/usr/lib/awc/"
    fi
  done
  check_no_dangling_symlinks "$temp_root/root"
  (
    cd "$temp_root/root"
    find . -type f -not -path './DEBIAN/*' -printf '%P\n' | sort | xargs md5sum > DEBIAN/md5sums
    sed -i "s/^Installed-Size: .*/Installed-Size: $(du -sk . | cut -f1)/" DEBIAN/control
  )
  dpkg-deb -b "$temp_root/root" "$deb.repacked" >/dev/null
  verify_root="$(mktemp -d)"
  dpkg-deb -x "$deb.repacked" "$verify_root"
  check_no_dangling_symlinks "$verify_root"
  mv "$deb.repacked" "$deb"
}

if host_can_build; then
  run_native
else
  echo "Host is missing Linux desktop or FFmpeg 8 build packages. Using Docker."
  run_docker
fi

bundle_ffmpeg_in_deb

echo
echo "Bundles:"
RELEASE_DIR="$ROOT/release/linux"
mkdir -p "$RELEASE_DIR"

shopt -s nullglob
BUNDLES_TO_COPY=(
  "$CLIENT/src-tauri/target/release/bundle/deb/"*.deb
  "$CLIENT/src-tauri/target/release/bundle/appimage/"*.AppImage
)
expected_bundle_count=1
[[ ",$BUNDLES," == *,deb,* ]] && expected_bundle_count=2
if (( ${#BUNDLES_TO_COPY[@]} != expected_bundle_count )); then
  echo "Expected $expected_bundle_count Linux bundle(s), found ${#BUNDLES_TO_COPY[@]}." >&2
  exit 1
fi

for bundle in "${BUNDLES_TO_COPY[@]}"; do
  cp -- "$bundle" "$RELEASE_DIR/"
done

ls -lah "$RELEASE_DIR"/*
