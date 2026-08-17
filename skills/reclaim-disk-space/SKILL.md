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

The runner rebuilds when its Rust or C source changes. The scanner uses a macOS `getattrlistbulk` C shim, adaptive directory-level concurrency, hard-link deduplication, compact parent-linked directory records, streaming top-K lists, and no subprocess per file. `auto` is the default interactive profile: it lowers process priority, derives its exploration ceiling from cores, RAM, and descriptor limits, monitors its own CPU use and host load, backs off to preserve responsiveness, and periodically reprobes. Use `max-throughput` only for an explicitly approved unattended run. Pass an integer only for controlled benchmarks or debugging.

For a focused scan:

```sh
scripts/run-disk-scout.sh "$HOME/Library" auto > /tmp/library-storage.tsv
```

Interpret output as follows:

- `private`: APFS bytes reported as immediately freed if the item is deleted; use this as the most conservative deletion estimate when available.
- `allocated`: allocated blocks attributed to files after hard-link deduplication; clones and snapshots can still make subtree sums differ from volume usage.
- `logical`: apparent file lengths; sparse files and clones can make this misleading.
- `tiny`, `small`, `small_text`: inode-heavy populations that can make tools and backups slow even when byte totals are moderate.
- `EXTENSION`: dynamically discovered extension totals, including `<none>` and unknown formats. Semantic categories never replace this raw accounting.
- `CATEGORY`: optional path and format-based interpretation for stores whose extensions alone are ambiguous.
- `TOP_*`: overlapping directory rankings. Select independent branches before summing.
- `ERROR_PATH`: permission or per-entry failures. State blind spots rather than claiming full attribution.

## Investigation priorities

Review these surfaces when their categories or paths are prominent:

- AI: Hugging Face, model-server, LM Studio, Ollama, PyTorch, Core ML, dataset, embedding, checkpoint, and experiment caches.
- Apple: Xcode DerivedData, Archives, DeviceSupport, CoreSimulator devices and runtimes, SwiftPM, CocoaPods, and diagnostics.
- Containers: Docker Desktop, Colima or Lima sparse disks, build cache, images, volumes, and Kubernetes data.
- Developer stacks: Python, uv, pip, Conda; JavaScript, npm, pnpm, Yarn; Rust, Cargo, rustup, target; JVM, Gradle, Maven; Go; .NET and NuGet; Ruby and Bundler; PHP and Composer; Android; native C and C++; game engines; Terraform, Pulumi, and serverless tooling.
- Media: Final Cut, Premiere, After Effects, DaVinci Resolve, Blender, proxies, optimized media, render caches, waveform or thumbnail caches, and raw project assets.
- Applications: messaging attachments, browser or editor caches and state databases, local mail, Photos, cloud-sync offline copies, crash logs, and update or install remnants.

Use [references/stack-cleanup-playbook.md](references/stack-cleanup-playbook.md) for stack-specific inspection and preferred cleanup mechanisms.

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
