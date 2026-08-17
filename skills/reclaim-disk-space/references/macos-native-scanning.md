# macOS native scanning notes

## Why `getattrlistbulk`

Traditional recursive scanners commonly call `readdir` and then `lstat` for every entry. On trees with millions of files, metadata round trips dominate. The included C shim requests names and metadata for many directory entries in one `getattrlistbulk` call, then Rust performs classification and aggregation in memory.

Directory-level workers provide useful I/O concurrency without flooding the kernel with one task per file. Auto mode starts conservatively, measures metadata entries per second, probes higher concurrency, keeps increases only when they produce sustained marginal gain, and backs off otherwise. It periodically re-probes because warm cache and directory shape change during a scan. GPU acceleration does not help because directory enumeration and metadata retrieval are kernel and filesystem operations, while classification is cheap string matching.

On Apple Silicon, each worker owns a reusable 1 MiB native result buffer aligned to the 16 KiB hardware page size. Telemetry counters are sharded per worker and aligned to 128 bytes to avoid cache-line ping-pong across performance and efficiency cores. The buffer is reused across directories, avoiding allocator and zero-fill churn on directory-heavy trees.

Child directories are opened with `openat` while their parent descriptor is already available and handed to workers through a bounded descriptor queue. This avoids repeating an absolute-path lookup from the volume root for every directory. The queue limit is derived from `RLIMIT_NOFILE`, uses at most one quarter of the process allowance, and is capped at 8,192. Paths fall back to ordinary open when the handoff window is full.

The controller is intentionally bounded rather than omniscient: directory populations change during a traversal, so no single worker count is a permanent physical constant. Its generous ceiling is the minimum of 128 times logical CPU count, one worker per 16 MiB RAM, and one worker per eight available descriptors, with a final emergency cap of 16,384. Workers are spawned lazily with 512 KiB stacks, so a powerful machine can advertise large headroom without paying for idle threads or 1 MiB native buffers. It probes geometrically and averages multiple samples at each candidate. Read `workers_best`, probe counts, and peak entry rate together. `workers_final` can be a temporary exploratory value if a short scan ends during a probe.

## APFS metrics

- `ATTR_FILE_TOTALSIZE`: logical length.
- `ATTR_FILE_ALLOCSIZE`: allocated bytes attributed to the file.
- `ATTR_CMNEXT_PRIVATESIZE`: bytes not trapped in a clone or snapshot and expected to be freed immediately on deletion.
- `(device, file ID)`: used for hard-link deduplication when link count is greater than one.

Private size is a conservative signal, not a transaction guarantee. Open files, snapshots created after scanning, hard links outside the root, application behavior, and filesystem accounting can change the result.

## Coverage limitations

- The scanner does not follow symbolic links.
- It skips entries whose device differs from the scan root to avoid crossing mounts.
- macOS privacy controls and permissions can hide paths. Full Disk Access may be necessary for complete Mail, Messages, Photos, or other protected-data attribution.
- Data-volume scans can contain APFS firmlink views and clone relationships whose subtree sums do not equal the volume used-block counter.
- Top-directory entries overlap by design; never add nested rankings.

## Performance tuning

- Use `auto` for normal scans. Use a fixed worker count only to reproduce benchmarks or diagnose a filesystem-specific issue.
- Scan a focused root when the likely area is known.
- Redirect TSV output to a file; do not stream millions of progress messages.
- Keep the 1 MiB native per-directory buffer unless profiling proves a different size is materially better.
- Benchmark elapsed time and correctness on the same warm or cold cache conditions before claiming a speedup.

## Incremental discovery

Use the included FSEvents helper to detect changes after a completed report. FSEvents is a dirty-path signal, not a journal to replay as filesystem truth. Reconcile dirty subtrees with the native scanner. Treat `MustScanSubDirs`, user or kernel drops, event-ID wrap, and watched-root changes as full-refresh conditions.

General APFS directories cannot rely on `ATTR_CMNEXT_RECURSIVE_GENCOUNT`; it is nonzero only for directories explicitly marked to maintain directory statistics. The cache therefore uses FSEvents and fails closed when history is incomplete.
