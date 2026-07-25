#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

APP_ID="io.github.yearbook_enzyme.neon_rain"
MANIFEST="packaging/flatpak/${APP_ID}.yml"
BUILD_DIR="flatpak-build"
REPO_DIR="flatpak-repo"
DIST_DIR="dist"
BUNDLE="$DIST_DIR/neon-rain-${APP_ID}.flatpak"

for command in flatpak flatpak-builder appstreamcli eu-strip eu-elfcompress; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "Missing required command: $command" >&2
    exit 1
  }
done

[[ -f packaging/flatpak/cargo-sources.json ]] || {
  echo "Missing packaging/flatpak/cargo-sources.json" >&2
  echo "Regenerate it with the Flatpak cargo generator before building." >&2
  exit 1
}

flatpak remote-add --if-not-exists --user \
  flathub https://dl.flathub.org/repo/flathub.flatpakrepo

flatpak install --user -y flathub \
  org.freedesktop.Platform//25.08 \
  org.freedesktop.Sdk//25.08 \
  org.freedesktop.Sdk.Extension.rust-stable//25.08

rm -rf "$BUILD_DIR" "$REPO_DIR" "$BUNDLE"
mkdir -p "$DIST_DIR"

flatpak-builder \
  --force-clean \
  --user \
  --install-deps-from=flathub \
  --repo="$REPO_DIR" \
  --install \
  "$BUILD_DIR" \
  "$MANIFEST"

flatpak build-bundle \
  "$REPO_DIR" \
  "$BUNDLE" \
  "$APP_ID" \
  --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo

sha256sum "$BUNDLE" > "$BUNDLE.sha256"

echo "Built and installed $APP_ID"
echo "Run: flatpak run $APP_ID"
echo "Bundle: $BUNDLE"
