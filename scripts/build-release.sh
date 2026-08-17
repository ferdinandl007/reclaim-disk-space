#!/bin/sh
set -eu

PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
OUTPUT_ROOT=${1:?usage: build-release.sh OUTPUT_ROOT VERSION}
VERSION=${2:?usage: build-release.sh OUTPUT_ROOT VERSION}
ARCH=arm64
TARGET=aarch64-apple-darwin
MIN_MACOS=13.0

case "$VERSION" in
  ''|.|..|*[!A-Za-z0-9._-]*)
    echo "VERSION must contain only letters, numbers, dots, underscores, and hyphens" >&2
    exit 2
    ;;
esac

STAGE_NAME="reclaim-disk-space-${VERSION}-macos-arm64"
STAGE="$OUTPUT_ROOT/$STAGE_NAME"
BUILD_DIR=${TMPDIR:-/tmp}/reclaim-disk-space-release-build

case "$(uname -m)" in
  arm64) ;;
  *)
    echo "Apple Silicon release builds must run on an arm64 macOS host" >&2
    exit 2
    ;;
esac

command -v clang >/dev/null 2>&1 || { echo "clang is required" >&2; exit 1; }
command -v rustc >/dev/null 2>&1 || { echo "rustc is required" >&2; exit 1; }

mkdir -p "$OUTPUT_ROOT" "$BUILD_DIR"
rm -rf "$STAGE"
mkdir -p "$STAGE/skills"
cp -R "$PROJECT_ROOT/skills/reclaim-disk-space" "$STAGE/skills/"

clang -O3 -DNDEBUG -Wall -Wextra -Werror \
  -arch "$ARCH" -mmacosx-version-min="$MIN_MACOS" \
  -c "$PROJECT_ROOT/skills/reclaim-disk-space/scripts/macos_bulk_attrs.c" \
  -o "$BUILD_DIR/macos_bulk_attrs-arm64.o"

rustc --edition=2021 -C opt-level=3 -C target-cpu=generic \
  --target "$TARGET" \
  -C link-arg="$BUILD_DIR/macos_bulk_attrs-arm64.o" \
  -C link-arg=-mmacosx-version-min="$MIN_MACOS" \
  "$PROJECT_ROOT/skills/reclaim-disk-space/scripts/disk-scout.rs" \
  -o "$STAGE/skills/reclaim-disk-space/scripts/disk-scout"

rustc --edition=2021 -C opt-level=3 -C target-cpu=generic \
  --target "$TARGET" \
  -C link-arg="$BUILD_DIR/macos_bulk_attrs-arm64.o" \
  -C link-arg=-mmacosx-version-min="$MIN_MACOS" \
  "$PROJECT_ROOT/skills/reclaim-disk-space/scripts/disk-clean.rs" \
  -o "$STAGE/skills/reclaim-disk-space/scripts/disk-clean"

clang -O3 -DNDEBUG -Wall -Wextra -Werror \
  -arch "$ARCH" -mmacosx-version-min="$MIN_MACOS" \
  "$PROJECT_ROOT/skills/reclaim-disk-space/scripts/fsevents-since.c" \
  -framework CoreServices \
  -o "$STAGE/skills/reclaim-disk-space/scripts/fsevents-since"

chmod 755 \
  "$STAGE/skills/reclaim-disk-space/scripts/disk-scout" \
  "$STAGE/skills/reclaim-disk-space/scripts/disk-clean" \
  "$STAGE/skills/reclaim-disk-space/scripts/fsevents-since"

cp "$PROJECT_ROOT/scripts/install-release.sh" "$STAGE/install.sh"
chmod 755 "$STAGE/install.sh"

file \
  "$STAGE/skills/reclaim-disk-space/scripts/disk-scout" \
  "$STAGE/skills/reclaim-disk-space/scripts/disk-clean" \
  "$STAGE/skills/reclaim-disk-space/scripts/fsevents-since"

tar -C "$OUTPUT_ROOT" -czf "$OUTPUT_ROOT/$STAGE_NAME.tar.gz" "$STAGE_NAME"
printf '%s\n' "$OUTPUT_ROOT/$STAGE_NAME.tar.gz"
