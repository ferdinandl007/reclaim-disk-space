#!/bin/sh
set -eu

PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
CLEANER="$PROJECT_ROOT/skills/reclaim-disk-space/scripts/run-disk-clean.sh"
WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/reclaim-disk-space-cleaner.XXXXXX")
trap 'rm -rf "$WORK_DIR"' EXIT HUP INT TERM

mkdir -p "$WORK_DIR/normal/branch/nested" "$WORK_DIR/external"
printf '%s\n' safe > "$WORK_DIR/normal/branch/nested/file.txt"
printf '%s\n' protected > "$WORK_DIR/external/secret.txt"
"$CLEANER" --root "$WORK_DIR/normal" --execute --confirm "$WORK_DIR/normal" --workers 1 --profile max-throughput > "$WORK_DIR/normal-report.tsv" 2> "$WORK_DIR/normal-errors.log"
test ! -e "$WORK_DIR/normal"
test -f "$WORK_DIR/external/secret.txt"
grep -q '^SUMMARY.*profiling=false' "$WORK_DIR/normal-report.tsv"

mkdir -p "$WORK_DIR/profile"
printf '%s\n' profile > "$WORK_DIR/profile/file.txt"
DISK_CLEAN_PROFILE=1 "$CLEANER" --root "$WORK_DIR/profile" --execute --confirm "$WORK_DIR/profile" --workers 1 --profile max-throughput > "$WORK_DIR/profile-report.tsv" 2> "$WORK_DIR/profile-errors.log"
grep -q '^SUMMARY.*profiling=true' "$WORK_DIR/profile-report.tsv"

mkdir -p "$WORK_DIR/race/raced" "$WORK_DIR/race-external"
printf '%s\n' protected > "$WORK_DIR/race-external/secret.txt"
i=0
while [ "$i" -lt 4000 ]; do
    printf '%s\n' payload > "$WORK_DIR/race/raced/file-$i.txt"
    i=$((i + 1))
done

( while :; do
    if [ -d "$WORK_DIR/race/raced" ] && [ ! -L "$WORK_DIR/race/raced" ]; then
        mv "$WORK_DIR/race/raced" "$WORK_DIR/race/raced-original" 2>/dev/null || true
        ln -s "$WORK_DIR/race-external" "$WORK_DIR/race/raced" 2>/dev/null || true
    fi
    sleep 0.001
done ) &
RACER_PID=$!
set +e
"$CLEANER" --root "$WORK_DIR/race" --execute --confirm "$WORK_DIR/race" --workers 1 --profile max-throughput --keep-root > "$WORK_DIR/race-report.tsv" 2> "$WORK_DIR/race-errors.log"
set -e
kill "$RACER_PID" 2>/dev/null || true
wait "$RACER_PID" 2>/dev/null || true
test -f "$WORK_DIR/race-external/secret.txt"

echo "Cleaner race-safety checks passed"
