---
name: reclaim-disk-space
description: Audit and safely reclaim disk space on macOS developer, AI, iOS, media, and general-purpose workstations. Use when a disk is unexpectedly full; when Xcode, simulators, Docker, Colima, package caches, model weights, datasets, build products, editor databases, media caches, logs, or millions of small files may be responsible; or when an agent needs an evidence-based cleanup plan without risking user data.
---

# Reclaim Disk Space

Find storage consumers that ordinary largest-file searches miss, separate logical size from real allocation and APFS immediate-reclaim estimates, and produce a confirmation-safe cleanup plan.

## Safety boundary

- Scan read-only first. Never infer permission to delete from a request to inspect, diagnose, or report.
- Before deleting, show exact resolved paths, estimated immediate reclaim, risk tier, regeneration consequence, and any app or process that must be closed.
- Obtain explicit confirmation for the exact targets. A broad phrase such as “delete everything” does not authorize deleting projects, documents, messages, photos, model training data, databases, credentials, or broad roots.
- Never recursively delete `/`, `/System/Volumes/Data`, a home directory, an unresolved variable, or a glob whose expansion has not been inspected.
- Prefer app-native cleanup, package-manager cleanup, or Trash over raw deletion. Re-measure free space afterward.

Read [references/classification-and-risk.md](references/classification-and-risk.md) before proposing cleanup. Read [references/macos-native-scanning.md](references/macos-native-scanning.md) when interpreting APFS values, permission gaps, or scanner performance.

## Workflow

1. Establish filesystem truth with `df -h /System/Volumes/Data` and `diskutil apfs list`. Record free space before cleanup.
2. Run the native scanner over the narrowest useful root first. Use the Data volume when unexplained space is system-wide.
3. Compare `private`, `allocated`, `logical`, file count, tiny-file count, small-text count, and the dynamic extension table. Do not rank only by logical bytes.
4. Investigate the top non-overlapping branches and categories. Avoid adding parent and child totals together.
5. Label candidates by risk and confidence. Distinguish cache or build output from user-owned or application-database content.
6. Present a cleanup proposal with exact targets and conservative reclaim estimates. Stop for confirmation before mutation.
7. Close affected applications, execute only confirmed cleanup, and verify both application health and filesystem free space.

For confirmed trees containing very large file populations, use the guarded native deletion utility instead of opaque recursive shell removal. It enumerates once, refuses broad and cross-filesystem roots, requires the canonical root twice for execution, retries interrupted unlinks, discovers useful concurrency dynamically, backs off under low-space/error/latency pressure, and emits live progress:

```sh
# Read-only plan
scripts/run-disk-clean.sh --root "/exact/confirmed/path"

# Execute only after the user confirms this exact canonical path
scripts/run-disk-clean.sh --root "/exact/confirmed/path" \
  --execute --confirm "/exact/confirmed/path" --workers auto --profile interactive
```

Rerunning the same command is resumable because missing entries count as complete. Never use the utility to translate a broad cleanup request into deletion authority; the Safety boundary still applies.

## Native scan

Build and run with:

```sh
scripts/run-disk-scout.sh /System/Volumes/Data auto > /tmp/disk-scout-report.tsv
```

The runner rebuilds when its Rust or C source changes. The scanner uses a macOS `getattrlistbulk` C shim, adaptive directory-level concurrency, hard-link deduplication, compact parent-linked directory records, streaming top-K lists, selective filesystem birth/modified-time queries, and no subprocess per file. `auto` is the default interactive profile: it lowers process priority, derives its exploration ceiling from cores, RAM, and descriptor limits, monitors its own CPU use and host load, backs off to preserve responsiveness, and periodically reprobes. `max-throughput` starts at roughly half the logical CPU count so short scans do not wait for a probe window, then continues adaptive probing; use it only for an explicitly approved unattended run. Detailed per-directory scanner timing is disabled by default and is enabled only with `DISK_SCOUT_PROFILE=1`; the cleaner uses the analogous `DISK_CLEAN_PROFILE=1` flag. Pass an integer only for controlled benchmarks or debugging.

For a focused scan:

