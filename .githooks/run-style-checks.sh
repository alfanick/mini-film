#!/usr/bin/env bash

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

echo "🔎 Running format check (cargo fmt --all -- --check)"
cargo fmt --all -- --check

echo "🧪 Running clippy check (cargo clippy --all-targets -- -D warnings)"
cargo clippy --all-targets -- -D warnings
