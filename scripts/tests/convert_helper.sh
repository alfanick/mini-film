#!/usr/bin/env bash
LOG_FILE="__LOG_FILE__"
EXIT_CODE="__EXIT_CODE__"

echo "$@" >> "$LOG_FILE"

if [ "$#" -lt 2 ]; then
  exit "$EXIT_CODE"
fi

LAST=""
while [ "$#" -gt 0 ]; do
  LAST="$1"
  shift
done

if [ -n "$LAST" ]; then
  touch "$LAST"
fi

exit "$EXIT_CODE"

