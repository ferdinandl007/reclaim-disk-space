# Classification and cleanup risk

## Size semantics

- Prefer APFS `private` bytes for immediate-reclaim estimates.
- Use allocated bytes when private size is unavailable, but disclose clone or snapshot uncertainty.
- Use logical bytes to understand content volume, not guaranteed disk reclaim.
- Deduplicate hard links. Do not sum overlapping parent and child directories.
- Treat millions of tiny files as a performance and inode-management problem even when their bytes are modest.

## Risk tiers

### Tier 1 — regenerated cache or disposable build output

Examples: Xcode DerivedData, compiler caches, package download caches, browser code or GPU cache, language build outputs, stale crash logs, thumbnails, and render cache explicitly marked regenerable.

An agent may recommend these first, but must still show exact paths and obtain deletion confirmation. Explain the next build, download, or render cost.

### Tier 2 — managed runtime state

Examples: simulator devices, Xcode archives, Docker images and stopped containers, Docker or Colima volumes, rustup toolchains, Android SDKs or AVDs, local model-manager downloads, and editor extension caches.

Use the owning application's CLI or UI where possible. Inventory what will disappear. Volumes, archives, toolchains, and simulator data may contain work that is not reproducible.

### Tier 3 — application databases and synchronized or offline content

Examples: messaging media stores, Cursor or VS Code state databases, browser profiles, Mail, cloud-drive offline copies, Photos libraries, and local database servers.

Do not raw-delete these based on size alone. Close the app, verify backup or sync state, and prefer application-native retention or reset controls. A cache-looking filename can still be authoritative state.

### Tier 4 — user-owned source and media

Examples: repositories, Documents or Desktop projects, datasets, checkpoints, recordings, Final Cut libraries, Premiere or Resolve projects, photos, exports, archives, signing assets, and credentials.

Never classify these as automatic cleanup. Present them as archive, move, or review candidates only.

## Classification caveats

- Treat the dynamic extension table as the raw file-format accounting layer. Do not require an extension to exist in a hardcoded list before reporting it.
- Treat semantic categories as interpretation only. They capture extensionless stores and directory context but must not suppress unknown extensions.
- Extensions are hints. A `.bin` may be a model, firmware, archive member, or application resource.
- Directory context outranks generic extensions when context is specific, except unmistakable model and dataset formats.
- Sparse VM disks can have huge logical size and small allocation.
- APFS clones share extents; deleting one clone may free little or nothing.
- Snapshot-pinned bytes may not be released until the relevant snapshot is removed by the owning system.
- Hard links outside the scan root can reduce actual reclaim below the reported subtree estimate.
