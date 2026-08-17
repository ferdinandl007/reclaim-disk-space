#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
BUILD_DIR=${TMPDIR:-/tmp}/reclaim-disk-space-build
OUTPUT=${1:-"$SCRIPT_DIR/disk-scout"}
OBJECT="$BUILD_DIR/macos_bulk_attrs.o"

mkdir -p "$BUILD_DIR"
if [ ! -f "$OBJECT" ] || [ "$SCRIPT_DIR/macos_bulk_attrs.c" -nt "$OBJECT" ]; then
  clang -O3 -DNDEBUG -Wall -Wextra -Werror \
    -c "$SCRIPT_DIR/macos_bulk_attrs.c" \
    -o "$OBJECT"
fi
rustc --edition=2021 -C opt-level=3 -C target-cpu=native \
  -C link-arg="$OBJECT" \
  "$SCRIPT_DIR/disk-scout.rs" \
  -o "$OUTPUT"
chmod 755 "$OUTPUT"
printf '%s\n' "$OUTPUT"
