#!/bin/sh
set -eu

PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
OUTPUT_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/reclaim-disk-space-release.XXXXXX")
trap 'rm -rf "$OUTPUT_ROOT"' EXIT HUP INT TERM

for version in '../escape' '/tmp/absolute' 'has space' 'shell;payload' '..'; do
    if "$PROJECT_ROOT/scripts/build-release.sh" "$OUTPUT_ROOT" "$version" >/dev/null 2>&1; then
        echo "Accepted unsafe release version: $version" >&2
        exit 1
    fi
done

echo "Release safety checks passed"
