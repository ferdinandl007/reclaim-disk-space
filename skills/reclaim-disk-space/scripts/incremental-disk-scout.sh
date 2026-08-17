#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT=${1:?usage: incremental-disk-scout.sh ROOT CACHE_DIRECTORY}
CACHE_DIR=${2:?usage: incremental-disk-scout.sh ROOT CACHE_DIRECTORY}
EVENT_TOOL="$SCRIPT_DIR/fsevents-since"
SCANNER="$SCRIPT_DIR/disk-scout"
REPORT="$CACHE_DIR/report.tsv"
ARTIFACT="$CACHE_DIR/index.bin"
EVENT_ID="$CACHE_DIR/event-id"
ROOT_FILE="$CACHE_DIR/root"
SCANNER_FINGERPRINT_FILE="$CACHE_DIR/scanner-fingerprint"
CHANGES="$CACHE_DIR/changes.tsv"
SCHEMA_VERSION=3

if [ ! -x "$EVENT_TOOL" ] || [ "$SCRIPT_DIR/fsevents-since.c" -nt "$EVENT_TOOL" ]; then
  "$SCRIPT_DIR/build-fsevents-since.sh" "$EVENT_TOOL" >/dev/null
fi
if [ ! -x "$SCANNER" ] || \
   [ "$SCRIPT_DIR/disk-scout.rs" -nt "$SCANNER" ] || \
   [ "$SCRIPT_DIR/macos_bulk_attrs.c" -nt "$SCANNER" ]; then
  "$SCRIPT_DIR/build-disk-scout.sh" "$SCANNER" >/dev/null
fi

mkdir -p "$CACHE_DIR"
LOCK_DIR="$CACHE_DIR/.lock"
if ! mkdir "$LOCK_DIR" 2>/dev/null; then
  printf 'CACHE_STATUS\tbusy\treason=another_incremental_scan_is_running\n'
  exit 5
fi
trap 'rmdir "$LOCK_DIR" 2>/dev/null || true' EXIT HUP INT TERM

SCANNER_FINGERPRINT=$(shasum -a 256 "$SCANNER" | awk '{print $1}')
FULL_REASON=
if [ ! -f "$REPORT" ] || [ ! -f "$ARTIFACT" ] || [ ! -f "$EVENT_ID" ] || [ ! -f "$ROOT_FILE" ] || [ "$(sed -n '1p' "$ROOT_FILE")" != "$ROOT" ]; then
  FULL_REASON=missing_or_root_changed
elif [ ! -f "$SCANNER_FINGERPRINT_FILE" ] || [ "$(sed -n '1p' "$SCANNER_FINGERPRINT_FILE")" != "$SCHEMA_VERSION:$SCANNER_FINGERPRINT" ]; then
  FULL_REASON=scanner_changed
fi

if [ -n "$FULL_REASON" ]; then
  START_EVENT=$($EVENT_TOOL --current)
  "$SCANNER" "$ROOT" auto --artifact "$ARTIFACT" > "$REPORT.tmp"
  mv "$REPORT.tmp" "$REPORT"
  printf '%s\n' "$START_EVENT" > "$EVENT_ID.tmp"
  mv "$EVENT_ID.tmp" "$EVENT_ID"
  printf '%s\n' "$ROOT" > "$ROOT_FILE.tmp"
  mv "$ROOT_FILE.tmp" "$ROOT_FILE"
  printf '%s\n' "$SCHEMA_VERSION:$SCANNER_FINGERPRINT" > "$SCANNER_FINGERPRINT_FILE.tmp"
  mv "$SCANNER_FINGERPRINT_FILE.tmp" "$SCANNER_FINGERPRINT_FILE"
  printf 'CACHE_STATUS\tfull_scan_created\treason=%s\tevent_id=%s\tartifact=%s\n' "$FULL_REASON" "$START_EVENT" "$ARTIFACT"
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
  printf 'CACHE_STATUS\tdirty\taction=targeted_rescan\tartifact=%s\n' "$ARTIFACT"
  cat "$CHANGES"
  exit 4
fi

CURRENT_EVENT=$(awk -F '\t' '$1 == "CURRENT" {print $2}' "$CHANGES" | tail -n 1)
if [ -n "$CURRENT_EVENT" ]; then
  printf '%s\n' "$CURRENT_EVENT" > "$EVENT_ID.tmp"
  mv "$EVENT_ID.tmp" "$EVENT_ID"
fi
printf 'CACHE_STATUS\treused\tevent_id=%s\tartifact=%s\n' "${CURRENT_EVENT:-unknown}" "$ARTIFACT"
cat "$REPORT"
