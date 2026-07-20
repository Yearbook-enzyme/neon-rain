#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

VERSION="$(
  sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml |
  head -n 1
)"
ARCH="${NEON_RAIN_BUNDLE_ARCH:-x86_64}"
PLATFORM="${NEON_RAIN_BUNDLE_PLATFORM:-linux}"
NAME="neon-rain-${VERSION}-${ARCH}-${PLATFORM}"
DIST="$ROOT/dist"
STAGE="$DIST/$NAME"
ARCHIVE="$DIST/$NAME.tar.gz"

[[ -n "$VERSION" ]] || {
  echo "Could not determine the package version." >&2
  exit 1
}

[[ -x target/release/neon-rain ]] || {
  echo "target/release/neon-rain is missing; run cargo build --release --locked first." >&2
  exit 1
}

rm -rf "$STAGE" "$ARCHIVE" "$ARCHIVE.sha256"
mkdir -p \
  "$STAGE/bin" \
  "$STAGE/share/applications" \
  "$STAGE/share/doc/neon-rain" \
  "$STAGE/share/pixmaps" \
  "$STAGE/share/neon-rain" \
  "$STAGE/tools"

install -m 0755 target/release/neon-rain "$STAGE/bin/neon-rain"
install -m 0644 packaging/linux/neon-rain.desktop \
  "$STAGE/share/applications/neon-rain.desktop"
install -m 0644 LICENSE "$STAGE/share/doc/neon-rain/LICENSE"
install -m 0644 README.md "$STAGE/share/doc/neon-rain/README.md"
install -m 0644 docs/GENERIC_LINUX.md \
  "$STAGE/share/doc/neon-rain/GENERIC_LINUX.md"
install -m 0644 docs/CONFIGURATION.md \
  "$STAGE/share/doc/neon-rain/CONFIGURATION.md"
install -m 0644 docs/CAPTURE.md \
  "$STAGE/share/doc/neon-rain/CAPTURE.md"
install -m 0644 docs/assets/neon-rain-social-preview.png \
  "$STAGE/share/pixmaps/neon-rain.png"
install -m 0644 config/neon-rain.conf \
  "$STAGE/share/neon-rain/config.example.conf"
install -m 0755 scripts/capture-neon-rain.sh \
  "$STAGE/tools/capture-neon-rain.sh"

cat > "$STAGE/neon-rain" <<'LAUNCHER'
#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
BINARY="$ROOT/bin/neon-rain"

if [[ ! -x "$BINARY" ]]; then
  echo "Neon Rain binary is missing: $BINARY" >&2
  exit 1
fi

if ! command -v fc-match >/dev/null 2>&1; then
  cat >&2 <<'MESSAGE'
Neon Rain needs Fontconfig's fc-match command.
Install Fontconfig with your distribution's package manager, then try again.
MESSAGE
  exit 1
fi

missing="$(
  ldd "$BINARY" 2>/dev/null |
  awk '/not found/ { print $1 }'
)"

if [[ -n "$missing" ]]; then
  echo "Neon Rain is missing required shared libraries:" >&2
  printf '  %s\n' $missing >&2
  echo "See share/doc/neon-rain/GENERIC_LINUX.md for runtime requirements." >&2
  exit 1
fi

font="$(
  fc-match -f '%{family}\n' \
    'Migu 1M,Noto Sans Mono CJK JP,Noto Sans CJK JP,Noto Sans Mono,DejaVu Sans Mono,monospace' |
  head -n 1
)"

if [[ -z "$font" ]]; then
  echo "Warning: no usable font was reported by Fontconfig." >&2
fi

exec "$BINARY" "$@"
LAUNCHER
chmod 0755 "$STAGE/neon-rain"

cat > "$STAGE/install-user.sh" <<'INSTALLER'
#!/usr/bin/env bash
set -Eeuo pipefail

SOURCE="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
VERSIONED_NAME="$(basename "$SOURCE")"
INSTALL_ROOT="${XDG_DATA_HOME:-$HOME/.local/share}/neon-rain"
BIN_DIR="$HOME/.local/bin"
APP_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
ICON_DIR="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/512x512/apps"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/neon-rain"
TARGET="$INSTALL_ROOT/$VERSIONED_NAME"

mkdir -p "$INSTALL_ROOT" "$BIN_DIR" "$APP_DIR" "$ICON_DIR" "$CONFIG_DIR"
rm -rf "$TARGET"
cp -a "$SOURCE" "$TARGET"
ln -sfn "$TARGET/neon-rain" "$BIN_DIR/neon-rain"

sed \
  "s|^Exec=.*|Exec=$BIN_DIR/neon-rain|" \
  "$SOURCE/share/applications/neon-rain.desktop" \
  > "$APP_DIR/neon-rain.desktop"

install -m 0644 "$SOURCE/share/pixmaps/neon-rain.png" \
  "$ICON_DIR/neon-rain.png"

if [[ ! -e "$CONFIG_DIR/config.conf" ]]; then
  install -m 0644 "$SOURCE/share/neon-rain/config.example.conf" \
    "$CONFIG_DIR/config.conf"
  echo "Created initial configuration:"
  echo "  $CONFIG_DIR/config.conf"
fi

echo "Installed Neon Rain to:"
echo "  $TARGET"
echo
echo "Launcher:"
echo "  $BIN_DIR/neon-rain"
echo
echo "Make sure $BIN_DIR is in PATH."
INSTALLER
chmod 0755 "$STAGE/install-user.sh"

cat > "$STAGE/uninstall-user.sh" <<'UNINSTALLER'
#!/usr/bin/env bash
set -Eeuo pipefail

INSTALL_ROOT="${XDG_DATA_HOME:-$HOME/.local/share}/neon-rain"
BIN_PATH="$HOME/.local/bin/neon-rain"
APP_PATH="${XDG_DATA_HOME:-$HOME/.local/share}/applications/neon-rain.desktop"
ICON_PATH="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/512x512/apps/neon-rain.png"
CONFIG_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/neon-rain"
STATE_DIR="${XDG_STATE_HOME:-$HOME/.local/state}/neon-rain"
PURGE=false

if [[ "${1:-}" == "--purge" ]]; then
  PURGE=true
fi

rm -f "$BIN_PATH" "$APP_PATH" "$ICON_PATH"
rm -rf "$INSTALL_ROOT"

if [[ "$PURGE" == true ]]; then
  rm -rf "$CONFIG_DIR" "$STATE_DIR"
  echo "Removed Neon Rain, configuration, and remembered session state."
else
  echo "Removed Neon Rain. Configuration and remembered session state were preserved."
  echo "Run with --purge to remove them too."
fi
UNINSTALLER
chmod 0755 "$STAGE/uninstall-user.sh"

"$STAGE/neon-rain" --version
"$STAGE/neon-rain" --help >/dev/null

tar -C "$DIST" -czf "$ARCHIVE" "$NAME"

(
  cd "$DIST"
  sha256sum "$(basename "$ARCHIVE")" > "$(basename "$ARCHIVE").sha256"
)

echo "Created:"
echo "  $ARCHIVE"
echo "  $ARCHIVE.sha256"
