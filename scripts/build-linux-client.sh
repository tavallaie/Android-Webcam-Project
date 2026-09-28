#!/usr/bin/env bash
set -euo pipefail

# Build the Tauri desktop client for Linux (.deb and AppImage).
# Usage:
#   ./scripts/build-linux-client.sh
#   ./scripts/build-linux-client.sh appimage
#   ./scripts/build-linux-client.sh deb

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CLIENT="$ROOT/CLIENT/tauri-client"
IMAGE="${AWC_LINUX_IMAGE:-awc-linux-build:local}"
BUNDLES="${1:-deb,appimage}"

need_cmd() {
  command -v "$1" >/dev/null 2>&1
}

host_can_build() {
  need_cmd pnpm && need_cmd cargo && need_cmd pkg-config || return 1
  pkg-config --exists webkit2gtk-4.1 gtk+-3.0 libavcodec libv4l2 || return 1
  local major
  major="$(pkg-config --modversion libavcodec | cut -d. -f1)"
  # FFmpeg 8 ships libavcodec 62.
  [[ "$major" -ge 62 ]]
}

run_native() {
  echo "Building on the host with pnpm."
  pnpm --dir "$CLIENT" install --frozen-lockfile
  pnpm --dir "$CLIENT" tauri build --bundles "$BUNDLES"
}

run_docker() {
  need_cmd docker || {
    echo "Install Docker, or install GTK, WebKit, v4l, and FFmpeg 8 headers, then rerun." >&2
    exit 1
  }
  need_cmd node && need_cmd pnpm && need_cmd rustc && need_cmd cargo || {
    echo "This Docker build uses host Node, pnpm, and Rust. Install those tools first." >&2
    exit 1
  }

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
    bash -lc "pnpm --dir CLIENT/tauri-client install --frozen-lockfile --store-dir /tmp/pnpm-store && pnpm --dir CLIENT/tauri-client tauri build --bundles '$BUNDLES'"
}

if host_can_build; then
  run_native
else
  echo "Host is missing Linux desktop or FFmpeg 8 build packages. Using Docker."
  run_docker
fi

echo
echo "Bundles:"
ls -lah "$CLIENT/src-tauri/target/release/bundle/deb/"*.deb 2>/dev/null || true
ls -lah "$CLIENT/src-tauri/target/release/bundle/appimage/"*.AppImage 2>/dev/null || true
