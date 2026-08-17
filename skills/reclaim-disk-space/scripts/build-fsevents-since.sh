#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
OUTPUT=${1:-"$SCRIPT_DIR/fsevents-since"}

clang -O3 -DNDEBUG -Wall -Wextra -Werror \
  "$SCRIPT_DIR/fsevents-since.c" \
  -framework CoreServices \
  -o "$OUTPUT"
chmod 755 "$OUTPUT"
printf '%s\n' "$OUTPUT"
