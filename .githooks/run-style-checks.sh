#!/usr/bin/env bash

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

echo "🔎 Checking cargo toolchain versions"
"$(dirname "$0")/check-cargo-versions.sh"

echo "🔎 Checking HTML/CSS/JS assets"
"$(dirname "$0")/check-frontend-assets.sh"

echo "🔎 Running format check (cargo fmt --all -- --check)"
cargo fmt --all -- --check

echo "🧪 Running clippy check (cargo clippy --all-targets -- -D warnings)"
cargo clippy --all-targets -- -D warnings
