#!/usr/bin/env bash
set -euo pipefail
bin_dir="${XDG_BIN_HOME:-$HOME/.local/bin}"
apps_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
rm -f "$bin_dir/neon-rain" "$apps_dir/neon-rain.desktop"
echo "Removed the user-installed Neon Rain binary and desktop entry."
echo "Configuration and learned analysis caches were preserved."
