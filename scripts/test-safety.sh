#!/bin/sh
set -eu

PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
CLEANER="$PROJECT_ROOT/skills/reclaim-disk-space/scripts/run-disk-clean.sh"
WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/reclaim-disk-space-safety.XXXXXX")
trap 'rm -rf "$WORK_DIR"' EXIT HUP INT TERM

expect_rejected() {
    label=$1
    shift
    output="$WORK_DIR/$label.log"
    if "$CLEANER" "$@" >"$output" 2>&1; then
        cat "$output" >&2
        echo "safety regression: $label was accepted" >&2
        exit 1
    fi
}

cd "$PROJECT_ROOT"
expect_rejected relative-root --root .
expect_rejected filesystem-root --root /
expect_rejected data-volume-root --root /System/Volumes/Data
expect_rejected users-root --root /Users

mkdir -p "$WORK_DIR/real"
ln -s "$WORK_DIR/real" "$WORK_DIR/link"
expect_rejected symlink-root --root "$WORK_DIR/link"

echo "Safety checks passed"
