# AppImage blank page and FFmpeg ABI mismatch

## Objective

Make Linux AppImage and Debian builds start with their frontend assets and with a supported FFmpeg runtime.

## Context

The repository is `Android-Webcam-Project` on `main`. No ticket key exists.

## Impact

- Change `desktop/vite.config.js` to emit relative production asset URLs.
- Keep `desktop/Dockerfile.linux` on the FFmpeg 8 build image.
- Change `scripts/build-linux-client.sh` to repack the Debian bundle with local FFmpeg 8 libraries.
- Change `desktop/src-tauri/build.rs` to set a runtime search path for AppImage and Debian layouts.
- Do not delete files.

## Contract impact

No API keys, background jobs, or database changes.

## Size

- `desktop/vite.config.js`: 4 lines.
- `desktop/Dockerfile.linux`: no change after confirming the FFmpeg 8 build.
- `scripts/build-linux-client.sh`: about 48 lines.
- `desktop/src-tauri/build.rs`: 4 lines.
- `desktop/src-tauri/tauri.conf.json`: no change.

## Pattern

Copy the existing Vite configuration style, the existing Docker package list, and the existing Tauri Linux dependency style.

## Tests

Use the built `index.html` asset references and `readelf` dependency output as focused regression checks before the full build.

## Steps

1. Confirm the current branch and dependency mismatch.
2. Apply the smallest packaging and asset-path changes.
3. Build the Linux bundles.
4. Verify frontend paths, ELF dependencies, Debian metadata, and AppImage contents.
5. Run the project verification commands.
6. Commit, review, and prepare the pull request.

## Risk

The FFmpeg crate or host package version may not support the selected ABI. Check the build output and the final ELF `NEEDED` entries.

The AppImage may still require host GTK or WebKit libraries. Check the extracted payload and launch diagnostics after the FFmpeg mismatch is removed.

## Verification

- `pnpm --dir desktop install --frozen-lockfile`
- `./scripts/build-linux-client.sh`