```sh
scripts/run-disk-scout.sh "$HOME/Library" auto > /tmp/library-storage.tsv
```

Interpret output as follows:

- `private`: APFS bytes reported as immediately freed if the item is deleted; use this as the most conservative deletion estimate when available.
- `allocated`: allocated blocks attributed to files after hard-link deduplication; clones and snapshots can still make subtree sums differ from volume usage.
- `logical`: apparent file lengths; sparse files and clones can make this misleading.
- `tiny`, `small`, `small_text`: inode-heavy populations that can make tools and backups slow even when byte totals are moderate.
- `EXTENSION`: dynamically discovered extension totals, including `<none>` and unknown formats. Semantic categories never replace this raw accounting. To protect memory on adversarial trees, the extension index is capped and excess keys aggregate into `<other>`.
- `timestamp_queries`: how many entries required an exact native timestamp lookup; a much smaller number than total entries is expected in the fast path.
- `native_directories`, `fallback_directories`, and `partial_directories`: backend and completeness telemetry. A nonzero `partial_directories` or `permission_errors` makes the scanner exit nonzero unless `DISK_SCOUT_ALLOW_INCOMPLETE=1` is explicitly set.
- `CATEGORY`: optional path and format-based interpretation for stores whose extensions alone are ambiguous.
- `ENVIRONMENT`: every detected standard or marker-confirmed Python environment (`.venv`, `venv`, `virtualenv`, `.python`, or a directory containing `pyvenv.cfg`), UV-backed `.venv`, plus Conda environments under Conda `envs` roots or containing `conda-meta`, with independent totals, newest modified epoch, age, and a `stale_review` hint. This is an inventory signal, not deletion authority.
- `PROJECT`: detected Python, JavaScript, Rust, Go, iOS, Docker, JVM, and related project roots, with source/generated file totals, source activity age, Git ref activity when a `.git` directory is present, repository overlap, and a stale-review hint. `activity_basis` keeps source and Git evidence explicit; generated activity never counts as source freshness. `stale_review=review` means old source plus recent Git metadata and requires human review. A project is never automatically selected for deletion.
- `GIT_REPOSITORY`: bounded, read-only Git metadata for detected repositories: branch/HEAD, resolved HEAD object when safe, common object-store directory, ref and index mtimes, worktree/remote/submodule counts, and `worktree_state`. `worktree_state=unknown` is intentional until index comparison proves cleanliness; `in_progress` is a safety stop for merge/rebase/cherry-pick markers. No Git subprocesses or hooks are executed.
- `EVIDENCE_SUMMARY`: total versus reported record counts, including any bounded-report truncation.
- `VERSION_CLUSTER` and `VERSION_MEMBER`: conservative same-directory families such as `photo.jpg`, `photo v2.jpg`, `photo (3).jpg`, or exported app variants. They combine filename normalization, size similarity, and creation/modified-date proximity; `evidence_quality` exposes which signals were actually available, and `suggested_keep` is only a review starting point.
- `TOP_*`: diagnostic rankings that may overlap. For additive cleanup candidates, use the persisted artifact `independent` query, which partitions the tree and guarantees `overlap=false`.
- `ERROR_PATH`: permission or per-entry failures with a reason. State blind spots rather than claiming full attribution.
- `HARDLINK_SUMMARY`: duplicate accounting telemetry. The current fast path attributes shared inode bytes to the first observed path; reports mark that attribution as nondeterministic rather than pretending it is canonical.

Machine-readable fields use TSV escaping: literal tabs, newlines, carriage returns, and backslashes are emitted as `\\t`, `\\n`, `\\r`, and `\\\\`. Agents should decode those sequences before displaying paths.

The incremental helper invalidates its cached report when the scanner binary or report schema changes; a clean filesystem event stream alone is not enough to reuse an old report. It also takes an atomic cache lock and reports `CACHE_STATUS busy` rather than allowing concurrent writers to corrupt the cache.

## Persisted artifact and instant investigation

Add `--artifact /exact/path/index.bin` to a scan to persist a compact directory index. The index stores parent links, direct and recursive APFS metrics, file-count pressure, context, environment and project markers, and is written atomically. It contains one record per directory and never one record per file.

