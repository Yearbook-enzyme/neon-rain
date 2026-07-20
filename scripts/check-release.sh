#!/usr/bin/env bash
set -Eeuo pipefail

ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo "== Shell syntax =="
for script in scripts/*.sh; do
  bash -n "$script"
done

echo "== Rust formatting =="
nix develop --command cargo fmt --check

echo "== Rust tests =="
nix develop --command cargo test --all-targets --locked

echo "== Clippy =="
nix develop --command cargo clippy --all-targets --locked

echo "== Optimized release build =="
nix develop --command cargo build --release --locked

echo "== CLI and XDG smoke tests =="
SMOKE_ROOT="$(mktemp -d)"
trap 'rm -rf "$SMOKE_ROOT"' EXIT
mkdir -p "$SMOKE_ROOT/config" "$SMOKE_ROOT/state" "$SMOKE_ROOT/home"

run_cli() {
  nix develop --command env \
    HOME="$SMOKE_ROOT/home" \
    XDG_CONFIG_HOME="$SMOKE_ROOT/config" \
    XDG_STATE_HOME="$SMOKE_ROOT/state" \
    target/release/neon-rain "$@"
}

run_cli --version
run_cli --list-scenes | tee "$SMOKE_ROOT/scenes.txt"
grep -Fq "lucid-dream" "$SMOKE_ROOT/scenes.txt"
grep -Fq "cyber-tunnel" "$SMOKE_ROOT/scenes.txt"
grep -Fq "aurora-drift" "$SMOKE_ROOT/scenes.txt"

run_cli --list-palettes | tee "$SMOKE_ROOT/palettes.txt"
grep -Fq "vaporwave" "$SMOKE_ROOT/palettes.txt"
grep -Fq "rainbow" "$SMOKE_ROOT/palettes.txt"

run_cli --write-default-config
CONFIG="$SMOKE_ROOT/config/neon-rain/config.conf"
test -f "$CONFIG"
grep -Fq "scene = classic-matrix" "$CONFIG"
grep -Fq "# palette = theme" "$CONFIG"

sed -i 's/scene = classic-matrix/scene = lucid-dream/' "$CONFIG"
run_cli --print-config | tee "$SMOKE_ROOT/effective.txt"
grep -Fq "scene = lucid-dream" "$SMOKE_ROOT/effective.txt"
grep -Fq "theme = dream" "$SMOKE_ROOT/effective.txt"
grep -Fq "palette = vaporwave" "$SMOKE_ROOT/effective.txt"
grep -Fq "field_of_view = 66" "$SMOKE_ROOT/effective.txt"
grep -Fq "auto_flight = weave" "$SMOKE_ROOT/effective.txt"

run_cli --reset-session

echo "== Nix flake =="
nix flake check -L

echo
echo "RELEASE CHECKS PASSED"
