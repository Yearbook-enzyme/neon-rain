#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

for command in cargo dpkg dpkg-deb dpkg-shlibdeps sha256sum; do
  command -v "$command" >/dev/null 2>&1 || {
    echo "Missing required command: $command" >&2
    exit 1
  }
done

VERSION="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"
[[ -n "$VERSION" ]] || {
  echo "Could not determine version from Cargo.toml" >&2
  exit 1
}

BINARY="$ROOT/target/release/neon-rain"
[[ -x "$BINARY" ]] || {
  echo "Missing release binary; run cargo build --release --locked first" >&2
  exit 1
}

ARCH="${DEB_ARCH:-$(dpkg --print-architecture)}"
DEB_VERSION="${VERSION/-alpha./~alpha.}-1"
DIST="$ROOT/dist"
WORK="$(mktemp -d)"
STAGE="$WORK/package"
DEB="$DIST/neon-rain_${DEB_VERSION}_${ARCH}.deb"

cleanup() {
  rm -rf "$WORK"
}
trap cleanup EXIT

mkdir -p \
  "$WORK/debian" \
  "$STAGE/DEBIAN" \
  "$STAGE/usr/bin" \
  "$STAGE/usr/share/applications" \
  "$STAGE/usr/share/doc/neon-rain" \
  "$STAGE/usr/share/icons/hicolor/512x512/apps" \
  "$STAGE/usr/share/neon-rain" \
  "$DIST"

cat > "$WORK/debian/control" <<'CONTROL'
Source: neon-rain
Section: graphics
Priority: optional
Maintainer: Logan Campbell <144038028+Yearbook-enzyme@users.noreply.github.com>
Standards-Version: 4.7.2
Rules-Requires-Root: no

Package: neon-rain
Architecture: any
Depends: ${shlibs:Depends}, ${misc:Depends}
Description: Living, music-reactive Matrix rain visualizer
 Neon Rain renders a persistent GPU-accelerated Matrix rain world with
 cinematic camera movement, media coupling, and optional music reactivity.
CONTROL

shlib_output="$(
  cd "$WORK"
  dpkg-shlibdeps -O -e"$BINARY"
)"
shlib_deps="$(printf '%s\n' "$shlib_output" | sed -n 's/^shlibs:Depends=//p')"
[[ -n "$shlib_deps" ]] || {
  echo "dpkg-shlibdeps did not produce runtime dependencies" >&2
  exit 1
}

install -m 0755 "$BINARY" "$STAGE/usr/bin/neon-rain"
install -m 0644 packaging/linux/neon-rain.desktop \
  "$STAGE/usr/share/applications/neon-rain.desktop"
install -m 0644 docs/assets/neon-rain-social-preview.png \
  "$STAGE/usr/share/icons/hicolor/512x512/apps/neon-rain.png"
install -m 0644 config/neon-rain.conf \
  "$STAGE/usr/share/neon-rain/config.example.conf"
install -m 0644 README.md "$STAGE/usr/share/doc/neon-rain/README.md"
install -m 0644 LICENSE "$STAGE/usr/share/doc/neon-rain/LICENSE"
install -m 0644 docs/DEBIAN.md "$STAGE/usr/share/doc/neon-rain/DEBIAN.md"
install -m 0644 docs/CONFIGURATION.md \
  "$STAGE/usr/share/doc/neon-rain/CONFIGURATION.md"

installed_size="$(du -sk "$STAGE/usr" | awk '{print $1}')"

cat > "$STAGE/DEBIAN/control" <<CONTROL
Package: neon-rain
Version: $DEB_VERSION
Section: graphics
Priority: optional
Architecture: $ARCH
Maintainer: Logan Campbell <144038028+Yearbook-enzyme@users.noreply.github.com>
Installed-Size: $installed_size
Depends: $shlib_deps, fontconfig
Recommends: libvulkan1 | libgl1, pipewire-bin, playerctl, fonts-noto-cjk
Suggests: strawberry
Homepage: https://github.com/Yearbook-enzyme/neon-rain
Description: Living, music-reactive Matrix rain visualizer
 Neon Rain renders a persistent GPU-accelerated Matrix rain world with
 cinematic camera movement, media coupling, and optional music reactivity.
CONTROL

rm -f "$DEB" "$DEB.sha256"
dpkg-deb --root-owner-group --build "$STAGE" "$DEB"
(
  cd "$DIST"
  sha256sum "$(basename "$DEB")" > "$(basename "$DEB").sha256"
)

echo "Created:"
echo "  $DEB"
echo "  $DEB.sha256"
