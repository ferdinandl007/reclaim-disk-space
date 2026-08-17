# Reclaim Disk Space

[![macOS CI](https://github.com/ferdinandl007/reclaim-disk-space/actions/workflows/ci.yml/badge.svg)](https://github.com/ferdinandl007/reclaim-disk-space/actions/workflows/ci.yml)

Native, evidence-first disk auditing and guarded cleanup for macOS developer, AI, iOS, container, and media workstations.

Most disk tools answer only “which files are large?” This project also answers:

- What is actually allocated on APFS versus merely logical or clone-attributed?
- Which directories contain millions of tiny files or small text files?
- Which storage belongs to Xcode, simulators, Docker, Colima, Python, JavaScript, Rust, AI models, media, or other development stacks?
- What can be regenerated safely, and what is application state or user data?
- How can a cleanup agent delete only an explicitly confirmed canonical path?

The scanner is written in Rust with a small macOS C shim using `getattrlistbulk`. It uses adaptive directory-level concurrency, hard-link deduplication, compact aggregation, dynamic extension accounting, and a self-monitoring interactive mode. The guarded cleaner enumerates once, refuses dangerous roots and cross-filesystem traversal, requires exact-path confirmation, and reports live progress.

## Safety first

Scanning is read-only. Cleanup is deliberately separate.

The cleaner will not accept `/`, `/System/Volumes/Data`, a home directory, a relative path, or a root that crosses onto another filesystem. Execution requires the exact canonical path twice:

```sh
skills/reclaim-disk-space/scripts/run-disk-clean.sh \
  --root "$HOME/Library/Developer/Xcode/DerivedData" \
  --execute \
  --confirm "$HOME/Library/Developer/Xcode/DerivedData" \
  --workers auto \
  --profile interactive
```

Always run the read-only plan first and close the owning application. This tool cannot determine whether an application database, model, dataset, simulator, Docker volume, or project is personally important. Treat those as review candidates, not automatic cleanup.

## Quick start

Requirements:

- macOS with Apple Command Line Tools (`clang`)
- Rust compiler (`rustc`)
- A local filesystem path you have permission to inspect

Build the native tools:

```sh
make build
```

Scan a focused area first:

```sh
make scan ROOT="$HOME/Library" OUT=/tmp/library-storage.tsv
```

For a system-wide report:

```sh
make scan ROOT=/System/Volumes/Data OUT=/tmp/data-volume-storage.tsv
```

Read the report as TSV. Important records include `SUMMARY`, `CATEGORY`, `EXTENSION`, `TOP_PRIVATE`, `TOP_ALLOCATED`, `TOP_LOGICAL`, `SMALL_TEXT_HOTSPOT`, and `ERROR_PATH`.

Generate a deletion plan without changing anything:

```sh
make plan ROOT="$HOME/Library/Developer/Xcode/DerivedData"
```

Execute only after reviewing and confirming that exact path:

```sh
make delete CONFIRM="$HOME/Library/Developer/Xcode/DerivedData"
```

Use `PROFILE=max-throughput` only for an explicitly approved unattended run. `auto` is the responsive default and adapts its ceiling from hardware, file-descriptor limits, observed latency, process CPU, and host load.

## Install the agent skill

The repository includes a reusable Codex skill at `skills/reclaim-disk-space`. Install it into a local Codex skill directory:

```sh
mkdir -p "$HOME/.codex/skills"
cp -R skills/reclaim-disk-space "$HOME/.codex/skills/reclaim-disk-space"
```

The skill teaches an agent how to scan first, interpret APFS allocation, classify common developer and AI stacks, identify inode-heavy trees, propose exact cleanup targets, and verify the result. It does not grant permission to delete user data.

## What makes it different

### APFS-aware accounting

The scanner reports logical length, allocated blocks, and macOS private size. These values can diverge dramatically because of sparse files, clones, snapshots, and hard links. The report also keeps parent rankings visibly overlapping so totals are not accidentally double-counted.

### Dynamic file-type discovery

Extensions are counted dynamically, including extensionless files and unknown formats. Semantic categories are an interpretation layer for directory and format context; they never hide raw extension totals. Categories cover common Python/uv/pip/Conda, JavaScript/npm/pnpm/Yarn, Rust/Cargo, JVM, Go, .NET, Ruby, PHP, native C/C++, Android, iOS/macOS, Docker/Colima, AI models and datasets, databases, browsers/editors, and media tooling.

### Small-file and text-file hotspots

The report counts tiny files, small files, and small text files. This catches dependency trees, generated metadata, caches, indexes, and repositories that hurt backup and filesystem performance without appearing among the largest individual files.

### Adaptive concurrency

Directory workers are discovered rather than fixed. Interactive mode starts conservatively, probes useful concurrency, backs off when metadata latency or host load rises, and monitors the utility’s own CPU use. The ceiling is derived from logical CPUs, memory, file descriptors, and a generous safety cap, so the same binary can scale across Apple Silicon machines without assuming a particular core count. GPU work is intentionally not used: filesystem metadata enumeration is kernel and storage bound, not a GPU workload.

### Native macOS metadata path

The C shim batches directory metadata through `getattrlistbulk`; Rust performs classification and aggregation without launching a subprocess per file. The same native metadata layer is shared by the scanner and guarded cleaner.

## Incremental scans

For repeated agent checks, use the FSEvents-assisted incremental wrapper:

```sh
skills/reclaim-disk-space/scripts/incremental-disk-scout.sh \
  "$HOME/Library" \
  /tmp/library-storage-cache
```

The first run creates a full report and records an FSEvents starting point. Later runs reuse the report when nothing changed, or print dirty paths for a targeted rescan. It refuses to present a stale cache as current.

## Limitations

- macOS privacy controls and Full Disk Access can hide protected Mail, Messages, Photos, and application data.
- Symbolic links are not followed.
- Mounts on a different device are skipped.
- APFS firmlinks, clones, snapshots, and open files can make subtree sums differ from `df`.
- Simulator runtimes, Docker/Colima volumes, editor databases, model weights, datasets, archives, credentials, and source trees require application-aware review.
- This is macOS-specific; the scanner currently relies on macOS APIs and filesystem semantics.

## Development

The project intentionally has no third-party Rust crates. Build scripts compile the C shim with `clang` and the Rust binaries with `rustc` using native optimization. Generated binaries are ignored by Git.

Before opening a pull request:

```sh
make build
python3 "${CODEX_HOME:-$HOME/.codex}/skills/.system/skill-creator/scripts/quick_validate.py" skills/reclaim-disk-space
```

If the validator is not available at that path, run the equivalent `quick_validate.py` from your Codex skill installation. Also test a read-only focused scan on a temporary directory and verify that deletion refuses broad roots.

See [CONTRIBUTING.md](CONTRIBUTING.md) and [SECURITY.md](SECURITY.md).

## License

MIT. See [LICENSE](LICENSE).
