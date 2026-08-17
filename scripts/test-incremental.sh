#!/bin/sh
set -eu

PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
INCREMENTAL="$PROJECT_ROOT/skills/reclaim-disk-space/scripts/incremental-disk-scout.sh"
WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/reclaim-disk-space-incremental.XXXXXX")
trap 'rm -rf "$WORK_DIR"' EXIT HUP INT TERM

mkdir -p "$WORK_DIR/root" "$WORK_DIR/cache"
printf '%s\n' fixture > "$WORK_DIR/root/file.txt"
"$INCREMENTAL" "$WORK_DIR/root" "$WORK_DIR/cache" > "$WORK_DIR/first.tsv"
grep -q '^CACHE_STATUS\tfull_scan_created' "$WORK_DIR/first.tsv"
test -s "$WORK_DIR/cache/index.bin"
"$PROJECT_ROOT/skills/reclaim-disk-space/scripts/run-disk-scout.sh" query "$WORK_DIR/cache/index.bin" summary > "$WORK_DIR/artifact.tsv"
grep -q '^ARTIFACT_SUMMARY.*overlap=false' "$WORK_DIR/artifact.tsv"
"$INCREMENTAL" "$WORK_DIR/root" "$WORK_DIR/cache" > "$WORK_DIR/second.tsv"
grep -q '^CACHE_STATUS\treused' "$WORK_DIR/second.tsv"

printf '%s\n' stale > "$WORK_DIR/cache/scanner-fingerprint"
"$INCREMENTAL" "$WORK_DIR/root" "$WORK_DIR/cache" > "$WORK_DIR/fingerprint.tsv"
grep -q '^CACHE_STATUS\tfull_scan_created\treason=scanner_changed' "$WORK_DIR/fingerprint.tsv"

mkdir "$WORK_DIR/cache/.lock"
set +e
"$INCREMENTAL" "$WORK_DIR/root" "$WORK_DIR/cache" > "$WORK_DIR/busy.tsv"
busy_rc=$?
set -e
[ "$busy_rc" -eq 5 ]
grep -q '^CACHE_STATUS\tbusy' "$WORK_DIR/busy.tsv"

echo "Incremental cache checks passed"
