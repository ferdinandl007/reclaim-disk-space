#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
BINARY="$SCRIPT_DIR/disk-scout"

if [ ! -x "$BINARY" ] || \
   [ "$SCRIPT_DIR/disk-scout.rs" -nt "$BINARY" ] || \
   [ "$SCRIPT_DIR/macos_bulk_attrs.c" -nt "$BINARY" ]; then
  "$SCRIPT_DIR/build-disk-scout.sh" "$BINARY" >/dev/null
fi

exec "$BINARY" "$@"
