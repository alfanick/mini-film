#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  exit 0
fi

if ! command -v apt-get >/dev/null 2>&1; then
  echo "apt-get is required to install Linux image dependencies on this runner." >&2
  exit 1
fi

apt_prefix=()
if [[ "$(id -u)" -ne 0 ]]; then
  apt_prefix=(sudo)
fi

"${apt_prefix[@]}" apt-get update
"${apt_prefix[@]}" apt-get install -y \
  imagemagick \
  libimage-exiftool-perl

install_rawtherapee_stub() {
  local bin_dir="${RUNNER_TEMP:-/tmp}/mini-film-ci-bin"
  mkdir -p "$bin_dir"
  cat > "$bin_dir/rawtherapee-cli" <<'STUB'
#!/usr/bin/env bash
echo "rawtherapee-cli is not installed on this CI runner; this stub is for dependency checks only." >&2
exit 127
STUB
  chmod +x "$bin_dir/rawtherapee-cli"

  if [[ -n "${GITHUB_PATH:-}" ]]; then
    echo "$bin_dir" >> "$GITHUB_PATH"
  fi
}

if command -v rawtherapee-cli >/dev/null 2>&1; then
  exit 0
fi

if apt-cache show rawtherapee >/dev/null 2>&1; then
  if ! "${apt_prefix[@]}" apt-get install -y rawtherapee; then
    echo "::warning::rawtherapee package install failed; using CI-only dependency-check stub."
    install_rawtherapee_stub
  fi
else
  echo "::warning::rawtherapee package is unavailable on this runner; using CI-only dependency-check stub."
  install_rawtherapee_stub
fi
