#!/bin/sh
set -eu

PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
SCANNER="$PROJECT_ROOT/skills/reclaim-disk-space/scripts/run-disk-scout.sh"
FIXTURE=$(mktemp -d "${TMPDIR:-/tmp}/reclaim-disk-space-scout.XXXXXX")
REPORT="$FIXTURE/report.tsv"
trap 'rm -rf "$FIXTURE"' EXIT HUP INT TERM

mkdir -p "$FIXTURE/photos" "$FIXTURE/project/.git" "$FIXTURE/project/.venv" "$FIXTURE/project/src" "$FIXTURE/project/build"
truncate -s 120000 "$FIXTURE/photos/photo.jpg"
truncate -s 118000 "$FIXTURE/photos/photo v2.jpg"
truncate -s 117000 "$FIXTURE/photos/photo (3).jpg"
printf '%s\n' '[project]' > "$FIXTURE/project/pyproject.toml"
printf '%s\n' 'print(1)' > "$FIXTURE/project/src/main.py"
printf '%s\n' 'home = /usr/bin/python3' > "$FIXTURE/project/.venv/pyvenv.cfg"
truncate -s 100000 "$FIXTURE/project/build/generated.bin"
touch -t 202001010000 \
    "$FIXTURE/project/pyproject.toml" \
    "$FIXTURE/project/src/main.py" \
    "$FIXTURE/project/.venv/pyvenv.cfg"

"$SCANNER" "$FIXTURE" 2 > "$REPORT"
grep -q '^SUMMARY.*timestamp_queries=' "$REPORT"
grep -q '^ENVIRONMENT.*kind=python_venv' "$REPORT"
grep -q '^PROJECT.*source_files=.*generated_files=' "$REPORT"
grep -q '^PROJECT.*stale_review=true' "$REPORT"
grep -q '^VERSION_CLUSTER.*evidence_quality=' "$REPORT"

echo "Scanner fixture checks passed"
