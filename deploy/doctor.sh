#!/usr/bin/env bash
set -u

ok=0
warn=0
check_required() {
  local command_name="$1"
  local purpose="$2"
  if command -v "$command_name" >/dev/null 2>&1; then
    printf '  [ready]   %-30s %s\n' "$command_name" "$purpose"
  else
    printf '  [missing] %-30s %s\n' "$command_name" "$purpose"
    warn=$((warn + 1))
  fi
}
check_optional() {
  local command_name="$1"
  local purpose="$2"
  if command -v "$command_name" >/dev/null 2>&1; then
    printf '  [ready]   %-30s %s\n' "$command_name" "$purpose"
  else
    printf '  [optional]%-30s %s\n' " $command_name" "$purpose"
  fi
}

echo "Neon Rain deployment doctor"
echo
check_required pw-record "PipeWire system-audio capture"
check_optional playerctl "generic MPRIS player metadata"
check_optional neon-rain-moodbar-profile "external full-track timeline enrichment"
check_optional neon-rain-lyric-runtime "lyric semantic enrichment"
check_optional neon-rain-track-profile "hand-authored/inferred track profiles"

echo
cache_dir="${XDG_CACHE_HOME:-$HOME/.cache}/neon-rain/analysis"
config_dir="${XDG_CONFIG_HOME:-$HOME/.config}/neon-rain"
printf '  [path]    %-30s %s\n' "analysis cache" "$cache_dir"
printf '  [path]    %-30s %s\n' "configuration" "$config_dir"

if [[ $warn -gt 0 ]]; then
  echo
  echo "Core Matrix rendering still works, but live audio capture needs pw-record."
  exit 1
fi

echo
echo "Core deployment capabilities look ready."
