#!/bin/sh
set -eu

PROJECT_ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
SCANNER="$PROJECT_ROOT/skills/reclaim-disk-space/scripts/run-disk-scout.sh"
FIXTURE=$(mktemp -d "${TMPDIR:-/tmp}/reclaim-disk-space-scout.XXXXXX")
REPORT="$FIXTURE/report.tsv"
trap 'rm -rf "$FIXTURE"' EXIT HUP INT TERM

mkdir -p "$FIXTURE/photos" "$FIXTURE/noisy-text" "$FIXTURE/project/.git/logs/refs/heads" "$FIXTURE/project/.git/refs/heads" "$FIXTURE/project/.venv" "$FIXTURE/project/src" "$FIXTURE/project/build"
mkdir -p "$FIXTURE/git-project"
git -c init.defaultBranch=main init -q "$FIXTURE/git-project"
git -C "$FIXTURE/git-project" config user.name fixture
git -C "$FIXTURE/git-project" config user.email fixture@example.invalid
printf '%s\n' committed > "$FIXTURE/git-project/main.py"
git -C "$FIXTURE/git-project" add main.py
git -C "$FIXTURE/git-project" -c commit.gpgsign=false commit -q -m fixture
for letter in a b c d e f g h i j k l m n o p q r s t u v w x y z; do
    printf '%s\n' 'ordinary text' > "$FIXTURE/noisy-text/plain-$letter.txt"
done
printf '%s\n' 'ref: refs/heads/main' > "$FIXTURE/project/.git/HEAD"
printf '%s\n' 'fixture' > "$FIXTURE/project/.git/refs/heads/main"
printf '%s\n' 'fixture' > "$FIXTURE/project/.git/logs/HEAD"
touch -t 202001010000 "$FIXTURE/project/.git/HEAD" "$FIXTURE/project/.git/refs/heads/main" "$FIXTURE/project/.git/logs/HEAD"
truncate -s 120000 "$FIXTURE/photos/photo.jpg"
truncate -s 118000 "$FIXTURE/photos/photo v2.jpg"
truncate -s 117000 "$FIXTURE/photos/photo (3).jpg"
NEWLINE_PHOTO=$(printf 'photo\nbase.jpg')
NEWLINE_PHOTO_V2=$(printf 'photo\nbase v2.jpg')
truncate -s 116000 "$FIXTURE/photos/$NEWLINE_PHOTO"
truncate -s 115000 "$FIXTURE/photos/$NEWLINE_PHOTO_V2"
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
timestamp_queries=$(awk -F '\t' '/^SUMMARY/{for (i = 1; i <= NF; i++) if ($i ~ /^timestamp_queries=/) { split($i, parts, "="); print parts[2] }}' "$REPORT")
[ "$timestamp_queries" -lt 40 ]
grep -q '^ENVIRONMENT.*kind=python_venv' "$REPORT"
grep -q '^PROJECT.*source_files=.*generated_files=' "$REPORT"
grep -q '^PROJECT.*git_repo=true.*git_branch=main.*activity_basis=source+git_ref' "$REPORT"
grep -q '^GIT_REPOSITORY.*branch=main.*worktree_state=unknown' "$REPORT"
grep -q '^GIT_REPOSITORY.*worktree_state=unknown.*index_entries=1' "$REPORT"
grep -q '^PROJECT.*stale_review=true' "$REPORT"
grep -q '^VERSION_CLUSTER.*evidence_quality=' "$REPORT"
grep -F -q '\n' "$REPORT"

mkdir -p "$FIXTURE/project/.git/rebase-merge"
IN_PROGRESS_REPORT="$FIXTURE/in-progress-report.tsv"
"$SCANNER" "$FIXTURE" 2 > "$IN_PROGRESS_REPORT"
grep -q '^GIT_REPOSITORY.*worktree_state=in_progress' "$IN_PROGRESS_REPORT"

mkdir -p "$FIXTURE/unreadable"
chmod 000 "$FIXTURE/unreadable"
set +e
INCOMPLETE_REPORT="$FIXTURE/incomplete-report.tsv"
"$SCANNER" "$FIXTURE" 2> "$FIXTURE/incomplete-errors.log" > "$INCOMPLETE_REPORT"
incomplete_rc=$?
set -e
chmod 755 "$FIXTURE/unreadable"
[ "$incomplete_rc" -ne 0 ]
grep -q '^SUMMARY.*partial_directories=' "$INCOMPLETE_REPORT"
grep -q '^ERROR_PATH.*reason=' "$INCOMPLETE_REPORT"

printf '%s\n' modified >> "$FIXTURE/git-project/main.py"
DIRTY_REPORT="$FIXTURE/dirty-report.tsv"
"$SCANNER" "$FIXTURE" 2 > "$DIRTY_REPORT"
grep -q '^GIT_REPOSITORY.*worktree_state=dirty.*modified_tracked_files=1' "$DIRTY_REPORT"

echo "Scanner fixture checks passed"
