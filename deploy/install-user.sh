#!/usr/bin/env bash
set -euo pipefail

project_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="$project_dir/target/release/neon-rain"
bin_dir="${XDG_BIN_HOME:-$HOME/.local/bin}"
apps_dir="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/neon-rain"
cache_dir="${XDG_CACHE_HOME:-$HOME/.cache}/neon-rain/analysis"

if [[ ! -x "$binary" ]]; then
  echo "Release binary not found; building it first..."
  (cd "$project_dir" && cargo build --release)
fi

mkdir -p "$bin_dir" "$apps_dir" "$config_dir" "$cache_dir"
install -m755 "$binary" "$bin_dir/neon-rain"
sed "s|__BINDIR__|$bin_dir|g" "$project_dir/deploy/neon-rain.desktop.in" \
  > "$apps_dir/neon-rain.desktop"
chmod 644 "$apps_dir/neon-rain.desktop"

if command -v update-desktop-database >/dev/null 2>&1; then
  update-desktop-database "$apps_dir" >/dev/null 2>&1 || true
fi

echo "Installed Neon Rain to $bin_dir/neon-rain"
echo "Desktop entry: $apps_dir/neon-rain.desktop"
echo "Run deploy/doctor.sh to inspect optional capabilities."
