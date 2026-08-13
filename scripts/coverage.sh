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
  --no-default-features \
  --tests \
  --lcov \
  --output-path target/coverage/lcov.info

echo "Generating coverage summary"
cargo llvm-cov report \
  --json \
  --summary-only \
  --output-path target/coverage/coverage-summary.json

cat <<'EOF'
Coverage artifacts:
  - target/coverage/lcov.info
  - target/coverage/coverage-summary.json
EOF
