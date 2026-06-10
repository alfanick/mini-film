#!/usr/bin/env bash

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

if ! command -v npm >/dev/null 2>&1; then
  echo "npm is not installed; required for HTML/CSS/JS asset checks." >&2
  exit 1
fi

if [[ ! -x node_modules/.bin/prettier || package-lock.json -nt node_modules/.package-lock.json || package.json -nt node_modules/.package-lock.json ]]; then
  echo "Installing pinned frontend tooling with npm ci"
  npm ci --ignore-scripts
fi

npm run check:assets
