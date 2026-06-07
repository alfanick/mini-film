#!/usr/bin/env bash

set -euo pipefail

cd "$(git rev-parse --show-toplevel)"

RAW_FILE="${MINI_FILM_SANITY_RAW:-/home/alfanick/Pictures/mini-film-sample.dng}"
PROFILES_ROOT="${MINI_FILM_SANITY_PROFILES_ROOT:-/home/alfanick/Pictures/RNI}"
PROFILE="${MINI_FILM_SANITY_PROFILE:-$PROFILES_ROOT/emulations/Agfa Scala 200.xmp}"
BINARY="${MINI_FILM_BINARY:-target/release/mini-film}"
WORKDIR="${MINI_FILM_SANITY_WORKDIR:-${RUNNER_TEMP:-/tmp}/mini-film-ci-sanity}"
JPG_QUALITY="${MINI_FILM_SANITY_JPG_QUALITY:-70}"
LONG_EDGE="${MINI_FILM_SANITY_LONG_EDGE:-2160}"
THUMB_EDGE="${MINI_FILM_SANITY_THUMB_EDGE:-2048}"
SAMPLER_PROFILE_LIST="${MINI_FILM_SANITY_SAMPLER_PROFILE_LIST:-$'Agfa Scala 200.xmp\nKodak Portra 400.xmp'}"

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
SAMPLER_OUTPUT="$WORKDIR/sampler.html"
RAW_STEM="$(basename "$RAW_FILE")"
RAW_NAME="${RAW_STEM%.*}"
SAMPLER_PROFILES_ROOT="$WORKDIR/sampler-profiles"

mkdir -p "$SAMPLER_PROFILES_ROOT/emulations" "$SAMPLER_PROFILES_ROOT/profiles"

while IFS= read -r sampler_profile; do
  [ -z "$sampler_profile" ] && continue

  source_profile="$sampler_profile"
  if [[ "$sampler_profile" != /* ]]; then
    source_profile="$PROFILES_ROOT/emulations/$sampler_profile"
  fi

  if [ ! -f "$source_profile" ]; then
    echo "Selected sampler emulation profile not found: $source_profile" >&2
    exit 1
  fi

  filename="$(basename "$source_profile")"
  cp "$source_profile" "$SAMPLER_PROFILES_ROOT/emulations/$filename"

  tmp_info="$(mktemp)"
  "$BINARY" info "$source_profile" --profiles-root "$PROFILES_ROOT" > "$tmp_info"
  linked_profile=$(sed -n 's/^Linked RGBTable profile: //p' "$tmp_info" | head -n 1)
  rm -f "$tmp_info"

  if [ -n "$linked_profile" ] && [ -f "$linked_profile" ]; then
    cp "$linked_profile" "$SAMPLER_PROFILES_ROOT/profiles/$(basename "$linked_profile")"
  fi
done <<< "$SAMPLER_PROFILE_LIST"

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

"$BINARY" sampler "$RAW_FILE" \
  --output "$SAMPLER_OUTPUT" \
  --profiles-root "$SAMPLER_PROFILES_ROOT" \
  --jpg-quality "$JPG_QUALITY" \
  --columns 4 \
  --thumbnail-long-edge "$THUMB_EDGE" \
  --progressive

test -s "$SAMPLER_OUTPUT"
grep -q "<html" "$SAMPLER_OUTPUT"

echo "Sampler command passed"

echo "Sanity check completed successfully"
