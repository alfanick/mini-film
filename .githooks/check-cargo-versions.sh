#!/usr/bin/env bash
set -euo pipefail

if ! command -v cargo >/dev/null 2>&1; then
  echo "❌ cargo not found in PATH"
  exit 1
fi

if ! command -v rustc >/dev/null 2>&1; then
  echo "❌ rustc not found in PATH"
  exit 1
fi

printf 'cargo:  %s\n' "$(cargo --version)"
printf 'rustc:  %s\n' "$(rustc --version)"

if command -v rustup >/dev/null 2>&1; then
  printf 'rustup: %s\n' "$(rustup --version | head -n 1)"
  printf 'active toolchain: %s\n' "$(rustup show active-toolchain 2>/dev/null | sed -n '1p')"
fi

if command -v rustfmt >/dev/null 2>&1; then
  printf 'rustfmt:%s\n' "$(rustfmt --version)"
fi

if command -v cargo-fmt >/dev/null 2>&1; then
  printf 'cargo-fmt helper: %s\n' "$(cargo fmt --version)"
fi

if cargo clippy --version >/dev/null 2>&1; then
  printf 'clippy: %s\n' "$(cargo clippy --version)"
fi
