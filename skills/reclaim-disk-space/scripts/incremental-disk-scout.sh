#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT=${1:?usage: incremental-disk-scout.sh ROOT CACHE_DIRECTORY}
CACHE_DIR=${2:?usage: incremental-disk-scout.sh ROOT CACHE_DIRECTORY}
EVENT_TOOL="$SCRIPT_DIR/fsevents-since"
SCANNER="$SCRIPT_DIR/disk-scout"
REPORT="$CACHE_DIR/report.tsv"
EVENT_ID="$CACHE_DIR/event-id"
ROOT_FILE="$CACHE_DIR/root"
CHANGES="$CACHE_DIR/changes.tsv"

if [ ! -x "$EVENT_TOOL" ] || [ "$SCRIPT_DIR/fsevents-since.c" -nt "$EVENT_TOOL" ]; then
  "$SCRIPT_DIR/build-fsevents-since.sh" "$EVENT_TOOL" >/dev/null
fi
if [ ! -x "$SCANNER" ] || \
   [ "$SCRIPT_DIR/disk-scout.rs" -nt "$SCANNER" ] || \
   [ "$SCRIPT_DIR/macos_bulk_attrs.c" -nt "$SCANNER" ]; then
  "$SCRIPT_DIR/build-disk-scout.sh" "$SCANNER" >/dev/null
fi

mkdir -p "$CACHE_DIR"

if [ ! -f "$REPORT" ] || [ ! -f "$EVENT_ID" ] || [ ! -f "$ROOT_FILE" ] || [ "$(sed -n '1p' "$ROOT_FILE")" != "$ROOT" ]; then
  START_EVENT=$($EVENT_TOOL --current)
  "$SCANNER" "$ROOT" auto > "$REPORT.tmp"
  mv "$REPORT.tmp" "$REPORT"
  printf '%s\n' "$START_EVENT" > "$EVENT_ID.tmp"
  mv "$EVENT_ID.tmp" "$EVENT_ID"
  printf '%s\n' "$ROOT" > "$ROOT_FILE.tmp"
  mv "$ROOT_FILE.tmp" "$ROOT_FILE"
  printf 'CACHE_STATUS\tfull_scan_created\tevent_id=%s\n' "$START_EVENT"
  cat "$REPORT"
  exit 0
fi

"$EVENT_TOOL" "$ROOT" "$(sed -n '1p' "$EVENT_ID")" > "$CHANGES.tmp"
mv "$CHANGES.tmp" "$CHANGES"

if rg -q '^RESET\t' "$CHANGES"; then
  printf 'CACHE_STATUS\tfull_refresh_required\treason=fsevents_reset\n'
  cat "$CHANGES"
  exit 3
fi

if rg -q '^EVENT\t' "$CHANGES"; then
  printf 'CACHE_STATUS\tdirty\taction=targeted_rescan\n'
  cat "$CHANGES"
  exit 4
fi

CURRENT_EVENT=$(awk -F '\t' '$1 == "CURRENT" {print $2}' "$CHANGES" | tail -n 1)
if [ -n "$CURRENT_EVENT" ]; then
  printf '%s\n' "$CURRENT_EVENT" > "$EVENT_ID.tmp"
  mv "$EVENT_ID.tmp" "$EVENT_ID"
fi
printf 'CACHE_STATUS\treused\tevent_id=%s\n' "${CURRENT_EVENT:-unknown}"
cat "$REPORT"
