#!/usr/bin/env bash

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

echo "🔎 Checking cargo toolchain versions"
"$(dirname "$0")/check-cargo-versions.sh"

if ! command -v cargo-outdated >/dev/null 2>&1; then
  echo "cargo-outdated is not installed. Install with: cargo install cargo-outdated" >&2
  exit 1
fi

echo "🧾 Running cargo outdated for direct dependencies"
cargo outdated --workspace --root-deps-only --exit-code 1

echo "🔎 Checking HTML/CSS/JS assets"
"$(dirname "$0")/check-frontend-assets.sh"

echo "🔎 Running format check (cargo fmt --all -- --check)"
cargo fmt --all -- --check

echo "🧪 Running CLI clippy check (cargo clippy --all-targets --no-default-features -- -D warnings)"
cargo clippy --all-targets --no-default-features -- -D warnings

if [[ "$(uname -s)" != "Linux" ]] || pkg-config --exists libsoup-3.0 javascriptcoregtk-4.1 webkit2gtk-4.1; then
  echo "🧪 Running GUI clippy check (cargo clippy --all-targets -- -D warnings)"
  cargo clippy --all-targets -- -D warnings
else
  echo "⚠️  Skipping GUI clippy check; install WebKitGTK 4.1 development packages to enable it."
fi
