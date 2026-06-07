#!/usr/bin/env bash

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
  echo "cargo-llvm-cov is not installed." >&2
  echo "Install it with: cargo install --locked cargo-llvm-cov" >&2
  exit 1
fi

mkdir -p target/coverage

echo "🧪 Running coverage (lcov)"
cargo llvm-cov \
  --all-targets \
  --tests \
  --lcov \
  --output-path target/coverage/lcov.info

echo "Coverage report written to target/coverage/lcov.info"
