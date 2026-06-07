#!/usr/bin/env bash
set -euo pipefail

: "${GITHUB_WORKSPACE:=}"
: "${TMPDIR:=/tmp}"

log_space() {
  local label="$1"
  echo "== ${label} =="
  if [[ -n "$GITHUB_WORKSPACE" && -d "$GITHUB_WORKSPACE" ]]; then
    du -sh "$GITHUB_WORKSPACE" 2>/dev/null || true
  fi
  du -sh "$TMPDIR" 2>/dev/null || true
  du -sh "$HOME/.cargo" 2>/dev/null || true
}

log_space "Before cleanup"

if command -v cargo >/dev/null 2>&1 && [[ -n "$GITHUB_WORKSPACE" && -d "$GITHUB_WORKSPACE" ]]; then
  cd "$GITHUB_WORKSPACE"
  if [[ -f Cargo.toml ]]; then
    cargo clean --workspace --quiet || true
  fi
fi

if [[ -n "$GITHUB_WORKSPACE" && -d "$GITHUB_WORKSPACE" ]]; then
  rm -rf \
    "$GITHUB_WORKSPACE/target" \
    "$GITHUB_WORKSPACE/artifacts" \
    "$GITHUB_WORKSPACE/.rustc_info.json" \
    2>/dev/null || true

  find "$GITHUB_WORKSPACE" -xdev -type d -name target -print0 | xargs -0 rm -rf 2>/dev/null || true
fi

if [[ -d "$TMPDIR" ]]; then
  find "$TMPDIR" -maxdepth 1 -type d -name "mini-film*" -print0 | xargs -0 rm -rf 2>/dev/null || true
fi

if [[ -d "$HOME/.cache" ]]; then
  find "$HOME/.cache" -maxdepth 1 -type d -name 'mini-film*' -print0 | xargs -0 rm -rf 2>/dev/null || true
fi

log_space "After cleanup"
