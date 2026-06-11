#!/usr/bin/env bash
set -euo pipefail

if [[ "$(uname -s)" != "Linux" ]]; then
  exit 0
fi

if pkg-config --exists libsoup-3.0 javascriptcoregtk-4.1 webkit2gtk-4.1; then
  echo "Linux GUI build dependencies are already available"
  exit 0
fi

if ! command -v apt-get >/dev/null 2>&1; then
  echo "apt-get is required to install Linux GUI build dependencies on this runner." >&2
  exit 1
fi

apt_prefix=()
if [[ "$(id -u)" -ne 0 ]]; then
  apt_prefix=(sudo)
fi

"${apt_prefix[@]}" apt-get update
"${apt_prefix[@]}" apt-get install -y \
  libwebkit2gtk-4.1-dev \
  libjavascriptcoregtk-4.1-dev \
  libsoup-3.0-dev \
  libgtk-3-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  patchelf
