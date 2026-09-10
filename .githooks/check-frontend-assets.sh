#!/usr/bin/env bash
# Install locked developer tooling when stale, then check source assets and types.

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

if ! command -v npm >/dev/null 2>&1; then
  echo "npm is not installed; required for HTML/CSS/JavaScript/TypeScript asset checks." >&2
  exit 1
fi

node -e 'const [major, minor] = process.versions.node.split(".").map(Number); if (major < 24 ||
  (major === 24 && minor < 12)) { console.error("Frontend checks require Node.js 24.12 or newer"); process.exit(1); }'

if [[ ! -x node_modules/.bin/prettier || ! -x node_modules/.bin/tsc ||
      package-lock.json -nt node_modules/.package-lock.json ||
      package.json -nt node_modules/.package-lock.json ]]; then
  echo "Installing pinned frontend tooling with npm ci"
  npm ci --ignore-scripts --include=dev --include=optional --no-audit --no-fund
fi

# Online freshness is deliberately a hook/CI policy, never part of Cargo's deterministic asset build.
npm run check:frontend-versions
npm audit --audit-level=high

if [[ "${MINI_FILM_FORMAT_STAGED:-0}" == "1" ]]; then
  npm run format:staged
fi

npm run check:assets
