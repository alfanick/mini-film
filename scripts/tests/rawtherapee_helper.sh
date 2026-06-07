#!/usr/bin/env bash
LOG_FILE="__LOG_FILE__"
OUTPUT_IMAGE="__OUTPUT_IMAGE__"
CREATE_OUTPUT="__CREATE_OUTPUT__"
EXIT_CODE="__EXIT_CODE__"

echo "$@" >> "$LOG_FILE"

while [ "$#" -gt 0 ]; do
  if [ "$1" = "-o" ]; then
    shift
    OUTPUT="$1"
    break
  fi
  shift
done

if [ "$CREATE_OUTPUT" = "1" ] && [ -n "$OUTPUT" ]; then
  if [ -n "$OUTPUT_IMAGE" ]; then
    cp "$OUTPUT_IMAGE" "$OUTPUT"
  else
    touch "$OUTPUT"
  fi
fi

exit "$EXIT_CODE"

