#!/usr/bin/env bash
set -Eeuo pipefail

cd "$(dirname -- "${BASH_SOURCE[0]}")/.."

cargo fmt --check
cargo check --locked
cargo test settings::tests --locked
