#!/bin/sh
set -eu

PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
CLEANER="$PROJECT_ROOT/skills/reclaim-disk-space/scripts/run-disk-clean.sh"
WORKERS=${1:-"1 2 4 8"}
FILES=${2:-8192}
WORK_DIR=$(mktemp -d "${TMPDIR:-/tmp}/reclaim-disk-space-clean-benchmark.XXXXXX")
trap 'rm -rf "$WORK_DIR"' EXIT HUP INT TERM

printf 'BENCHMARK\tcomponent=cleaner\tfiles=%s\n' "$FILES"
printf 'workers\telapsed_seconds\tnodes_deleted\tnodes_not_found\terrors\tpeak_rss_bytes\tuser_seconds\tsys_seconds\tinstructions\tcycles\n'

for worker in $WORKERS; do
    root="$WORK_DIR/root-$worker"
    report="$WORK_DIR/report-$worker.tsv"
    timing="$WORK_DIR/timing-$worker.txt"
    mkdir -p "$root"
    i=1
    while [ "$i" -le "$FILES" ]; do
        : > "$root/file-$i.txt"
        i=$((i + 1))
    done
    env DISK_CLEAN_PROFILE=1 /usr/bin/time -l "$CLEANER" --root "$root" --execute --confirm "$root" --workers "$worker" --profile max-throughput > "$report" 2> "$timing"
    summary=$(awk -F '\t' '/^SUMMARY/{print; exit}' "$report")
    field() { printf '%s\n' "$summary" | awk -F '\t' -v key="$1" '{for (i = 1; i <= NF; i++) if ($i ~ "^" key "=") { sub("^" key "=", "", $i); print $i; exit }}'; }
    elapsed=$(awk '/real/ {print $1; exit}' "$timing")
    rss=$(awk '/maximum resident set size/ {print $1; exit}' "$timing")
    user_seconds=$(awk '/real/ { for (i = 1; i <= NF; i++) if ($i == "user") { print $(i - 1); exit } }' "$timing")
    sys_seconds=$(awk '/real/ { for (i = 1; i <= NF; i++) if ($i == "sys") { print $(i - 1); exit } }' "$timing")
    instructions=$(awk '/instructions retired/ { print $1; exit }' "$timing")
    cycles=$(awk '/cycles elapsed/ { print $1; exit }' "$timing")
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$worker" "${elapsed:-unknown}" "$(field nodes_deleted)" "$(field nodes_not_found)" "$(field errors)" "${rss:-unknown}" "${user_seconds:-unknown}" "${sys_seconds:-unknown}" "${instructions:-unknown}" "${cycles:-unknown}"
done
