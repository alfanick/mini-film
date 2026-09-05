#!/usr/bin/env bash
# Install locked developer tooling when stale, then check source assets and types.

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

if ! command -v npm >/dev/null 2>&1; then
  echo "npm is not installed; required for HTML/CSS/JavaScript/TypeScript asset checks." >&2
  exit 1
fi

if [[ ! -x node_modules/.bin/prettier || ! -x node_modules/.bin/tsc ||
      package-lock.json -nt node_modules/.package-lock.json ||
      package.json -nt node_modules/.package-lock.json ]]; then
  echo "Installing pinned frontend tooling with npm ci"
  npm ci --ignore-scripts --include=dev --include=optional --no-audit --no-fund
fi

if [[ "${MINI_FILM_FORMAT_STAGED:-0}" == "1" ]]; then
  npm run format:staged
fi

npm run check:assets
