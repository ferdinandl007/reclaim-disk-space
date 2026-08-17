# Stack cleanup playbook

Inventory first. Commands below are examples of preferred ownership boundaries, not blanket deletion authorization.

## Apple development

- Inspect Xcode storage settings, `~/Library/Developer/Xcode`, `~/Library/Developer/CoreSimulator`, `/Library/Developer/CoreSimulator`, and `~/Library/Caches/org.swift.swiftpm`.
- Remove unavailable simulator records with `xcrun simctl delete unavailable` only after reviewing `xcrun simctl list`.
- Treat Archives, DeviceSupport, simulator device data, and downloaded runtimes as managed state, not ordinary cache.

## Containers and VMs

- Use `docker system df -v` before any prune. Inspect volumes separately; database volumes are often the only copy of data.
- Prefer targeted Docker image or build-cache cleanup to broad `docker system prune --volumes`.
- For Colima or Lima, inspect profile status and sparse-disk allocated size. Stop the VM before profile-level maintenance.

## AI and Python

- Inspect uv, pip, Conda, Hugging Face, Torch, model-server, LM Studio, Ollama, experiment-tracker, notebook, checkpoint, and dataset locations separately.
- Prefer `uv cache clean`, `pip cache purge`, model-manager removal, or dataset-library cleanup after identifying redownload cost.
- Virtual environments are Tier 1 only when lockfiles and native build prerequisites can reproduce them. Checkpoints and datasets are Tier 4.

## JavaScript and frontend

- Inspect npm, pnpm, Yarn, Bun, Playwright browser downloads, `node_modules`, `.next`, `.turbo`, framework caches, and package-manager stores.
- Package stores can be shared across projects. Prefer the package manager's prune or clean mechanism.

## Rust

- Inspect `~/.cargo/registry`, `~/.cargo/git`, `~/.rustup/toolchains`, project `target` trees, sccache, and mold or LLVM artifacts.
- Project `target` is generally regenerable. Cargo registry or git caches are redownloadable. Remove toolchains and components with `rustup`, especially when multiple targets or nightly snapshots are installed.
- Preserve unpublished crates, local registries, credentials, signing keys, and source trees.

## JVM, Android, and native builds

- Inspect Gradle caches, wrappers and daemons, Maven or Ivy repositories, Android SDK system images and AVDs, Bazel caches, CMake builds, Conan or vcpkg stores, and compiler caches.
- Prefer Gradle, Maven, SDK Manager, AVD Manager, Conan, or vcpkg ownership tools. Android emulator data and local Maven artifacts may be unique.

## Go, .NET, Ruby, and PHP

- Inspect `go env GOCACHE GOMODCACHE GOPATH`, NuGet global, cache and temp locations, Bundler gems, rbenv or rvm rubies, and Composer cache or vendor trees.
- Build or module caches are usually reproducible; installed runtimes, unpublished packages, and local source overrides may not be.

## Infrastructure tooling

- Inspect `.terraform`, provider or plugin caches, Terragrunt cache, Pulumi plugin downloads, Serverless, CDK or SAM build outputs, local Kubernetes images, and cloud CLI caches.
- Never remove Terraform state, Pulumi state, kubeconfigs, cloud credentials, or secret stores as cache.

## Media production

- Inspect render caches, proxies, optimized media, waveform or thumbnail caches, autosaves, backups, motion templates, and generated deliverables separately from camera originals and project libraries.
- Use Final Cut, Premiere, After Effects, Resolve, Blender, or the relevant asset manager to delete generated media where possible.
- Confirm originals, relinkability, archive policy, and backup before touching project libraries or source media.
