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
  "$STAGE/share/doc/neon-rain"

install -m 0755 target/release/neon-rain "$STAGE/bin/neon-rain"
install -m 0644 packaging/linux/neon-rain.desktop \
  "$STAGE/share/applications/neon-rain.desktop"
install -m 0644 LICENSE "$STAGE/share/doc/neon-rain/LICENSE"
install -m 0644 README.md "$STAGE/share/doc/neon-rain/README.md"
install -m 0644 docs/GENERIC_LINUX.md \
  "$STAGE/share/doc/neon-rain/GENERIC_LINUX.md"

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
TARGET="$INSTALL_ROOT/$VERSIONED_NAME"

mkdir -p "$INSTALL_ROOT" "$BIN_DIR" "$APP_DIR"
rm -rf "$TARGET"
cp -a "$SOURCE" "$TARGET"
ln -sfn "$TARGET/neon-rain" "$BIN_DIR/neon-rain"

sed \
  "s|^Exec=.*|Exec=$BIN_DIR/neon-rain|" \
  "$SOURCE/share/applications/neon-rain.desktop" \
  > "$APP_DIR/neon-rain.desktop"

echo "Installed Neon Rain to:"
echo "  $TARGET"
echo
echo "Launcher:"
echo "  $BIN_DIR/neon-rain"
echo
echo "Make sure $BIN_DIR is in PATH."
INSTALLER
chmod 0755 "$STAGE/install-user.sh"

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
