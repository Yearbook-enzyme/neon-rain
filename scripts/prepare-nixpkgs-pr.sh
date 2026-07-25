#!/usr/bin/env bash
set -Eeuo pipefail

NEON_ROOT="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
NIXPKGS_ROOT="${1:-$HOME/Projects/nixpkgs}"
BRANCH="neon-rain-init"
PACKAGE_PATH="pkgs/by-name/ne/neon-rain/package.nix"
MAINTAINERS_PATH="maintainers/maintainer-list.nix"
TEMPLATE="$NEON_ROOT/packaging/nixpkgs/package.nix.in"

fail() {
  echo "error: $*" >&2
  exit 1
}

[[ -d "$NIXPKGS_ROOT/.git" ]] || fail "not a nixpkgs checkout: $NIXPKGS_ROOT"
[[ -f "$NIXPKGS_ROOT/$MAINTAINERS_PATH" ]] || fail "maintainer list not found"
[[ -f "$TEMPLATE" ]] || fail "package template not found"

cd "$NIXPKGS_ROOT"
git diff --quiet && git diff --cached --quiet || fail "nixpkgs working tree is not clean"

if ! git remote get-url upstream >/dev/null 2>&1; then
  git remote add upstream https://github.com/NixOS/nixpkgs.git
fi

git fetch upstream master
git switch master
git merge --ff-only upstream/master

if git show-ref --verify --quiet "refs/heads/$BRANCH"; then
  git branch -D "$BRANCH"
fi
git switch -c "$BRANCH"

mkdir -p "$(dirname "$PACKAGE_PATH")"
cp "$TEMPLATE" "$PACKAGE_PATH"

python3 - "$MAINTAINERS_PATH" <<'PY'
from pathlib import Path
import re
import sys

path = Path(sys.argv[1])
text = path.read_text()
handle = "yearbook-enzyme"
if re.search(rf"^  {re.escape(handle)} = \{{", text, flags=re.M):
    raise SystemExit(0)

entry = '''  yearbook-enzyme = {
    name = "Logan Campbell";
    github = "Yearbook-enzyme";
    githubId = 144038028;
  };
'''

matches = list(re.finditer(r"^  ([A-Za-z0-9_'\-]+) = \{", text, flags=re.M))
for match in matches:
    if match.group(1).lower() > handle:
        text = text[: match.start()] + entry + text[match.start() :]
        break
else:
    closing = text.rfind("}")
    if closing < 0:
        raise SystemExit("could not find end of maintainer list")
    text = text[:closing] + entry + text[closing:]

path.write_text(text)
PY

source_json="$(
  nix run nixpkgs#nix-prefetch-github -- \
    Yearbook-enzyme neon-rain --rev v0.1.0-alpha.3
)"
source_hash="$(printf '%s' "$source_json" | python3 -c 'import json,sys; print(json.load(sys.stdin)["hash"])')"
[[ "$source_hash" == sha256-* ]] || fail "could not determine source hash"
sed -i "s|@SOURCE_HASH@|$source_hash|" "$PACKAGE_PATH"
sed -i 's|@CARGO_HASH@|sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=|' "$PACKAGE_PATH"

set +e
build_output="$(nix-build -A neon-rain 2>&1)"
build_status=$?
set -e

if [[ $build_status -eq 0 ]]; then
  fail "the fake Cargo hash unexpectedly built successfully"
fi

cargo_hash="$(
  printf '%s\n' "$build_output" |
    grep -Eo 'sha256-[A-Za-z0-9+/=]+' |
    tail -n 1
)"
[[ "$cargo_hash" == sha256-* ]] || {
  printf '%s\n' "$build_output" >&2
  fail "could not extract the expected Cargo hash"
}

sed -i "s|sha256-AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=|$cargo_hash|" "$PACKAGE_PATH"

nix-build -A neon-rain
nix-build lib/tests/maintainers.nix
nix-shell -p nixfmt-rfc-style --run "nixfmt $PACKAGE_PATH $MAINTAINERS_PATH"

# Rebuild once after formatting so the exact committed form is tested.
nix-build -A neon-rain

git add "$PACKAGE_PATH" "$MAINTAINERS_PATH"
git commit -m "neon-rain: init at 0.1.0-alpha.3"

echo
echo "nixpkgs package branch is ready."
echo "Push with: git push -u origin $BRANCH"
echo "Then open a PR to NixOS/nixpkgs master."
