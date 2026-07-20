#!/usr/bin/env bash
set -Eeuo pipefail

MODE="${1:-help}"
OUTPUT="${2:-}"
STAMP="$(date +%Y%m%d-%H%M%S)"
PICTURES="${XDG_PICTURES_DIR:-$HOME/Pictures}"
VIDEOS="${XDG_VIDEOS_DIR:-$HOME/Videos}"

usage() {
  cat <<'EOF'
Usage:
  capture-neon-rain.sh screenshot [OUTPUT.png]
  capture-neon-rain.sh video [OUTPUT.mkv]

The helper captures the current desktop/session. Put Neon Rain in the desired
state first, hide overlays unless they are part of the demonstration, then run
this command from another terminal or bind it to a shortcut.

Video capture stops with Ctrl+C.
EOF
}

case "$MODE" in
  screenshot)
    OUTPUT="${OUTPUT:-$PICTURES/neon-rain-$STAMP.png}"
    mkdir -p "$(dirname "$OUTPUT")"

    if command -v spectacle >/dev/null 2>&1; then
      spectacle -b -n -o "$OUTPUT"
    elif command -v grim >/dev/null 2>&1; then
      grim "$OUTPUT"
    elif command -v gnome-screenshot >/dev/null 2>&1; then
      gnome-screenshot -f "$OUTPUT"
    elif command -v import >/dev/null 2>&1; then
      import -window root "$OUTPUT"
    else
      echo "No supported screenshot tool found." >&2
      echo "Install Spectacle, grim, gnome-screenshot, or ImageMagick." >&2
      exit 1
    fi

    echo "Saved screenshot: $OUTPUT"
    ;;

  video)
    OUTPUT="${OUTPUT:-$VIDEOS/neon-rain-$STAMP.mkv}"
    mkdir -p "$(dirname "$OUTPUT")"

    if [[ -n "${WAYLAND_DISPLAY:-}" ]] && command -v wf-recorder >/dev/null 2>&1; then
      echo "Recording Wayland session to $OUTPUT"
      echo "Press Ctrl+C to stop."
      wf-recorder -f "$OUTPUT"
    elif [[ -n "${DISPLAY:-}" ]] && command -v ffmpeg >/dev/null 2>&1; then
      SIZE=""
      if command -v xrandr >/dev/null 2>&1; then
        SIZE="$(xrandr | awk '/\*/ { print $1; exit }' || true)"
      fi
      SIZE="${SIZE:-1920x1080}"
      echo "Recording X11 display ${DISPLAY}.0 at $SIZE to $OUTPUT"
      echo "Press Ctrl+C to stop."
      ffmpeg \
        -f x11grab \
        -video_size "$SIZE" \
        -framerate 60 \
        -i "${DISPLAY}.0" \
        -c:v libx264 \
        -preset veryfast \
        -crf 18 \
        "$OUTPUT"
    else
      echo "No supported video recorder found for this session." >&2
      echo "Wayland: install wf-recorder. X11: install ffmpeg." >&2
      exit 1
    fi
    ;;

  help|-h|--help)
    usage
    ;;

  *)
    echo "Unknown capture mode: $MODE" >&2
    usage >&2
    exit 2
    ;;
esac
