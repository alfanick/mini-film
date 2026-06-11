#!/usr/bin/env bash
set -euo pipefail

if command -v cargo-outdated >/dev/null 2>&1; then
  exit 0
fi

echo "cargo-outdated is missing; installing it for this CI run"
cargo install --locked cargo-outdated
