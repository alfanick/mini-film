#!/usr/bin/env bash

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

RAW_FILE="${MINI_FILM_SANITY_RAW:-/home/alfanick/Pictures/mini-film-sample.dng}"
PROFILES_ROOT="${MINI_FILM_SANITY_PROFILES_ROOT:-/home/alfanick/Pictures/profile-library}"
PROFILE="${MINI_FILM_SANITY_PROFILE:-$PROFILES_ROOT/emulations/Agfa Scala 200.xmp}"
BINARY="${MINI_FILM_BINARY:-target/release/mini-film}"
WORKDIR="${MINI_FILM_SANITY_WORKDIR:-${RUNNER_TEMP:-/tmp}/mini-film-ci-sanity}"
JPG_QUALITY="${MINI_FILM_SANITY_JPG_QUALITY:-70}"
LONG_EDGE="${MINI_FILM_SANITY_LONG_EDGE:-2160}"

if [ ! -f "$RAW_FILE" ]; then
  echo "Sample RAW not found: $RAW_FILE" >&2
  exit 1
fi

if [ ! -f "$PROFILE" ]; then
  echo "Profile file not found: $PROFILE" >&2
  exit 1
fi

if [ ! -f "$PROFILES_ROOT/emulations" ] && [ ! -d "$PROFILES_ROOT/emulations" ]; then
  echo "Profiles root does not contain emulations/ directory: $PROFILES_ROOT" >&2
  exit 1
fi

if [ ! -x "$BINARY" ]; then
  echo "mini-film binary not executable at $BINARY" >&2
  exit 1
fi

rm -rf "$WORKDIR"
mkdir -p "$WORKDIR"
mkdir -p "$WORKDIR/batch-input" "$WORKDIR/batch-output"
cp "$RAW_FILE" "$WORKDIR/batch-input/$(basename "$RAW_FILE")"

INFO_LOG="$WORKDIR/info.txt"
APPLY_OUTPUT="$WORKDIR/mini-film-sanity.jpg"
BATCH_OUTPUT="$WORKDIR/batch-output"
RAW_STEM="$(basename "$RAW_FILE")"
RAW_NAME="${RAW_STEM%.*}"

set -x

"$BINARY" info "$PROFILE" \
  --profiles-root "$PROFILES_ROOT" \
  > "$INFO_LOG"

grep -q "Kind: emulation preset" "$INFO_LOG"

test -s "$INFO_LOG"

echo "Info command passed"

"$BINARY" apply "$RAW_FILE" \
  --output "$APPLY_OUTPUT" \
  --profile "$PROFILE" \
  --profiles-root "$PROFILES_ROOT" \
  --jpg-quality "$JPG_QUALITY" \
  --long-edge "$LONG_EDGE"

test -s "$APPLY_OUTPUT"

echo "Apply command passed"

"$BINARY" batch "$WORKDIR/batch-input" "$BATCH_OUTPUT" \
  --profile "$PROFILE" \
  --profiles-root "$PROFILES_ROOT" \
  --jobs 1 \
  --long-edge "$LONG_EDGE" \
  --jpg-quality "$JPG_QUALITY"

if [ ! -f "$BATCH_OUTPUT/$RAW_NAME.jpg" ]; then
  echo "Batch output was not generated at expected path" >&2
  exit 1
fi

echo "Batch command passed"

echo "Sanity check completed successfully"
