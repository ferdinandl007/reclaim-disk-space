#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
BINARY="$SCRIPT_DIR/disk-clean"

if [ ! -x "$BINARY" ] || \
   [ "$SCRIPT_DIR/disk-clean.rs" -nt "$BINARY" ] || \
   [ "$SCRIPT_DIR/macos_bulk_attrs.c" -nt "$BINARY" ]; then
  "$SCRIPT_DIR/build-disk-clean.sh" >/dev/null
fi

exec "$BINARY" "$@"
