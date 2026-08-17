#!/bin/sh
set -eu

case "$(uname -m)" in
  arm64) ;;
  *)
    echo "This release contains Apple Silicon binaries only (arm64)." >&2
    exit 2
    ;;
esac

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
CODEX_ROOT=${CODEX_HOME:-"$HOME/.codex"}
SKILL_DEST="$CODEX_ROOT/skills/reclaim-disk-space"
BIN_DEST=${LOCAL_BIN:-"$HOME/.local/bin"}

mkdir -p "$SKILL_DEST" "$BIN_DEST"
cp -R "$SCRIPT_DIR/skills/reclaim-disk-space/." "$SKILL_DEST/"

install -m 755 \
  "$SKILL_DEST/scripts/disk-scout" \
  "$BIN_DEST/reclaim-disk-scout"
install -m 755 \
  "$SKILL_DEST/scripts/disk-clean" \
  "$BIN_DEST/reclaim-disk-clean"
install -m 755 \
  "$SKILL_DEST/scripts/fsevents-since" \
  "$BIN_DEST/reclaim-fsevents-since"

printf 'Installed the Reclaim Disk Space Codex skill to %s\n' "$SKILL_DEST"
printf 'Installed native tools to %s\n' "$BIN_DEST"
printf 'If %s is not on PATH, add: export PATH="%s:$PATH"\n' "$BIN_DEST" "$BIN_DEST"

