#!/bin/sh
set -eu

PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
SCANNER="$PROJECT_ROOT/skills/reclaim-disk-space/scripts/run-disk-scout.sh"
ROOT=${1:?usage: benchmark-scan.sh ROOT [WORKER_LIST]}
WORKERS=${2:-"1 2 4 8 16"}
WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/reclaim-disk-space-benchmark.XXXXXX")
trap 'rm -rf "$WORK_DIR"' EXIT HUP INT TERM

printf 'BENCHMARK\troot=%s\n' "$ROOT"
printf 'workers\telapsed_seconds\tmetadata_entries\tpeak_rss_bytes\tuser_seconds\tsys_seconds\tinstructions\tcycles\tpermission_errors\tpartial_directories\n'
for worker in $WORKERS; do
    report="$WORK_DIR/report-$worker.tsv"
    timing="$WORK_DIR/timing-$worker.txt"
    /usr/bin/time -l "$SCANNER" "$ROOT" "$worker" > "$report" 2> "$timing"
    summary=$(awk -F '\t' '/^SUMMARY/{print; exit}' "$report")
    field() { printf '%s\n' "$summary" | awk -F '\t' -v key="$1" '{for (i = 1; i <= NF; i++) if ($i ~ "^" key "=") { sub("^" key "=", "", $i); print $i; exit }}'; }
    rss=$(awk '/maximum resident set size/ { print $1; exit }' "$timing")
    user_seconds=$(awk '/real/ { for (i = 1; i <= NF; i++) if ($i == "user") { print $(i - 1); exit } }' "$timing")
    sys_seconds=$(awk '/real/ { for (i = 1; i <= NF; i++) if ($i == "sys") { print $(i - 1); exit } }' "$timing")
    instructions=$(awk '/instructions retired/ { print $1; exit }' "$timing")
    cycles=$(awk '/cycles elapsed/ { print $1; exit }' "$timing")
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$worker" "$(field elapsed_seconds)" "$(field metadata_entries)" "${rss:-unknown}" "${user_seconds:-unknown}" "${sys_seconds:-unknown}" "${instructions:-unknown}" "${cycles:-unknown}" "$(field permission_errors)" "$(field partial_directories)"
done