Query it without touching the filesystem:

```sh
scripts/run-disk-scout.sh query /exact/path/index.bin summary
scripts/run-disk-scout.sh query /exact/path/index.bin independent private 50
scripts/run-disk-scout.sh query /exact/path/index.bin environments
scripts/run-disk-scout.sh query /exact/path/index.bin packages
scripts/run-disk-scout.sh query /exact/path/index.bin projects
scripts/run-disk-scout.sh query /exact/path/index.bin path /exact/path/to/investigate
```

`environments` lists every discovered Python, Conda, and UV environment and emits grouped totals. `packages` emits non-overlapping Conda package, Python `site-packages`, language dependency, and package-cache scopes, with a `scope_kind` on each row and grouped `PACKAGE_TOTAL` rows for fast agent aggregation. `path` returns one directory plus its immediate children. `independent` uses a best-first tree partition so no selected path is an ancestor of another selected path. The incremental helper stores this index beside `report.tsv` and reuses both when FSEvents is clean; dirty history fails closed and requests a targeted/full refresh rather than silently presenting stale data.

## Investigation priorities

Review these surfaces when their categories or paths are prominent:

- AI: Hugging Face, model-server, LM Studio, Ollama, PyTorch, Core ML, dataset, embedding, checkpoint, and experiment caches.
- Apple: Xcode DerivedData, Archives, DeviceSupport, CoreSimulator devices and runtimes, SwiftPM, CocoaPods, and diagnostics.
- Containers: Docker Desktop, Colima or Lima sparse disks, build cache, images, volumes, and Kubernetes data.
- Developer stacks: Python, uv, pip, Conda; JavaScript, npm, pnpm, Yarn; Rust, Cargo, rustup, target; JVM, Gradle, Maven; Go; .NET and NuGet; Ruby and Bundler; PHP and Composer; Android; native C and C++; game engines; Terraform, Pulumi, and serverless tooling.
- Media: Final Cut, Premiere, After Effects, DaVinci Resolve, Blender, proxies, optimized media, render caches, waveform or thumbnail caches, and raw project assets.
- Applications: messaging attachments, browser or editor caches and state databases, local mail, Photos, cloud-sync offline copies, crash logs, and update or install remnants.

When a cleanup candidate is old, inspect the matching `ENVIRONMENT`, `PROJECT`, `GIT_REPOSITORY`, or `VERSION_CLUSTER` records together with the exact path. Project age is based on the newest source/configuration activity, with Git ref activity shown as a separate corroborating signal; generated/cache activity is reported separately. These are review hints rather than hardcoded deletion rules.

Use [references/stack-cleanup-playbook.md](references/stack-cleanup-playbook.md) for stack-specific inspection and preferred cleanup mechanisms.

The cleanup executor writes bounded deletion targets to a private temporary spool, then removes files through root-relative directory handles with device/inode/type revalidation. It refuses incomplete inventories and reports separate attempted, deleted, not-found, and error counts. A cleanup error or race mismatch is a failed deletion, never silent success.

## Reporting contract

Report:

- current capacity, used space, and free space;
- scanner coverage, elapsed time, permission failures, and mount exclusions;
- largest independent storage branches;
- file-type or category totals and inode-heavy hotspots;
- a cleanup table with exact path, ownership, risk tier, conservative reclaim, recommended mechanism, and regeneration cost;
- a separate do-not-remove-automatically list for user data and application databases;
- expected total reclaim as a range, never as an inflated sum of overlapping paths.

If deletion is requested, split the proposal into small reversible batches and verify free space after each batch.

## Incremental cache

For repeated agent checks, use:

```sh
scripts/incremental-disk-scout.sh "$HOME/Library" /tmp/disk-scout-library-cache
```

The first call creates a full report and records the starting FSEvent ID. Later calls reuse the report instantly when no changes occurred. If FSEvents reports dirty paths, the command exits with status 4 and prints those paths for targeted rescans. If history was dropped, wrapped, or the watched root changed, it exits with status 3 and requires a full refresh. Never present a dirty cached report as current.

This cache deliberately stores one aggregate report rather than a per-file database; on nearly full systems, the index must not become another large storage consumer.
