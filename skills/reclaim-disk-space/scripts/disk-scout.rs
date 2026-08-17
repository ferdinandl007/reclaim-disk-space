use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::env;
use std::ffi::{c_char, c_int, c_void, CString, OsStr, OsString};
use std::fs;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const CATEGORY_COUNT: usize = 30;
const TOP_K: usize = 50;
const BATCH_SIZE: usize = 32;
const ABSOLUTE_WORKER_CAP: usize = 16_384;
const VERSION_INDEX_LIMIT_PER_DIRECTORY: usize = 50_000;
const VERSION_BUCKET_LIMIT: usize = 16;
const MAX_EXTENSION_KEYS: usize = 4096;
const MAX_HARDLINK_KEYS: usize = 1_000_000;
const MAX_DIRECTORY_RECORDS: usize = 1_000_000;
const MAX_CHILDREN_PER_DIRECTORY: usize = 200_000;
const VERSION_CLUSTER_DIRECTORY_TOP_K: usize = 20;
const VERSION_CLUSTER_TOP_K: usize = 100;
const ENVIRONMENT_TOP_K: usize = 200;
const PROJECT_TOP_K: usize = 200;
const STALE_REVIEW_DAYS: u64 = 180;

const CATEGORY_NAMES: [&str; CATEGORY_COUNT] = [
    "ai_model_weights",
    "ai_datasets",
    "python_env_dependencies",
    "python_source_notebooks",
    "javascript_dependencies",
    "javascript_typescript_source",
    "package_manager_caches",
    "xcode_simulators_builds",
    "ios_macos_source",
    "docker_vm_images",
    "git_vcs_data",
    "generic_build_artifacts",
    "databases_indexes",
    "logs_crash_diagnostics",
    "messaging_media",
    "browser_editor_caches",
    "archives_installers",
    "documents_text_config",
    "other_media",
    "other",
    "rust_cargo",
    "jvm_gradle_maven",
    "go_modules_build",
    "dotnet_nuget",
    "ruby_php_dependencies",
    "android_mobile_build",
    "native_cpp_build",
    "game_engine_assets_build",
    "media_production_cache",
    "infrastructure_cloud",
];

const AI_MODEL: usize = 0;
const AI_DATASET: usize = 1;
const PYTHON_DEPS: usize = 2;
const PYTHON_SOURCE: usize = 3;
const JS_DEPS: usize = 4;
const JS_SOURCE: usize = 5;
const PACKAGE_CACHE: usize = 6;
const XCODE_BUILD: usize = 7;
const IOS_SOURCE: usize = 8;
const DOCKER_VM: usize = 9;
const GIT_DATA: usize = 10;
const BUILD_ARTIFACT: usize = 11;
const DATABASE_INDEX: usize = 12;
const LOG_CRASH: usize = 13;
const MESSAGE_MEDIA: usize = 14;
const BROWSER_CACHE: usize = 15;
const ARCHIVE_INSTALLER: usize = 16;
const DOCUMENT_TEXT: usize = 17;
const OTHER_MEDIA: usize = 18;
const OTHER: usize = 19;
const RUST_CARGO: usize = 20;
const JVM_BUILD: usize = 21;
const GO_BUILD: usize = 22;
const DOTNET_BUILD: usize = 23;
const RUBY_PHP: usize = 24;
const ANDROID_BUILD: usize = 25;
const NATIVE_CPP: usize = 26;
const GAME_ENGINE: usize = 27;
const MEDIA_PRODUCTION: usize = 28;
const INFRA_CLOUD: usize = 29;

const CTX_AI: u32 = 1 << 0;
const CTX_PYTHON_DEPS: u32 = 1 << 1;
const CTX_JS_DEPS: u32 = 1 << 2;
const CTX_PACKAGE_CACHE: u32 = 1 << 3;
const CTX_XCODE: u32 = 1 << 4;
const CTX_DOCKER: u32 = 1 << 5;
const CTX_GIT: u32 = 1 << 6;
const CTX_BUILD: u32 = 1 << 7;
const CTX_BROWSER: u32 = 1 << 8;
const CTX_WHATSAPP: u32 = 1 << 9;
const CTX_MESSAGE_MEDIA: u32 = 1 << 10;
const CTX_LOGS: u32 = 1 << 11;
const CTX_RUST: u32 = 1 << 12;
const CTX_JVM: u32 = 1 << 13;
const CTX_GO: u32 = 1 << 14;
const CTX_DOTNET: u32 = 1 << 15;
const CTX_RUBY_PHP: u32 = 1 << 16;
const CTX_ANDROID: u32 = 1 << 17;
const CTX_NATIVE: u32 = 1 << 18;
const CTX_GAME: u32 = 1 << 19;
const CTX_MEDIA_PRODUCTION: u32 = 1 << 20;
const CTX_INFRA: u32 = 1 << 21;
const CTX_CONDA: u32 = 1 << 22;
const CTX_PROJECT_TREE: u32 = 1 << 23;

#[derive(Clone, Copy, Default)]
struct Metrics {
    logical: u64,
    physical: u64,
    private: u64,
    files: u64,
    tiny: u64,
    small: u64,
    small_text: u64,
    newest_modified_seconds: u64,
    newest_source_modified_seconds: u64,
    newest_generated_modified_seconds: u64,
    source_files: u64,
    generated_files: u64,
}

impl Metrics {
    fn add_assign(&mut self, other: Metrics) {
        self.logical = self.logical.saturating_add(other.logical);
        self.physical = self.physical.saturating_add(other.physical);
        self.private = self.private.saturating_add(other.private);
        self.files = self.files.saturating_add(other.files);
        self.tiny = self.tiny.saturating_add(other.tiny);
        self.small = self.small.saturating_add(other.small);
        self.small_text = self.small_text.saturating_add(other.small_text);
        self.newest_modified_seconds = self.newest_modified_seconds.max(other.newest_modified_seconds);
        self.newest_source_modified_seconds = self.newest_source_modified_seconds.max(other.newest_source_modified_seconds);
        self.newest_generated_modified_seconds = self.newest_generated_modified_seconds.max(other.newest_generated_modified_seconds);
        self.source_files = self.source_files.saturating_add(other.source_files);
        self.generated_files = self.generated_files.saturating_add(other.generated_files);
    }
}

#[derive(Clone, Copy, Default)]
struct CategoryMetric {
    logical: u64,
    physical: u64,
    private: u64,
    files: u64,
    tiny: u64,
    small: u64,
    small_text: u64,
}

#[derive(Default)]
struct CategoryTotals {
    values: [CategoryMetric; CATEGORY_COUNT],
    extensions: HashMap<String, CategoryMetric>,
}

impl CategoryTotals {
    fn add_metric(metric: &mut CategoryMetric, logical: u64, physical: u64, private: u64, text_like: bool) {
        metric.logical = metric.logical.saturating_add(logical);
        metric.physical = metric.physical.saturating_add(physical);
        metric.private = metric.private.saturating_add(private);
        metric.files += 1;
        if logical <= 4096 { metric.tiny += 1; }
        if logical <= 65536 { metric.small += 1; }
        if logical <= 65536 && text_like { metric.small_text += 1; }
    }

    fn add_file(&mut self, category: usize, extension: String, logical: u64, physical: u64, private: u64, text_like: bool) {
        let metric = &mut self.values[category];
        Self::add_metric(metric, logical, physical, private, text_like);
        let key = if self.extensions.contains_key(&extension) || self.extensions.len() < MAX_EXTENSION_KEYS {
            extension
        } else {
            "<other>".to_string()
        };
        let extension_metric = self.extensions.entry(key).or_default();
        Self::add_metric(extension_metric, logical, physical, private, text_like);
    }

    fn merge(&mut self, other: CategoryTotals) {
        for index in 0..CATEGORY_COUNT {
            self.values[index].logical = self.values[index].logical.saturating_add(other.values[index].logical);
            self.values[index].physical = self.values[index].physical.saturating_add(other.values[index].physical);
            self.values[index].private = self.values[index].private.saturating_add(other.values[index].private);
            self.values[index].files = self.values[index].files.saturating_add(other.values[index].files);
            self.values[index].tiny = self.values[index].tiny.saturating_add(other.values[index].tiny);
            self.values[index].small = self.values[index].small.saturating_add(other.values[index].small);
            self.values[index].small_text = self.values[index].small_text.saturating_add(other.values[index].small_text);
        }
        for (extension, values) in other.extensions {
            let key = if self.extensions.contains_key(&extension) || self.extensions.len() < MAX_EXTENSION_KEYS {
                extension
            } else {
                "<other>".to_string()
            };
            let metric = self.extensions.entry(key).or_default();
            metric.logical = metric.logical.saturating_add(values.logical);
            metric.physical = metric.physical.saturating_add(values.physical);
            metric.private = metric.private.saturating_add(values.private);
            metric.files = metric.files.saturating_add(values.files);
            metric.tiny = metric.tiny.saturating_add(values.tiny);
            metric.small = metric.small.saturating_add(values.small);
            metric.small_text = metric.small_text.saturating_add(values.small_text);
        }
    }
}

struct DirectoryRecord {
    name: OsString,
    parent: Option<u32>,
    context: u32,
    direct: Metrics,
    total: Metrics,
    environment_kind: Option<&'static str>,
    project_kind: Option<&'static str>,
    git_evidence: Option<GitEvidence>,
}

struct Task {
    id: u32,
    path: PathBuf,
    context: u32,
    directory_fd: Option<OwnedFd>,
}

struct ScanResult {
    id: u32,
    direct: Metrics,
    children: Vec<(PathBuf, u32, Option<OwnedFd>)>,
    version_clusters: Vec<VersionCluster>,
    version_cluster_count: u64,
    version_candidates: u64,
    version_candidates_skipped: u64,
    project_kind: Option<&'static str>,
    environment_kind: Option<&'static str>,
    timestamp_queries: u64,
    timestamp_failures: u64,
    git_evidence: Option<GitEvidence>,
    errors: Vec<(PathBuf, String)>,
    backend: &'static str,
    complete: bool,
    mounts_skipped: u64,
    entries_seen: u64,
}

struct SharedState {
    queue: VecDeque<Task>,
    records: Vec<DirectoryRecord>,
    active: usize,
    done: bool,
    permission_errors: u64,
    mounts_skipped: u64,
    error_paths: Vec<(PathBuf, String)>,
    native_directories: u64,
    fallback_directories: u64,
    partial_directories: u64,
    version_clusters: Vec<VersionCluster>,
    version_cluster_count: u64,
    version_candidates: u64,
    version_candidates_skipped: u64,
    timestamp_queries: u64,
    timestamp_failures: u64,
}

#[derive(Clone)]
struct VersionCandidate {
    path: PathBuf,
    logical: u64,
    physical: u64,
    private: u64,
    created_seconds: u64,
    modified_seconds: u64,
    version_rank: i32,
    has_version_signal: bool,
}

struct VersionCluster {
    key: String,
    members: Vec<VersionCandidate>,
    confidence: &'static str,
    reason: String,
    review_reclaim_private: u64,
    review_reclaim_physical: u64,
    suggested_keep: usize,
    created_span_days: u64,
    modified_span_days: u64,
    evidence_quality: &'static str,
}

#[derive(Clone)]
struct GitEvidence {
    branch: String,
    head_ref: String,
    head_oid: String,
    common_git_dir: String,
    worktree_state: &'static str,
    ref_activity_seconds: u64,
    index_modified_seconds: u64,
    metadata_bytes: u64,
    worktree_count: u64,
    locked_worktree_count: u64,
    prunable_worktree_count: u64,
    remote_count: u64,
    submodule_count: u64,
    index_entries: u64,
    modified_tracked_files: u64,
    deleted_tracked_files: u64,
}

struct HardlinkSet {
    shards: Vec<Mutex<HashSet<(u64, u64)>>>,
    duplicates: AtomicU64,
    tracked: AtomicUsize,
    saturated: AtomicBool,
}

#[repr(align(128))]
#[derive(Default)]
struct WorkerTelemetry {
    entries: AtomicU64,
    directories: AtomicU64,
    scan_nanos: AtomicU64,
}

struct AutoTuner {
    current: usize,
    best: usize,
    peak: usize,
    max: usize,
    probing: bool,
    baseline_rate: f64,
    best_observed_rate: f64,
    cooldown_samples: u8,
    probes: u32,
    accepted_probes: u32,
    rejected_probes: u32,
    last_entries: u64,
    last_sample: Instant,
    probe_samples: u8,
    probe_rate_sum: f64,
    cpu_budget_cores: f64,
    load_budget: f64,
    last_cpu_seconds: f64,
    peak_cpu_cores: f64,
    peak_system_load: f64,
}

impl AutoTuner {
    fn new(initial: usize, max: usize, now: Instant, cpu_budget_cores: f64, load_budget: f64, cpu_seconds: f64) -> Self {
        Self {
            current: initial,
            best: initial,
            peak: initial,
            max,
            probing: false,
            baseline_rate: 0.0,
            best_observed_rate: 0.0,
            cooldown_samples: 0,
            probes: 0,
            accepted_probes: 0,
            rejected_probes: 0,
            last_entries: 0,
            last_sample: now,
            probe_samples: 0,
            probe_rate_sum: 0.0,
            cpu_budget_cores,
            load_budget,
            last_cpu_seconds: cpu_seconds,
            peak_cpu_cores: 0.0,
            peak_system_load: 0.0,
        }
    }

    fn observe(&mut self, now: Instant, total_entries: u64, backlog: usize, cpu_seconds: f64, system_load: f64) -> Option<usize> {
        let elapsed = now.duration_since(self.last_sample).as_secs_f64();
        if elapsed < 1.0 { return None; }
        let rate = total_entries.saturating_sub(self.last_entries) as f64 / elapsed;
        let cpu_cores = (cpu_seconds - self.last_cpu_seconds).max(0.0) / elapsed;
        self.last_cpu_seconds = cpu_seconds;
        self.peak_cpu_cores = self.peak_cpu_cores.max(cpu_cores);
        self.peak_system_load = self.peak_system_load.max(system_load);
        self.last_entries = total_entries;
        self.last_sample = now;
        self.best_observed_rate = self.best_observed_rate.max(rate);

        let cpu_ratio = if self.cpu_budget_cores.is_finite() { cpu_cores / self.cpu_budget_cores.max(0.1) } else { 0.0 };
        let load_ratio = if self.load_budget.is_finite() { system_load / self.load_budget.max(0.1) } else { 0.0 };
        let pressure = cpu_ratio.max(load_ratio);
        if pressure > 1.05 && self.current > 1 {
            self.current = ((self.current as f64 / pressure) * 0.80).floor().max(1.0) as usize;
            self.best = self.best.min(self.current);
            self.probing = false;
            self.cooldown_samples = 4;
            return Some(self.current);
        }

        let mut responsive_max = self.max;
        if self.cpu_budget_cores.is_finite() && cpu_cores > 0.05 {
            let projected = ((self.current as f64 * self.cpu_budget_cores / cpu_cores) * 0.90)
                .floor()
                .max(self.current as f64) as usize;
            responsive_max = responsive_max.min(projected);
        }
        if self.load_budget.is_finite() && system_load > 0.05 {
            let projected = ((self.current as f64 * self.load_budget / system_load) * 0.90)
                .floor()
                .max(self.current as f64) as usize;
            responsive_max = responsive_max.min(projected);
        }

        if backlog < self.current.saturating_mul(4) || rate == 0.0 {
            return None;
        }

        if self.probing {
            self.probe_samples += 1;
            self.probe_rate_sum += rate;
            if self.probe_samples < 2 { return None; }
            self.probes += 1;
            let probe_rate = self.probe_rate_sum / self.probe_samples as f64;
            if probe_rate >= self.baseline_rate * 1.03 {
                self.best = self.current;
                self.baseline_rate = probe_rate;
                self.cooldown_samples = 1;
                self.accepted_probes += 1;
            } else {
                self.current = self.best;
                self.cooldown_samples = 4;
                self.rejected_probes += 1;
            }
            self.probing = false;
            return Some(self.current);
        }

        if self.cooldown_samples > 0 {
            self.cooldown_samples -= 1;
            return None;
        }

        if self.current < responsive_max {
            self.best = self.current;
            self.baseline_rate = rate;
            self.current = if self.current < 64 {
                self.current.saturating_mul(2).min(responsive_max)
            } else {
                (self.current + self.current / 2).min(responsive_max)
            };
            self.peak = self.peak.max(self.current);
            self.probing = true;
            self.probe_samples = 0;
            self.probe_rate_sum = 0.0;
            return Some(self.current);
        }
        None
    }
}

impl HardlinkSet {
    fn new() -> Self {
        Self {
            shards: (0..64).map(|_| Mutex::new(HashSet::new())).collect(),
            duplicates: AtomicU64::new(0),
            tracked: AtomicUsize::new(0),
            saturated: AtomicBool::new(false),
        }
    }

    fn is_first_parts(&self, device: u64, file_id: u64, link_count: u32) -> bool {
        if link_count <= 1 { return true; }
        let key = (device, file_id);
        let shard = ((key.0 ^ key.1) as usize) & (self.shards.len() - 1);
        let mut seen = self.shards[shard].lock().unwrap();
        if seen.contains(&key) {
            self.duplicates.fetch_add(1, Ordering::Relaxed);
            false
        } else if self.tracked.load(Ordering::Relaxed) >= MAX_HARDLINK_KEYS {
            self.saturated.store(true, Ordering::Relaxed);
            true
        } else if seen.insert(key) {
            self.tracked.fetch_add(1, Ordering::Relaxed);
            true
        } else {
            self.duplicates.fetch_add(1, Ordering::Relaxed);
            false
        }
    }

    fn saturated(&self) -> bool {
        self.saturated.load(Ordering::Relaxed)
    }
}

const VREG: u32 = 1;
const VDIR: u32 = 2;
const NATIVE_NAME_CAPACITY: usize = 1024;

#[repr(C)]
struct NativeEntry {
    file_id: u64,
    logical_size: u64,
    allocated_size: u64,
    private_size: u64,
    device_id: u64,
    link_count: u32,
    object_type: u32,
    error_code: u32,
    name_length: u32,
    name: [u8; NATIVE_NAME_CAPACITY],
}

impl Default for NativeEntry {
    fn default() -> Self {
        Self {
            file_id: 0,
            logical_size: 0,
            allocated_size: 0,
            private_size: 0,
            device_id: 0,
            link_count: 0,
            object_type: 0,
            error_code: 0,
            name_length: 0,
            name: [0; NATIVE_NAME_CAPACITY],
        }
    }
}

unsafe extern "C" {
    fn ds_scanner_create() -> *mut c_void;
    fn ds_recommended_fd_queue_limit() -> usize;
    fn ds_recommended_worker_limit() -> usize;
    fn ds_logical_cpu_count() -> u32;
    fn ds_process_cpu_seconds() -> f64;
    fn ds_host_cpu_busy_fraction() -> f64;
    fn ds_set_interactive_priority() -> i32;
    fn ds_scanner_open(directory: *mut c_void, path: *const c_char) -> c_int;
    fn ds_scanner_adopt_fd(directory: *mut c_void, fd: c_int) -> c_int;
    fn ds_scanner_open_child(directory: *mut c_void, name: *const c_char) -> c_int;
    fn ds_scanner_child_times(
        directory: *mut c_void,
        name: *const c_char,
        created_seconds: *mut u64,
        modified_seconds: *mut u64,
    ) -> c_int;
    fn ds_next_entry(directory: *mut c_void, output: *mut NativeEntry) -> c_int;
    fn ds_last_errno(directory: *mut c_void) -> c_int;
    fn ds_scanner_close(directory: *mut c_void);
    fn ds_scanner_destroy(directory: *mut c_void);
}

struct NativeScanner {
    handle: *mut c_void,
}

impl NativeScanner {
    fn new() -> Self {
        Self { handle: unsafe { ds_scanner_create() } }
    }

    fn open(&mut self, path: &Path) -> bool {
        if self.handle.is_null() { return false; }
        let path = match CString::new(path.as_os_str().as_bytes()) {
            Ok(value) => value,
            Err(_) => return false,
        };
        unsafe { ds_scanner_open(self.handle, path.as_ptr()) == 0 }
    }

    fn adopt(&mut self, fd: OwnedFd) -> bool {
        if self.handle.is_null() { return false; }
        let raw_fd = fd.into_raw_fd();
        if unsafe { ds_scanner_adopt_fd(self.handle, raw_fd) } == 0 {
            true
        } else {
            unsafe { libc_close(raw_fd); }
            false
        }
    }

    fn open_child(&self, name: &OsStr) -> Option<OwnedFd> {
        if self.handle.is_null() { return None; }
        let name = CString::new(name.as_bytes()).ok()?;
        let fd = unsafe { ds_scanner_open_child(self.handle, name.as_ptr()) };
        if fd < 0 { None } else { Some(unsafe { OwnedFd::from_raw_fd(fd) }) }
    }

    fn child_times(&self, name: &OsStr) -> Option<(u64, u64)> {
        if self.handle.is_null() { return None; }
        let name = match CString::new(name.as_bytes()) {
            Ok(value) => value,
            Err(_) => return None,
        };
        let mut created = 0u64;
        let mut modified = 0u64;
        let result = unsafe {
            ds_scanner_child_times(
                self.handle,
                name.as_ptr(),
                &mut created,
                &mut modified,
            )
        };
        if result == 0 { Some((created, modified)) } else { None }
    }

    fn close(&mut self) {
        if !self.handle.is_null() { unsafe { ds_scanner_close(self.handle); } }
    }
}

unsafe extern "C" {
    #[link_name = "close"]
    fn libc_close(fd: c_int) -> c_int;
}

impl Drop for NativeScanner {
    fn drop(&mut self) {
        if !self.handle.is_null() { unsafe { ds_scanner_destroy(self.handle); } }
    }
}

fn telemetry_totals(values: &[WorkerTelemetry]) -> (u64, u64, u64) {
    values.iter().fold((0, 0, 0), |totals, value| {
        (
            totals.0.saturating_add(value.entries.load(Ordering::Relaxed)),
            totals.1.saturating_add(value.directories.load(Ordering::Relaxed)),
            totals.2.saturating_add(value.scan_nanos.load(Ordering::Relaxed)),
        )
    })
}

fn lower_name(value: &OsStr) -> String {
    value.to_string_lossy().to_ascii_lowercase()
}

fn derive_context(parent: u32, name: &OsStr) -> u32 {
    let value = lower_name(name);
    let mut context = parent;

    if value.contains("whatsapp") { context |= CTX_WHATSAPP; }
    if context & CTX_WHATSAPP != 0 && value == "media" { context |= CTX_MESSAGE_MEDIA; }

    if matches!(value.as_str(), "huggingface" | "transformers" | "diffusers" | "ollama" | "lm-studio" | "models" | "model" | "checkpoints") {
        context |= CTX_AI;
    }
    if matches!(value.as_str(), ".venv" | "venv" | "virtualenv" | "site-packages" | "__pycache__" | ".tox" | ".nox") {
        context |= CTX_PYTHON_DEPS;
    }
    if matches!(value.as_str(), ".conda" | "conda" | "conda3" | "miniconda" | "miniconda3" | "anaconda" | "anaconda3") {
        context |= CTX_CONDA | CTX_PYTHON_DEPS;
    }
    if value == "node_modules" { context |= CTX_JS_DEPS; }
    if matches!(value.as_str(), ".npm" | ".pnpm-store" | "pnpm" | "yarn" | "pip" | "cocoapods" | "homebrew" | "archive-v0" | "wheels-v5" | "simple-v18") {
        context |= CTX_PACKAGE_CACHE;
    }
    if matches!(value.as_str(), "deriveddata" | "coresimulator" | "device support" | "ios devicesupport" | "archives") || value.ends_with(".xcarchive") || value.ends_with(".dsym") {
        context |= CTX_XCODE;
    }
    if value.contains("docker") || value.contains("colima") || value == "lima" || value == "_lima" {
        context |= CTX_DOCKER;
    }
    if value == ".git" || value == "objects" && parent & CTX_GIT != 0 || value == "lfs" && parent & CTX_GIT != 0 {
        context |= CTX_GIT;
    }
    if matches!(value.as_str(), "build" | "dist" | "target" | ".next" | ".turbo" | ".gradle" | ".m2" | "bazel-out" | ".pytest_cache" | ".mypy_cache" | ".ruff_cache") {
        context |= CTX_BUILD;
    }
    if matches!(value.as_str(), "cursor" | "chrome" | "chromium" | "code cache" | "gpucache" | "cacheddata" | "service worker" | "indexeddb" | "webstorage") {
        context |= CTX_BROWSER;
    }
    if matches!(value.as_str(), "logs" | "diagnosticreports" | "crashpad" | "crashes") {
        context |= CTX_LOGS;
    }
    if matches!(value.as_str(), ".cargo" | "cargo" | "rustup" | ".rustup" | "target")
        || parent & CTX_RUST != 0 && matches!(value.as_str(), "registry" | "git" | "target")
    {
        context |= CTX_RUST;
    }
    if matches!(value.as_str(), ".gradle" | "gradle" | ".m2" | "maven" | ".ivy2" | "ivy2")
        || parent & CTX_JVM != 0 && matches!(value.as_str(), "caches" | "repository" | "wrapper")
    {
        context |= CTX_JVM;
    }
    if matches!(value.as_str(), "go-build" | "gomodcache" | "gopath")
        || parent & CTX_GO != 0 && matches!(value.as_str(), "pkg" | "mod")
    {
        context |= CTX_GO;
    }
    if matches!(value.as_str(), ".nuget" | "nuget")
        || parent & CTX_DOTNET != 0 && matches!(value.as_str(), "packages" | "bin" | "obj")
    {
        context |= CTX_DOTNET;
    }
    if matches!(value.as_str(), ".bundle" | "gems" | "rubies" | ".composer" | "composer")
        || parent & CTX_RUBY_PHP != 0 && value == "vendor"
    {
        context |= CTX_RUBY_PHP;
    }
    if matches!(value.as_str(), ".android" | "android" | "android sdk" | "androidstudio" | "avd")
        || value.ends_with(".avd")
    {
        context |= CTX_ANDROID;
    }
    if matches!(value.as_str(), "cmakefiles" | ".conan" | "conan" | ".vcpkg" | "vcpkg") {
        context |= CTX_NATIVE;
    }
    if matches!(value.as_str(), "unity" | "unrealengine" | "unreal engine" | "deriveddatacache")
        || value.ends_with(".uproject") || value.ends_with(".unitypackage")
    {
        context |= CTX_GAME;
    }
    if matches!(value.as_str(), "final cut backups.localized" | "motion templates.localized" | "adobe" | "premiere pro" | "after effects" | "davinci resolve" | "render cache" | "optimized media" | "proxy media" | "blender")
        || value.ends_with(".fcpbundle") || value.ends_with(".fcpproject") || value.ends_with(".drp")
    {
        context |= CTX_MEDIA_PRODUCTION;
    }
    if matches!(value.as_str(), ".terraform" | ".terragrunt-cache" | "terraform.d" | ".pulumi" | "pulumi" | ".serverless" | ".aws-sam" | "cdk.out") {
        context |= CTX_INFRA;
    }
    context
}

fn extension(name: &OsStr) -> String {
    Path::new(name)
        .extension()
        .map(lower_name)
        .unwrap_or_default()
}

fn dynamic_extension(name: &OsStr) -> String {
    let value = extension(name);
    if value.is_empty() { return "<none>".to_string(); }
    if value.len() > 64 { return "<long-or-nonstandard>".to_string(); }
    value
        .chars()
        .map(|character| if character.is_control() { '?' } else { character })
        .collect()
}

fn classify_file(context: u32, name: &OsStr) -> (usize, bool) {
    let file_name = lower_name(name);
    let ext = extension(name);

    if context & CTX_MESSAGE_MEDIA != 0 { return (MESSAGE_MEDIA, false); }
    if matches!(ext.as_str(), "safetensors" | "gguf" | "ggml" | "onnx" | "tflite" | "mlmodel" | "mlpackage" | "pt" | "pth" | "ckpt" | "coreml" | "engine" | "weights" | "params")
        || context & CTX_AI != 0 && matches!(ext.as_str(), "bin" | "ot")
    {
        return (AI_MODEL, false);
    }
    if matches!(ext.as_str(), "parquet" | "arrow" | "jsonl" | "npy" | "npz" | "h5" | "hdf5" | "tfrecord" | "feather") {
        return (AI_DATASET, false);
    }
    if context & CTX_DOCKER != 0 || matches!(ext.as_str(), "qcow2" | "vmdk" | "vdi" | "raw") { return (DOCKER_VM, false); }
    if context & CTX_GIT != 0 { return (GIT_DATA, false); }
    if context & CTX_XCODE != 0 { return (XCODE_BUILD, false); }
    if context & CTX_PACKAGE_CACHE != 0 { return (PACKAGE_CACHE, false); }
    if context & CTX_JS_DEPS != 0 { return (JS_DEPS, false); }
    if context & CTX_PYTHON_DEPS != 0 { return (PYTHON_DEPS, false); }
    if context & CTX_RUST != 0 { return (RUST_CARGO, false); }
    if context & CTX_JVM != 0 { return (JVM_BUILD, false); }
    if context & CTX_GO != 0 { return (GO_BUILD, false); }
    if context & CTX_DOTNET != 0 { return (DOTNET_BUILD, false); }
    if context & CTX_RUBY_PHP != 0 { return (RUBY_PHP, false); }
    if context & CTX_ANDROID != 0 { return (ANDROID_BUILD, false); }
    if context & CTX_NATIVE != 0 { return (NATIVE_CPP, false); }
    if context & CTX_GAME != 0 { return (GAME_ENGINE, false); }
    if context & CTX_MEDIA_PRODUCTION != 0 { return (MEDIA_PRODUCTION, false); }
    if context & CTX_INFRA != 0 { return (INFRA_CLOUD, true); }
    if context & CTX_BROWSER != 0 { return (BROWSER_CACHE, false); }
    if context & CTX_LOGS != 0 { return (LOG_CRASH, true); }

    if matches!(ext.as_str(), "py" | "pyi" | "ipynb") { return (PYTHON_SOURCE, true); }
    if matches!(ext.as_str(), "pyc" | "pyo" | "pyd" | "whl" | "egg") { return (PYTHON_DEPS, false); }
    if matches!(ext.as_str(), "js" | "jsx" | "ts" | "tsx" | "mjs" | "cjs" | "vue" | "svelte") { return (JS_SOURCE, true); }
    if matches!(ext.as_str(), "swift" | "m" | "mm" | "xib" | "storyboard" | "xcconfig" | "pbxproj" | "entitlements") { return (IOS_SOURCE, true); }
    if ext == "rs" || matches!(file_name.as_str(), "cargo.toml" | "cargo.lock") || matches!(ext.as_str(), "rlib" | "rmeta") { return (RUST_CARGO, ext == "rs" || file_name == "cargo.toml"); }
    if matches!(ext.as_str(), "java" | "kt" | "kts" | "scala" | "class" | "jar" | "war") || matches!(file_name.as_str(), "pom.xml" | "build.gradle" | "build.gradle.kts") { return (JVM_BUILD, matches!(ext.as_str(), "java" | "kt" | "kts" | "scala")); }
    if ext == "go" || matches!(file_name.as_str(), "go.mod" | "go.sum" | "go.work") { return (GO_BUILD, true); }
    if matches!(ext.as_str(), "cs" | "fs" | "fsx" | "vb" | "dll" | "pdb" | "nupkg") || matches!(file_name.as_str(), "packages.lock.json" | "global.json") { return (DOTNET_BUILD, matches!(ext.as_str(), "cs" | "fs" | "fsx" | "vb")); }
    if matches!(ext.as_str(), "rb" | "gem" | "php" | "phar") || matches!(file_name.as_str(), "gemfile" | "gemfile.lock" | "composer.json" | "composer.lock") { return (RUBY_PHP, matches!(ext.as_str(), "rb" | "php")); }
    if matches!(ext.as_str(), "apk" | "aab" | "aar" | "dex") || file_name == "androidmanifest.xml" { return (ANDROID_BUILD, false); }
    if matches!(ext.as_str(), "c" | "cc" | "cpp" | "cxx" | "h" | "hh" | "hpp" | "hxx" | "cmake") || file_name == "cmakelists.txt" { return (NATIVE_CPP, true); }
    if matches!(ext.as_str(), "uasset" | "umap" | "unity" | "prefab" | "asset" | "fbx" | "obj" | "glb" | "gltf") { return (GAME_ENGINE, false); }
    if matches!(ext.as_str(), "fcpbundle" | "fcpproject" | "prproj" | "aep" | "aepx" | "drp" | "dra" | "blend" | "braw" | "r3d" | "mxf" | "exr" | "dpx" | "psd" | "ai") { return (MEDIA_PRODUCTION, false); }
    if matches!(ext.as_str(), "tf" | "tfvars" | "hcl") || matches!(file_name.as_str(), "pulumi.yaml" | "serverless.yml" | "template.yaml") { return (INFRA_CLOUD, true); }
    if context & CTX_BUILD != 0 || matches!(ext.as_str(), "o" | "a" | "dylib" | "class" | "jar" | "wasm" | "map") { return (BUILD_ARTIFACT, false); }
    if matches!(ext.as_str(), "db" | "sqlite" | "sqlite3" | "wal" | "shm" | "ldb" | "mdb" | "index" | "idx") || file_name.contains("leveldb") { return (DATABASE_INDEX, false); }
    if matches!(ext.as_str(), "log" | "crash" | "trace" | "ips" | "diag" | "xcresult") { return (LOG_CRASH, true); }
    if matches!(ext.as_str(), "zip" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "7z" | "rar" | "dmg" | "pkg" | "ipa" | "iso") { return (ARCHIVE_INSTALLER, false); }
    if matches!(ext.as_str(), "txt" | "md" | "rst" | "json" | "yaml" | "yml" | "toml" | "xml" | "plist" | "ini" | "conf" | "cfg" | "lock" | "csv" | "tsv" | "sql" | "sh" | "zsh" | "bash" | "fish" | "css" | "scss" | "html") {
        return (DOCUMENT_TEXT, true);
    }
    if matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "tiff" | "svg" | "mp4" | "mov" | "mkv" | "avi" | "webm" | "mp3" | "wav" | "m4a" | "flac" | "aac" | "pdf") {
        return (OTHER_MEDIA, false);
    }
    (OTHER, false)
}

fn generated_context(context: u32) -> bool {
    context & (CTX_AI | CTX_PYTHON_DEPS | CTX_JS_DEPS | CTX_PACKAGE_CACHE | CTX_XCODE | CTX_DOCKER | CTX_GIT | CTX_BUILD | CTX_BROWSER | CTX_LOGS | CTX_RUST | CTX_JVM | CTX_GO | CTX_DOTNET | CTX_RUBY_PHP | CTX_ANDROID | CTX_NATIVE | CTX_INFRA) != 0
}

fn activity_timestamp_needed(context: u32, name: &OsStr, version_candidate: bool) -> bool {
    let lower = lower_name(name);
    if version_candidate || project_marker_kind(name).is_some() || matches!(lower.as_str(), "pyvenv.cfg" | "conda-meta") {
        return true;
    }
    if generated_context(context) || context & CTX_PROJECT_TREE == 0 { return false; }
    let (category, text_like) = classify_file(context, name);
    text_like || matches!(category, OTHER_MEDIA | MEDIA_PRODUCTION | IOS_SOURCE | GAME_ENGINE)
}

fn source_activity_candidate(context: u32, name: &OsStr) -> bool {
    if generated_context(context) { return false; }
    let (category, text_like) = classify_file(context, name);
    text_like || matches!(category, OTHER_MEDIA | MEDIA_PRODUCTION | IOS_SOURCE | GAME_ENGINE)
}

fn version_cluster_key(name: &OsStr) -> Option<(String, i32, bool)> {
    let lower = lower_name(name);
    let extension = extension(name);
    let mut stem = Path::new(&lower).file_stem()?.to_string_lossy().to_string();
    let mut rank = 0i32;
    let mut signalled = false;

    for marker in [" copy", " duplicate", " final", " export", " exported", " edited", " edit", " backup", " old", " new"] {
        if let Some(base) = stem.strip_suffix(marker) {
            stem = base.trim_end_matches([' ', '_', '-']).to_string();
            rank = 1;
            signalled = true;
            break;
        }
    }
    if !signalled {
        if let Some(open) = stem.rfind('(') {
            let parenthetical_rank = stem[open + 1..].strip_suffix(')').and_then(|value| value.parse::<u16>().ok()).filter(|value| *value <= 999);
            if let Some(parenthetical_rank) = parenthetical_rank {
                stem = stem[..open].trim_end_matches([' ', '_', '-']).to_string();
                rank = parenthetical_rank as i32;
                signalled = true;
            }
        }
    }
    if !signalled {
        let bytes = stem.as_bytes();
        let mut split = bytes.len();
        while split > 0 && bytes[split - 1].is_ascii_digit() { split -= 1; }
        let numeric_rank = stem[split..].parse::<u16>().ok().filter(|value| *value <= 999);
        if split < bytes.len() && numeric_rank.is_some() {
            let delimiter = split > 0 && matches!(bytes[split - 1], b' ' | b'_' | b'-');
            if delimiter {
                stem = stem[..split - 1].trim_end_matches([' ', '_', '-']).to_string();
                rank = numeric_rank.unwrap_or(1) as i32;
                signalled = true;
            }
        }
    }
    if signalled {
        for marker in [" copy", " duplicate", " final", " export", " exported", " edited", " edit", " backup", " old", " new"] {
            if let Some(base) = stem.strip_suffix(marker) {
                stem = base.trim_end_matches([' ', '_', '-']).to_string();
                if rank == 0 { rank = 1; }
                break;
            }
        }
    }
    if !signalled {
        let compact = stem.to_ascii_lowercase();
        if let Some(position) = compact.rfind("version") {
            let suffix = compact[position + "version".len()..].trim();
            if suffix.parse::<u16>().ok().filter(|value| *value <= 999).is_some() {
                stem = stem[..position].trim_end_matches([' ', '_', '-']).to_string();
                rank = suffix.parse::<i32>().unwrap_or(1);
                signalled = true;
            }
        }
    }
    if !signalled {
        let compact = stem.to_ascii_lowercase();
        if let Some(position) = compact.rfind('v') {
            let suffix = compact[position + 1..].trim();
            let separated = position == 0 || matches!(compact.as_bytes()[position - 1], b' ' | b'_' | b'-');
            if separated && suffix.parse::<u16>().ok().filter(|value| *value <= 999).is_some() {
                stem = stem[..position].trim_end_matches([' ', '_', '-']).to_string();
                rank = suffix.parse::<i32>().unwrap_or(1);
                signalled = true;
            }
        }
    }
    if stem.is_empty() { return None; }
    let key = if extension.is_empty() { stem } else { format!("{stem}.{extension}") };
    Some((key, rank, signalled))
}

fn version_candidate_allowed(context: u32, name: &OsStr, signalled: bool) -> bool {
    if signalled { return true; }
    let ext = extension(name);
    context & (CTX_XCODE | CTX_BUILD | CTX_MEDIA_PRODUCTION | CTX_MESSAGE_MEDIA) != 0
        || matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "gif" | "webp" | "heic" | "tiff" | "svg" | "mp4" | "mov" | "mkv" | "avi" | "webm" | "mp3" | "wav" | "m4a" | "flac" | "aac" | "pdf" | "zip" | "tar" | "gz" | "dmg" | "pkg" | "ipa" | "iso" | "app")
}

fn maybe_collect_version_candidate(
    groups: &mut HashMap<String, Vec<VersionCandidate>>,
    candidate_count: &mut u64,
    skipped_count: &mut u64,
    context: u32,
    name: &OsStr,
    path: PathBuf,
    logical: u64,
    physical: u64,
    private: u64,
    created_seconds: u64,
    modified_seconds: u64,
) {
    let (key, version_rank, signalled) = match version_cluster_key(name) {
        Some(value) => value,
        None => return,
    };
    if !version_candidate_allowed(context, name, signalled) { return; }
    let is_new_key = !groups.contains_key(&key);
    if is_new_key && groups.len() >= VERSION_INDEX_LIMIT_PER_DIRECTORY {
        *skipped_count += 1;
        return;
    }
    let bucket = match groups.entry(key) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::hash_map::Entry::Vacant(entry) => entry.insert(Vec::new()),
    };
    if bucket.len() >= VERSION_BUCKET_LIMIT {
        *skipped_count += 1;
        return;
    }
    bucket.push(VersionCandidate { path, logical, physical, private, created_seconds, modified_seconds, version_rank, has_version_signal: signalled });
    *candidate_count += 1;
}

fn finalize_version_groups(groups: HashMap<String, Vec<VersionCandidate>>) -> (Vec<VersionCluster>, u64) {
    let mut clusters = Vec::new();
    for (key, mut members) in groups {
        if members.len() < 2 || !members.iter().any(|member| member.has_version_signal) { continue; }
        members.sort_unstable_by(|a, b| b.modified_seconds.cmp(&a.modified_seconds).then_with(|| b.version_rank.cmp(&a.version_rank)).then_with(|| b.private.cmp(&a.private)));
        let sizes: Vec<u64> = members.iter().map(|member| member.logical.max(member.physical)).filter(|value| *value > 0).collect();
        let size_evidence = sizes.len() >= 2;
        let max_size = sizes.iter().copied().max().unwrap_or(0);
        let min_size = sizes.iter().copied().min().unwrap_or(0);
        let size_similarity = if !size_evidence { 0.0 } else { min_size as f64 / max_size as f64 };
        let created: Vec<u64> = members.iter().map(|member| member.created_seconds).filter(|value| *value > 0).collect();
        let modified: Vec<u64> = members.iter().map(|member| member.modified_seconds).filter(|value| *value > 0).collect();
        let created_span_days = created.iter().min().zip(created.iter().max()).map(|(min, max)| max.abs_diff(*min) / 86_400).unwrap_or(0);
        let modified_span_days = modified.iter().min().zip(modified.iter().max()).map(|(min, max)| max.abs_diff(*min) / 86_400).unwrap_or(0);
        let close_in_time = (created.len() >= 2 && created_span_days <= 30) || (modified.len() >= 2 && modified_span_days <= 30);
        let signal_count = members.iter().filter(|member| member.has_version_signal).count();
        let confidence = if signal_count >= 2 && (size_similarity >= 0.70 || close_in_time) { "high" }
            else if size_similarity >= 0.45 || close_in_time { "medium" }
            else if signal_count >= 1 { "low" }
            else { continue };
        let evidence_quality = match (size_evidence, created.len() >= 2, modified.len() >= 2) {
            (true, true, true) => "name+size+created+modified",
            (true, true, false) => "name+size+created",
            (true, false, true) => "name+size+modified",
            (true, false, false) => "name+size",
            (false, true, true) => "name+created+modified",
            (false, true, false) => "name+created",
            (false, false, true) => "name+modified",
            (false, false, false) => "name_only",
        };
        let mut reasons = Vec::new();
        if signal_count > 0 { reasons.push("name_variant"); }
        if size_evidence && size_similarity >= 0.70 { reasons.push("size_similarity"); }
        if close_in_time { reasons.push("creation_or_modified_date"); }
        let suggested_keep = 0usize;
        let review_reclaim_private = members.iter().enumerate().filter(|(index, _)| *index != suggested_keep).map(|(_, member)| member.private).sum();
        let review_reclaim_physical = members.iter().enumerate().filter(|(index, _)| *index != suggested_keep).map(|(_, member)| member.physical).sum();
        clusters.push(VersionCluster { key, members, confidence, reason: reasons.join("+"), review_reclaim_private, review_reclaim_physical, suggested_keep, created_span_days, modified_span_days, evidence_quality });
    }
    let count = clusters.len() as u64;
    clusters.sort_unstable_by(|a, b| b.review_reclaim_private.cmp(&a.review_reclaim_private).then_with(|| b.review_reclaim_physical.cmp(&a.review_reclaim_physical)));
    clusters.truncate(VERSION_CLUSTER_DIRECTORY_TOP_K);
    (clusters, count)
}

fn project_marker_kind(name: &OsStr) -> Option<&'static str> {
    let value = lower_name(name);
    if value == ".git" { Some("git_project") }
    else if matches!(value.as_str(), "pyproject.toml" | "setup.py" | "requirements.txt" | "environment.yml" | "environment.yaml") { Some("python_project") }
    else if matches!(value.as_str(), "package.json" | "package-lock.json" | "pnpm-lock.yaml" | "yarn.lock") { Some("javascript_project") }
    else if value == "cargo.toml" { Some("rust_project") }
    else if matches!(value.as_str(), "go.mod" | "go.work") { Some("go_project") }
    else if value.ends_with(".xcodeproj") || value.ends_with(".xcworkspace") || value == "podfile" { Some("ios_project") }
    else if matches!(value.as_str(), "dockerfile" | "docker-compose.yml" | "docker-compose.yaml" | "compose.yml" | "compose.yaml") { Some("docker_project") }
    else if matches!(value.as_str(), "pom.xml" | "build.gradle" | "build.gradle.kts") { Some("jvm_project") }
    else if value == "composer.json" || value == "gemfile" { Some("ruby_php_project") }
    else { None }
}

fn merge_project_kind(current: Option<&'static str>, candidate: Option<&'static str>) -> Option<&'static str> {
    match (current, candidate) {
        (Some(existing), Some(next)) if existing == next => Some(existing),
        (Some(existing), Some(next)) if project_kind_priority(next) > project_kind_priority(existing) => Some(next),
        (Some(existing), Some(_)) => Some(existing),
        (None, value) => value,
        (value, None) => value,
    }
}

fn project_kind_priority(kind: &str) -> u8 {
    match kind {
        "ios_project" => 9,
        "rust_project" | "go_project" | "jvm_project" => 8,
        "python_project" | "javascript_project" => 7,
        "docker_project" => 6,
        "ruby_php_project" => 5,
        "git_project" => 1,
        _ => 0,
    }
}

fn escape_tsv(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\\' => escaped.push_str("\\\\"),
            _ => escaped.push(character),
        }
    }
    escaped.chars().take(2000).collect()
}

fn escape_path(path: &Path) -> String {
    escape_tsv(&path.to_string_lossy())
}

fn safe_git_field(value: &str) -> String {
    escape_tsv(value)
}

fn file_mtime_seconds(path: &Path) -> (u64, u64) {
    match fs::symlink_metadata(path) {
        Ok(metadata) => (metadata.mtime().max(0) as u64, metadata.len()),
        Err(_) => (0, 0),
    }
}

fn read_be_u32(bytes: &[u8], offset: &mut usize) -> Option<u32> {
    if *offset + 4 > bytes.len() { return None; }
    let value = u32::from_be_bytes(bytes[*offset..*offset + 4].try_into().ok()?);
    *offset += 4;
    Some(value)
}

fn inspect_git_index(root: &Path, git_dir: &Path) -> (&'static str, u64, u64, u64) {
    let index_path = git_dir.join("index");
    let size = match fs::symlink_metadata(&index_path) {
        Ok(metadata) if metadata.len() <= 64 * 1024 * 1024 => metadata.len(),
        _ => return ("unknown", 0, 0, 0),
    };
    if size < 12 { return ("unknown", 0, 0, 0); }
    let bytes = match fs::read(&index_path) { Ok(value) => value, Err(_) => return ("unknown", 0, 0, 0) };
    if &bytes[..4] != b"DIRC" { return ("unknown", 0, 0, 0); }
    let mut offset = 4;
    let version = match read_be_u32(&bytes, &mut offset) { Some(value) => value, None => return ("unknown", 0, 0, 0) };
    let entries = match read_be_u32(&bytes, &mut offset) { Some(value) => value as usize, None => return ("unknown", 0, 0, 0) };
    if !(2..=3).contains(&version) || entries > 1_000_000 { return ("unknown", 0, 0, 0); }
    let mut modified = 0;
    let mut deleted = 0;
    for _ in 0..entries {
        if offset + 62 > bytes.len() { return ("unknown", entries as u64, modified, deleted); }
        let ctime = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as i64;
        let ctime_nsec = u32::from_be_bytes(bytes[offset + 4..offset + 8].try_into().unwrap());
        let mtime = u32::from_be_bytes(bytes[offset + 8..offset + 12].try_into().unwrap()) as i64;
        let mtime_nsec = u32::from_be_bytes(bytes[offset + 12..offset + 16].try_into().unwrap());
        let device = u32::from_be_bytes(bytes[offset + 16..offset + 20].try_into().unwrap());
        let inode = u32::from_be_bytes(bytes[offset + 20..offset + 24].try_into().unwrap());
        let size = u32::from_be_bytes(bytes[offset + 36..offset + 40].try_into().unwrap()) as u64;
        let flags = u16::from_be_bytes(bytes[offset + 60..offset + 62].try_into().unwrap());
        let path_start = offset + 62;
        let path_end = match bytes[path_start..].iter().position(|value| *value == 0) {
            Some(value) => path_start + value,
            None => return ("unknown", entries as u64, modified, deleted),
        };
        let path = PathBuf::from(OsStr::from_bytes(&bytes[path_start..path_end]));
        let safe_path = path.is_relative() && path.components().all(|component| !matches!(component, std::path::Component::ParentDir | std::path::Component::RootDir | std::path::Component::Prefix(_)));
        if !safe_path { return ("unknown", entries as u64, modified, deleted); }
        let padded = (path_end + 1 + 7) & !7;
        if padded > bytes.len() { return ("unknown", entries as u64, modified, deleted); }
        offset = padded;
        let metadata = match fs::symlink_metadata(root.join(&path)) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => { deleted += 1; continue; }
            Err(_) => return ("unknown", entries as u64, modified, deleted),
        };
        let changed = metadata.ctime() != ctime || metadata.ctime_nsec() as u32 != ctime_nsec ||
            metadata.mtime() != mtime || metadata.mtime_nsec() as u32 != mtime_nsec ||
            metadata.dev() as u32 != device || metadata.ino() as u32 != inode || metadata.len() != size;
        if changed || (flags & 0x3000) != 0 { modified += 1; }
    }
    let state = if modified > 0 || deleted > 0 { "dirty" } else { "unknown" };
    (state, entries as u64, modified, deleted)
}

fn inspect_git_evidence(root: &Path) -> Option<GitEvidence> {
    let marker = root.join(".git");
    let marker_metadata = fs::symlink_metadata(&marker).ok()?;
    if marker_metadata.file_type().is_symlink() { return None; }
    let git_dir = if marker_metadata.is_dir() {
        marker.clone()
    } else {
        let pointer = fs::read_to_string(&marker).ok()?;
        let value = pointer.trim().strip_prefix("gitdir:")?.trim();
        let path = PathBuf::from(value);
        if path.is_absolute() { path } else { root.join(path) }
    };
    if !git_dir.is_dir() { return None; }

    let head_path = git_dir.join("HEAD");
    let head = fs::read_to_string(&head_path).unwrap_or_default();
    let head_ref = safe_git_field(head.trim());
    let branch = head.trim().strip_prefix("ref: refs/heads/").map(safe_git_field).unwrap_or_else(|| "(detached)".to_string());
    let head_oid = if let Some(reference) = head.trim().strip_prefix("ref: ") {
        let reference_path = Path::new(reference);
        let safe_reference = reference_path.is_relative() && reference_path.components().all(|component| !matches!(component, std::path::Component::ParentDir | std::path::Component::RootDir | std::path::Component::Prefix(_)));
        if safe_reference { fs::read_to_string(git_dir.join(reference_path)).unwrap_or_default() } else { String::new() }
    } else {
        head.clone()
    };
    let head_oid = safe_git_field(head_oid.trim());
    let mut ref_activity_seconds = file_mtime_seconds(&head_path).0;
    let mut metadata_bytes = file_mtime_seconds(&marker).1.saturating_add(file_mtime_seconds(&head_path).1);

    let mut roots = vec![git_dir.clone()];
    if let Ok(common_dir) = fs::read_to_string(git_dir.join("commondir")) {
        let path = PathBuf::from(common_dir.trim());
        let common = if path.is_absolute() { path } else { git_dir.join(path) };
        if common.is_dir() { roots.push(common); }
    }
    let common_git_dir = roots.last().map(|path| escape_path(path)).unwrap_or_default();
    let index_path = git_dir.join("index");
    let index_modified_seconds = file_mtime_seconds(&index_path).0;
    metadata_bytes = metadata_bytes.saturating_add(file_mtime_seconds(&index_path).1);
    let marker_worktree_state = if ["MERGE_HEAD", "CHERRY_PICK_HEAD", "REVERT_HEAD"].iter().any(|name| git_dir.join(name).exists())
        || ["rebase-merge", "rebase-apply", "sequencer"].iter().any(|name| git_dir.join(name).is_dir()) {
        "in_progress"
    } else {
        "unknown"
    };
    let (index_state, index_entries, modified_tracked_files, deleted_tracked_files) = inspect_git_index(root, &git_dir);
    let worktree_state = if marker_worktree_state == "in_progress" { "in_progress" } else { index_state };
    let worktree_root = roots.last().cloned().unwrap_or_else(|| git_dir.clone());
    let mut worktree_count = 0;
    let mut locked_worktree_count = 0;
    let mut prunable_worktree_count = 0;
    if let Ok(entries) = fs::read_dir(worktree_root.join("worktrees")) {
        for entry in entries.flatten() {
            if !entry.path().is_dir() { continue; }
            worktree_count += 1;
            if entry.path().join("locked").exists() { locked_worktree_count += 1; }
            if !entry.path().join("gitdir").exists() { prunable_worktree_count += 1; }
        }
    }
    let remote_count = fs::read_to_string(git_dir.join("config")).unwrap_or_default().lines()
        .filter(|line| line.trim_start().starts_with("[remote \"")).count() as u64;
    let submodule_count = fs::read_to_string(root.join(".gitmodules")).unwrap_or_default().lines()
        .filter(|line| line.trim_start().starts_with("[submodule \"")).count() as u64;
    for root in roots {
        for relative in ["logs/HEAD", "packed-refs"] {
            let path = root.join(relative);
            let (mtime, bytes) = file_mtime_seconds(&path);
            ref_activity_seconds = ref_activity_seconds.max(mtime);
            metadata_bytes = metadata_bytes.saturating_add(bytes);
        }
        for relative in ["logs/refs/heads", "refs/heads", "refs/tags"] {
            if let Ok(entries) = fs::read_dir(root.join(relative)) {
                for entry in entries.flatten() {
                    let (mtime, bytes) = file_mtime_seconds(&entry.path());
                    ref_activity_seconds = ref_activity_seconds.max(mtime);
                    metadata_bytes = metadata_bytes.saturating_add(bytes);
                }
            }
        }
    }
    Some(GitEvidence { branch, head_ref, head_oid, common_git_dir, worktree_state, ref_activity_seconds, index_modified_seconds, metadata_bytes, worktree_count, locked_worktree_count, prunable_worktree_count, remote_count, submodule_count, index_entries, modified_tracked_files, deleted_tracked_files })
}

fn environment_kind_for_child(parent_name: &OsStr, child_name: &OsStr, parent_context: u32) -> Option<&'static str> {
    let child = lower_name(child_name);
    let parent = lower_name(parent_name);
    if matches!(child.as_str(), ".venv" | "venv" | "virtualenv" | ".python") { return Some("python_venv"); }
    if parent == "envs" && parent_context & CTX_CONDA != 0 { return Some("conda_env"); }
    None
}

fn allocated(metadata: &fs::Metadata) -> u64 {
    metadata.blocks().saturating_mul(512)
}

fn add_file_metrics(
    direct: &mut Metrics,
    categories: &mut CategoryTotals,
    hardlinks: &HardlinkSet,
    context: u32,
    name: &OsStr,
    device: u64,
    file_id: u64,
    link_count: u32,
    logical: u64,
    physical: u64,
    private: u64,
    modified_seconds: u64,
) {
    let (category, text_like) = classify_file(context, name);
    let first_link = hardlinks.is_first_parts(device, file_id, link_count);
    let logical_for_space = if first_link { logical } else { 0 };
    let physical_for_space = if first_link { physical } else { 0 };
    let private_for_space = if first_link { private } else { 0 };

    direct.logical = direct.logical.saturating_add(logical_for_space);
    direct.physical = direct.physical.saturating_add(physical_for_space);
    direct.private = direct.private.saturating_add(private_for_space);
    direct.files += 1;
    if logical <= 4096 { direct.tiny += 1; }
    if logical <= 65536 { direct.small += 1; }
    if logical <= 65536 && text_like { direct.small_text += 1; }
    let generated = generated_context(context);
    if generated {
        direct.generated_files += 1;
        direct.newest_generated_modified_seconds = direct.newest_generated_modified_seconds.max(modified_seconds);
    } else {
        direct.source_files += 1;
        direct.newest_source_modified_seconds = direct.newest_source_modified_seconds.max(modified_seconds);
    }
    direct.newest_modified_seconds = direct.newest_modified_seconds.max(modified_seconds);
    categories.add_file(
        category,
        dynamic_extension(name),
        logical_for_space,
        physical_for_space,
        private_for_space,
        text_like,
    );
}

fn scan_directory_native(
    scanner: &mut NativeScanner,
    task: &mut Task,
    root_device: u64,
    hardlinks: &HardlinkSet,
    categories: &mut CategoryTotals,
    queued_fds: &AtomicUsize,
    queued_fd_limit: usize,
) -> Option<ScanResult> {
    let mut direct = Metrics::default();
    let mut children = Vec::new();
    let mut version_groups = HashMap::new();
    let mut version_candidates = 0;
    let mut version_candidates_skipped = 0;
    let mut project_kind = None;
    let mut environment_kind = None;
    let mut git_marker = false;
    let mut timestamp_queries = 0;
    let mut timestamp_failures = 0;
    let mut deferred_activity_candidates = Vec::new();
    let mut errors = Vec::new();
    let mut complete = true;
    let mut mounts_skipped = 0;
    let mut entries_seen = 0;

    let opened = match task.directory_fd.take() {
        Some(fd) => scanner.adopt(fd),
        None => scanner.open(&task.path),
    };
    if !opened { return None; }

    loop {
        let mut entry = NativeEntry::default();
        let result = unsafe { ds_next_entry(scanner.handle, &mut entry) };
        if result == 0 { break; }
        if result < 0 {
            let error_code = unsafe { ds_last_errno(scanner.handle) };
            errors.push((task.path.clone(), format!("native_errno={error_code}")));
            complete = false;
            break;
        }
        entries_seen += 1;

        let name_length = (entry.name_length as usize).min(NATIVE_NAME_CAPACITY);
        let name = OsString::from_vec(entry.name[..name_length].to_vec());
        if name.is_empty() || name == OsStr::new(".") || name == OsStr::new("..") { continue; }
        if entry.error_code != 0 {
            errors.push((task.path.join(&name), format!("entry_error={}", entry.error_code)));
            complete = false;
            continue;
        }
        if entry.device_id != root_device {
            mounts_skipped += 1;
            continue;
        }

        project_kind = merge_project_kind(project_kind, project_marker_kind(&name));
        let lower_entry_name = lower_name(&name);
        if lower_entry_name == ".git" { git_marker = true; }
        if lower_entry_name == "pyvenv.cfg" { environment_kind = Some("python_venv"); }
        if lower_entry_name == "conda-meta" { environment_kind = Some("conda_env"); }
        let entry_context = if entry.object_type == VDIR { derive_context(task.context, &name) } else { task.context };
        let version_info = version_cluster_key(&name);
        let version_candidate = version_candidate_allowed(entry_context, &name, version_info.as_ref().map(|value| value.2).unwrap_or(false));
        if !version_candidate && task.context & CTX_PROJECT_TREE == 0 && entry.object_type == VREG && source_activity_candidate(entry_context, &name) && deferred_activity_candidates.len() < 100_000 {
            deferred_activity_candidates.push(name.clone());
        }
        let (created_seconds, modified_seconds) = if activity_timestamp_needed(entry_context, &name, version_candidate) {
            timestamp_queries += 1;
            match scanner.child_times(&name) {
                Some(value) => value,
                None => { timestamp_failures += 1; (0, 0) },
            }
        } else {
            (0, 0)
        };
        if version_candidate {
            maybe_collect_version_candidate(
                &mut version_groups,
                &mut version_candidates,
                &mut version_candidates_skipped,
                entry_context,
                &name,
                task.path.join(&name),
                entry.logical_size,
                entry.allocated_size,
                entry.private_size,
                created_seconds,
                modified_seconds,
            );
        }

        if entry.object_type == VDIR {
            if children.len() >= MAX_CHILDREN_PER_DIRECTORY {
                if !errors.iter().any(|(_, reason)| reason == "children_limit_exceeded") {
                    errors.push((task.path.clone(), "children_limit_exceeded".to_string()));
                }
                complete = false;
                continue;
            }
            let context = entry_context;
            let reserved = queued_fds
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                    (current < queued_fd_limit).then_some(current + 1)
                })
                .is_ok();
            let directory_fd = if reserved {
                match scanner.open_child(&name) {
                    Some(fd) => Some(fd),
                    None => {
                        queued_fds.fetch_sub(1, Ordering::Relaxed);
                        None
                    }
                }
            } else {
                None
            };
            children.push((task.path.join(&name), context, directory_fd));
        } else if entry.object_type == VREG {
            add_file_metrics(
                &mut direct,
                categories,
                hardlinks,
                task.context,
                &name,
                entry.device_id,
                entry.file_id,
                entry.link_count,
                entry.logical_size,
                entry.allocated_size,
                entry.private_size,
                modified_seconds,
            );
        }
    }
    let git_evidence = if git_marker { inspect_git_evidence(&task.path) } else { None };
    if project_kind.is_some() {
        for name in deferred_activity_candidates {
            timestamp_queries += 1;
            let modified_seconds = match scanner.child_times(&name) {
                Some((_, modified_seconds)) => modified_seconds,
                None => { timestamp_failures += 1; 0 },
            };
            direct.newest_source_modified_seconds = direct.newest_source_modified_seconds.max(modified_seconds);
            direct.newest_modified_seconds = direct.newest_modified_seconds.max(modified_seconds);
        }
    }
    scanner.close();

    let (version_clusters, version_cluster_count) = finalize_version_groups(version_groups);
    Some(ScanResult { id: task.id, direct, children, version_clusters, version_cluster_count, version_candidates, version_candidates_skipped, project_kind, environment_kind, timestamp_queries, timestamp_failures, git_evidence, errors, backend: "native", complete, mounts_skipped, entries_seen })
}

fn scan_directory_fallback(
    task: &Task,
    root_device: u64,
    hardlinks: &HardlinkSet,
    categories: &mut CategoryTotals,
) -> ScanResult {
    let mut direct = Metrics::default();
    let mut children = Vec::new();
    let mut version_groups = HashMap::new();
    let mut version_candidates = 0;
    let mut version_candidates_skipped = 0;
    let mut project_kind = None;
    let mut environment_kind = None;
    let mut git_marker = false;
    let mut timestamp_queries = 0;
    let mut errors = Vec::new();
    let mut complete = true;
    let mut mounts_skipped = 0;
    let mut entries_seen = 0;

    match fs::read_dir(&task.path) {
        Ok(entries) => {
            for result in entries {
                entries_seen += 1;
                let entry = match result {
                    Ok(value) => value,
                    Err(error) => { errors.push((task.path.clone(), format!("read_dir={error}"))); complete = false; continue; }
                };
                let path = entry.path();
                let metadata = match fs::symlink_metadata(&path) {
                    Ok(value) => value,
                    Err(error) => { errors.push((path, format!("metadata={error}"))); complete = false; continue; }
                };
                let file_type = metadata.file_type();
                if file_type.is_symlink() { continue; }
                if metadata.dev() != root_device { mounts_skipped += 1; continue; }

                let name = entry.file_name();
                project_kind = merge_project_kind(project_kind, project_marker_kind(&name));
                let lower_entry_name = lower_name(&name);
                if lower_entry_name == ".git" { git_marker = true; }
                if lower_entry_name == "pyvenv.cfg" { environment_kind = Some("python_venv"); }
                if lower_entry_name == "conda-meta" { environment_kind = Some("conda_env"); }

                if file_type.is_dir() {
                    let context = derive_context(task.context, &name);
                    let created_seconds = metadata.created().ok().and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok()).map(|value| value.as_secs()).unwrap_or(0);
                    let modified_seconds = metadata.modified().ok().and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok()).map(|value| value.as_secs()).unwrap_or(0);
                    timestamp_queries += 1;
                    maybe_collect_version_candidate(&mut version_groups, &mut version_candidates, &mut version_candidates_skipped, context, &name, path.clone(), metadata.len(), allocated(&metadata), 0, created_seconds, modified_seconds);
                    if children.len() >= MAX_CHILDREN_PER_DIRECTORY {
                        errors.push((task.path.clone(), "children_limit_exceeded".to_string()));
                        complete = false;
                        continue;
                    }
                    children.push((path, context, None));
                } else if file_type.is_file() {
                    let modified_seconds = metadata.modified().ok().and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok()).map(|value| value.as_secs()).unwrap_or(0);
                    let created_seconds = metadata.created().ok().and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok()).map(|value| value.as_secs()).unwrap_or(0);
                    timestamp_queries += 1;
                    maybe_collect_version_candidate(&mut version_groups, &mut version_candidates, &mut version_candidates_skipped, task.context, &name, path, metadata.len(), allocated(&metadata), 0, created_seconds, modified_seconds);
                    add_file_metrics(
                        &mut direct,
                        categories,
                        hardlinks,
                        task.context,
                        &name,
                        metadata.dev(),
                        metadata.ino(),
                        metadata.nlink() as u32,
                        metadata.len(),
                        allocated(&metadata),
                        0,
                        modified_seconds,
                    );
                }
            }
        }
        Err(error) => { errors.push((task.path.clone(), format!("read_dir={error}"))); complete = false; },
    }

    let (version_clusters, version_cluster_count) = finalize_version_groups(version_groups);
    let git_evidence = if git_marker { inspect_git_evidence(&task.path) } else { None };
    ScanResult { id: task.id, direct, children, version_clusters, version_cluster_count, version_candidates, version_candidates_skipped, project_kind, environment_kind, timestamp_queries, timestamp_failures: 0, git_evidence, errors, backend: "fallback", complete, mounts_skipped, entries_seen }
}

fn scan_directory(
    scanner: &mut NativeScanner,
    task: &mut Task,
    root_device: u64,
    hardlinks: &HardlinkSet,
    categories: &mut CategoryTotals,
    queued_fds: &AtomicUsize,
    queued_fd_limit: usize,
) -> ScanResult {
    scan_directory_native(scanner, task, root_device, hardlinks, categories, queued_fds, queued_fd_limit)
        .unwrap_or_else(|| scan_directory_fallback(task, root_device, hardlinks, categories))
}

fn worker(
    worker_id: usize,
    shared: Arc<(Mutex<SharedState>, Condvar)>,
    root_device: u64,
    hardlinks: Arc<HardlinkSet>,
    target_workers: Arc<AtomicUsize>,
    telemetry: Arc<Vec<WorkerTelemetry>>,
    queued_fds: Arc<AtomicUsize>,
    queued_fd_limit: usize,
) -> CategoryTotals {
    let mut category_totals = CategoryTotals::default();
    let mut native_scanner = NativeScanner::new();

    loop {
        let mut tasks = {
            let (lock, condition) = &*shared;
            let mut state = lock.lock().unwrap();
            loop {
                let enabled = target_workers.load(Ordering::Relaxed);
                if worker_id < enabled && !state.queue.is_empty() {
                    let fair_share = state.queue.len() / enabled.saturating_mul(8).max(1);
                    let take = fair_share.clamp(1, BATCH_SIZE).min(state.queue.len());
                    let mut batch = Vec::with_capacity(take);
                    for _ in 0..take {
                        if let Some(task) = state.queue.pop_back() {
                            batch.push(task);
                        }
                    }
                    state.active += batch.len();
                    break batch;
                }
                if state.active == 0 && state.queue.is_empty() {
                    state.done = true;
                    condition.notify_all();
                    return category_totals;
                }
                state = condition.wait(state).unwrap();
            }
        };

        let mut results = Vec::with_capacity(tasks.len());
        for task in &mut tasks {
            let held_directory_fd = task.directory_fd.is_some();
            let scan_started = Instant::now();
            let result = scan_directory(
                &mut native_scanner,
                task,
                root_device,
                &hardlinks,
                &mut category_totals,
                &queued_fds,
                queued_fd_limit,
            );
            if held_directory_fd {
                queued_fds.fetch_sub(1, Ordering::Relaxed);
            }
            telemetry[worker_id].entries.fetch_add(result.entries_seen, Ordering::Relaxed);
            telemetry[worker_id].directories.fetch_add(1, Ordering::Relaxed);
            telemetry[worker_id].scan_nanos.fetch_add(
                scan_started.elapsed().as_nanos().min(u64::MAX as u128) as u64,
                Ordering::Relaxed,
            );
            results.push(result);
        }

        let (lock, condition) = &*shared;
        let mut state = lock.lock().unwrap();
        for result in results {
            state.records[result.id as usize].direct = result.direct;
            state.records[result.id as usize].project_kind = result.project_kind;
            state.records[result.id as usize].git_evidence = result.git_evidence;
            if result.backend == "native" { state.native_directories += 1; }
            if result.backend == "fallback" { state.fallback_directories += 1; }
            if !result.complete { state.partial_directories += 1; }
            if result.environment_kind.is_some() {
                state.records[result.id as usize].environment_kind = result.environment_kind;
            }
            state.version_cluster_count = state.version_cluster_count.saturating_add(result.version_cluster_count);
            state.version_candidates = state.version_candidates.saturating_add(result.version_candidates);
            state.version_candidates_skipped = state.version_candidates_skipped.saturating_add(result.version_candidates_skipped);
            state.timestamp_queries = state.timestamp_queries.saturating_add(result.timestamp_queries);
            state.timestamp_failures = state.timestamp_failures.saturating_add(result.timestamp_failures);
            for cluster in result.version_clusters {
                state.version_clusters.push(cluster);
                state.version_clusters.sort_unstable_by(|a, b| b.review_reclaim_private.cmp(&a.review_reclaim_private).then_with(|| b.review_reclaim_physical.cmp(&a.review_reclaim_physical)));
                state.version_clusters.truncate(VERSION_CLUSTER_TOP_K);
            }
            let parent_name = state.records[result.id as usize].name.clone();
            let parent_context = state.records[result.id as usize].context;
            for (child_path, context, directory_fd) in result.children {
                if state.records.len() >= MAX_DIRECTORY_RECORDS {
                    if state.error_paths.len() < 100 {
                        state.error_paths.push((child_path, format!("directory_record_limit={MAX_DIRECTORY_RECORDS}")));
                    }
                    state.permission_errors += 1;
                    state.partial_directories += 1;
                    continue;
                }
                let id = state.records.len() as u32;
                let name = child_path.file_name().unwrap_or_else(|| OsStr::new("")).to_os_string();
                let child_context = if result.project_kind.is_some() { context | CTX_PROJECT_TREE } else { context };
                state.records.push(DirectoryRecord {
                    name,
                    parent: Some(result.id),
                    context: child_context,
                    direct: Metrics::default(),
                    total: Metrics::default(),
                    environment_kind: environment_kind_for_child(&parent_name, child_path.file_name().unwrap_or_else(|| OsStr::new("")), parent_context),
                    project_kind: None,
                    git_evidence: None,
                });
                state.queue.push_back(Task { id, path: child_path, context: child_context, directory_fd });
            }
            state.permission_errors += result.errors.len() as u64;
            state.mounts_skipped += result.mounts_skipped;
            for error in result.errors {
                if state.error_paths.len() < 100 { state.error_paths.push(error); }
            }
        }
        state.active -= tasks.len();
        if state.active == 0 && state.queue.is_empty() { state.done = true; }
        condition.notify_all();
    }
}

fn spawn_worker_thread(
    worker_id: usize,
    shared: Arc<(Mutex<SharedState>, Condvar)>,
    root_device: u64,
    hardlinks: Arc<HardlinkSet>,
    target_workers: Arc<AtomicUsize>,
    telemetry: Arc<Vec<WorkerTelemetry>>,
    queued_fds: Arc<AtomicUsize>,
    queued_fd_limit: usize,
) -> std::io::Result<thread::JoinHandle<CategoryTotals>> {
    thread::Builder::new()
        .name(format!("disk-scout-{worker_id}"))
        .stack_size(512 * 1024)
        .spawn(move || {
            worker(
                worker_id,
                shared,
                root_device,
                hardlinks,
                target_workers,
                telemetry,
                queued_fds,
                queued_fd_limit,
            )
        })
}

fn keep_top(heap: &mut BinaryHeap<Reverse<(u64, u32)>>, value: u64, id: u32) {
    heap.push(Reverse((value, id)));
    if heap.len() > TOP_K { heap.pop(); }
}

fn path_for(root: &Path, records: &[DirectoryRecord], mut id: u32) -> PathBuf {
    if id == 0 { return root.to_path_buf(); }
    let mut names = Vec::new();
    while id != 0 {
        let record = &records[id as usize];
        names.push(record.name.clone());
        id = record.parent.unwrap_or(0);
    }
    let mut path = root.to_path_buf();
    for name in names.into_iter().rev() { path.push(name); }
    path
}

fn repository_root_id(records: &[DirectoryRecord], mut id: u32) -> Option<u32> {
    loop {
        if records[id as usize].git_evidence.is_some() { return Some(id); }
        match records[id as usize].parent {
            Some(parent) => id = parent,
            None => return None,
        }
    }
}

fn context_label(context: u32) -> &'static str {
    if context & CTX_MESSAGE_MEDIA != 0 { "messaging_media" }
    else if context & CTX_DOCKER != 0 { "docker_vm" }
    else if context & CTX_XCODE != 0 { "xcode_simulator_build" }
    else if context & CTX_PACKAGE_CACHE != 0 { "package_cache" }
    else if context & CTX_JS_DEPS != 0 { "javascript_dependencies" }
    else if context & CTX_PYTHON_DEPS != 0 { "python_dependencies" }
    else if context & CTX_RUST != 0 { "rust_cargo" }
    else if context & CTX_JVM != 0 { "jvm_gradle_maven" }
    else if context & CTX_GO != 0 { "go_modules_build" }
    else if context & CTX_DOTNET != 0 { "dotnet_nuget" }
    else if context & CTX_RUBY_PHP != 0 { "ruby_php" }
    else if context & CTX_ANDROID != 0 { "android_mobile" }
    else if context & CTX_NATIVE != 0 { "native_cpp" }
    else if context & CTX_GAME != 0 { "game_engine" }
    else if context & CTX_MEDIA_PRODUCTION != 0 { "media_production" }
    else if context & CTX_INFRA != 0 { "infrastructure_cloud" }
    else if context & CTX_GIT != 0 { "git_vcs" }
    else if context & CTX_BROWSER != 0 { "browser_editor_cache" }
    else if context & CTX_AI != 0 { "ai_models_data" }
    else if context & CTX_BUILD != 0 { "build_artifacts" }
    else if context & CTX_LOGS != 0 { "logs_diagnostics" }
    else { "general" }
}

fn format_size(bytes: u64) -> String {
    let gib = bytes as f64 / 1_073_741_824.0;
    if gib >= 1.0 { format!("{gib:.2} GiB") }
    else { format!("{:.1} MiB", bytes as f64 / 1_048_576.0) }
}

fn print_top(
    label: &str,
    heap: BinaryHeap<Reverse<(u64, u32)>>,
    root: &Path,
    records: &[DirectoryRecord],
) {
    let mut entries: Vec<(u64, u32)> = heap.into_iter().map(|Reverse(value)| value).collect();
    entries.sort_unstable_by(|a, b| b.0.cmp(&a.0));
    for (value, id) in entries {
        let record = &records[id as usize];
        println!(
            "{label}\tvalue={}\tprivate={}\tallocated={}\tlogical={}\tfiles={}\ttiny={}\tsmall={}\tsmall_text={}\tkind={}\tpath={}",
            value,
            format_size(record.total.private),
            format_size(record.total.physical),
            format_size(record.total.logical),
            record.total.files,
            record.total.tiny,
            record.total.small,
            record.total.small_text,
            context_label(record.context),
            escape_path(&path_for(root, records, id)),
        );
    }
}

fn main() {
    let mut arguments = env::args_os().skip(1);
    let root = PathBuf::from(arguments.next().expect("usage: disk-scout ROOT [auto|max-throughput|THREADS]"));
    let worker_argument = arguments.next();
    let worker_mode = worker_argument
        .as_ref()
        .and_then(|value| value.to_str())
        .unwrap_or("auto");
    let interactive_profile = worker_mode.eq_ignore_ascii_case("auto") || worker_mode.eq_ignore_ascii_case("interactive");
    let max_profile = worker_mode.eq_ignore_ascii_case("max") || worker_mode.eq_ignore_ascii_case("max-throughput");
    let auto_tune = interactive_profile || max_profile;
    if interactive_profile { unsafe { ds_set_interactive_priority(); } }
    let logical_cpus = unsafe { ds_logical_cpu_count() }.max(1);
    let cpu_budget_cores = if interactive_profile { (logical_cpus as f64 * 0.25).max(4.0) } else { f64::INFINITY };
    let load_budget = if interactive_profile { 0.85 } else { f64::INFINITY };
    let resource_worker_limit = unsafe { ds_recommended_worker_limit() }.clamp(4, ABSOLUTE_WORKER_CAP);
    let max_workers = if auto_tune {
        resource_worker_limit
    } else {
        worker_argument
            .and_then(|value| value.to_str().and_then(|text| text.parse::<usize>().ok()))
            .expect("THREADS must be an integer from 1 to 16384, or use auto/max-throughput")
            .clamp(1, ABSOLUTE_WORKER_CAP)
    };
    let initial_workers = if auto_tune {
        if max_profile {
            ((logical_cpus as usize) / 2).max(4).min(max_workers)
        } else {
            2.min(max_workers)
        }
    } else {
        max_workers
    };

    let root_metadata = fs::symlink_metadata(&root).expect("unable to stat root");
    let root_device = root_metadata.dev();
    let root_context = derive_context(0, root.file_name().unwrap_or_else(|| OsStr::new("")));
    let initial = SharedState {
        queue: VecDeque::from([Task { id: 0, path: root.clone(), context: root_context, directory_fd: None }]),
        records: vec![DirectoryRecord {
            name: OsString::new(),
            parent: None,
            context: root_context,
            direct: Metrics::default(),
            total: Metrics::default(),
            environment_kind: None,
            project_kind: None,
            git_evidence: None,
        }],
        active: 0,
        done: false,
        permission_errors: 0,
        mounts_skipped: 0,
        error_paths: Vec::new(),
        native_directories: 0,
        fallback_directories: 0,
        partial_directories: 0,
        version_clusters: Vec::new(),
        version_cluster_count: 0,
        version_candidates: 0,
        version_candidates_skipped: 0,
        timestamp_queries: 0,
        timestamp_failures: 0,
    };

    let shared = Arc::new((Mutex::new(initial), Condvar::new()));
    let hardlinks = Arc::new(HardlinkSet::new());
    let target_workers = Arc::new(AtomicUsize::new(initial_workers));
    let telemetry = Arc::new((0..max_workers).map(|_| WorkerTelemetry::default()).collect::<Vec<_>>());
    let queued_fds = Arc::new(AtomicUsize::new(0));
    let queued_fd_limit = unsafe { ds_recommended_fd_queue_limit() };
    let started = Instant::now();
    let mut tuner = AutoTuner::new(
        initial_workers,
        max_workers,
        started,
        cpu_budget_cores,
        load_budget,
        unsafe { ds_process_cpu_seconds() },
    );
    let mut workers = Vec::new();
    for worker_id in 0..initial_workers {
        workers.push(spawn_worker_thread(
            worker_id,
            shared.clone(),
            root_device,
            hardlinks.clone(),
            target_workers.clone(),
            telemetry.clone(),
            queued_fds.clone(),
            queued_fd_limit,
        ).expect("unable to spawn initial scanner worker"));
    }
    if !auto_tune {
        for worker_id in initial_workers..max_workers {
            workers.push(spawn_worker_thread(
                worker_id,
                shared.clone(),
                root_device,
                hardlinks.clone(),
                target_workers.clone(),
                telemetry.clone(),
                queued_fds.clone(),
                queued_fd_limit,
            ).expect("unable to spawn requested fixed scanner workers"));
        }
    }

    {
        let (lock, condition) = &*shared;
        let mut state = lock.lock().unwrap();
        while !state.done {
            let (next_state, _) = condition.wait_timeout(state, Duration::from_secs(1)).unwrap();
            state = next_state;
            if auto_tune && !state.done {
                let backlog = state.queue.len();
                let total_entries = telemetry_totals(&telemetry).0;
                if let Some(target) = tuner.observe(
                    Instant::now(),
                    total_entries,
                    backlog,
                    unsafe { ds_process_cpu_seconds() },
                    unsafe { ds_host_cpu_busy_fraction() },
                ) {
                    while workers.len() < target {
                        let worker_id = workers.len();
                        match spawn_worker_thread(
                            worker_id,
                            shared.clone(),
                            root_device,
                            hardlinks.clone(),
                            target_workers.clone(),
                            telemetry.clone(),
                            queued_fds.clone(),
                            queued_fd_limit,
                        ) {
                            Ok(handle) => workers.push(handle),
                            Err(_) => break,
                        }
                    }
                    let actual_target = target.min(workers.len());
                    if actual_target < target {
                        tuner.max = actual_target;
                        tuner.current = actual_target;
                        tuner.best = tuner.best.min(actual_target);
                        tuner.peak = tuner.peak.min(actual_target);
                    }
                    target_workers.store(actual_target, Ordering::Relaxed);
                    condition.notify_all();
                }
            }
        }
    }

    let workers_spawned = workers.len();
    let mut categories = CategoryTotals::default();
    for worker in workers {
        if let Ok(values) = worker.join() { categories.merge(values); }
    }

    let mut state = shared.0.lock().unwrap();
    let (metadata_entries, metadata_directories, metadata_scan_nanos) = telemetry_totals(&telemetry);
    for record in &mut state.records { record.total = record.direct; }
    for id in (1..state.records.len()).rev() {
        if let Some(parent) = state.records[id].parent {
            let child = state.records[id].total;
            state.records[parent as usize].total.add_assign(child);
        }
    }

    let mut allocated_top = BinaryHeap::new();
    let mut private_top = BinaryHeap::new();
    let mut files_top = BinaryHeap::new();
    let mut tiny_top = BinaryHeap::new();
    let mut small_text_top = BinaryHeap::new();
    let mut slack_top = BinaryHeap::new();
    for (id, record) in state.records.iter().enumerate() {
        let id = id as u32;
        keep_top(&mut allocated_top, record.total.physical, id);
        keep_top(&mut private_top, record.total.private, id);
        keep_top(&mut files_top, record.total.files, id);
        keep_top(&mut tiny_top, record.total.tiny, id);
        keep_top(&mut small_text_top, record.total.small_text, id);
        keep_top(&mut slack_top, record.total.physical.saturating_sub(record.total.logical), id);
    }

    println!(
        "SUMMARY\troot={}\tprivate={}\tallocated={}\tlogical={}\tdirectories={}\tnative_directories={}\tfallback_directories={}\tpartial_directories={}\tfiles={}\ttiny={}\tsmall={}\tsmall_text={}\thardlink_duplicates={}\thardlink_tracking_saturated={}\tpermission_errors={}\tmounts_skipped={}\tversion_candidates={}\tversion_candidates_skipped={}\tversion_clusters={}\ttimestamp_queries={}\ttimestamp_failures={}\tworker_mode={}\tworkers_initial={}\tworkers_final={}\tworkers_best={}\tworkers_peak={}\tworkers_spawned={}\tworkers_max={}\tworkers_resource_limit={}\tlogical_cpus={}\tcpu_budget_cores={:.2}\tpeak_cpu_cores={:.2}\thost_busy_budget={:.2}\tpeak_host_busy={:.2}\tautotune_probes={}\tautotune_accepted={}\tautotune_rejected={}\tpeak_entries_per_second={:.0}\tmetadata_entries={}\tmetadata_directories={}\tmetadata_worker_seconds={:.2}\tqueued_fd_limit={}\tmetadata_backend=macos_getattrlistbulk_openat\telapsed_seconds={:.2}",
        escape_path(&root),
        format_size(state.records[0].total.private),
        format_size(state.records[0].total.physical),
        format_size(state.records[0].total.logical),
        state.records.len(),
        state.native_directories,
        state.fallback_directories,
        state.partial_directories,
        state.records[0].total.files,
        state.records[0].total.tiny,
        state.records[0].total.small,
        state.records[0].total.small_text,
        hardlinks.duplicates.load(Ordering::Relaxed),
        hardlinks.saturated(),
        state.permission_errors,
        state.mounts_skipped,
        state.version_candidates,
        state.version_candidates_skipped,
        state.version_cluster_count,
        state.timestamp_queries,
        state.timestamp_failures,
        if interactive_profile { "interactive" } else if max_profile { "max-throughput" } else { "fixed" },
        initial_workers,
        target_workers.load(Ordering::Relaxed),
        tuner.best,
        tuner.peak,
        workers_spawned,
        max_workers,
        resource_worker_limit,
        logical_cpus,
        cpu_budget_cores,
        tuner.peak_cpu_cores,
        load_budget,
        tuner.peak_system_load,
        tuner.probes,
        tuner.accepted_probes,
        tuner.rejected_probes,
        tuner.best_observed_rate,
        metadata_entries,
        metadata_directories,
        metadata_scan_nanos as f64 / 1_000_000_000.0,
        queued_fd_limit,
        started.elapsed().as_secs_f64(),
    );

    let mut category_order: Vec<usize> = (0..CATEGORY_COUNT).collect();
    category_order.sort_unstable_by(|a, b| categories.values[*b].physical.cmp(&categories.values[*a].physical));
    for index in category_order {
        let metric = categories.values[index];
        println!(
            "CATEGORY\tname={}\tprivate={}\tallocated={}\tlogical={}\tfiles={}\ttiny={}\tsmall={}\tsmall_text={}",
            CATEGORY_NAMES[index],
            format_size(metric.private),
            format_size(metric.physical),
            format_size(metric.logical),
            metric.files,
            metric.tiny,
            metric.small,
            metric.small_text,
        );
    }

    let mut extension_order: Vec<(&String, &CategoryMetric)> = categories.extensions.iter().collect();
    extension_order.sort_unstable_by(|a, b| {
        b.1.physical
            .cmp(&a.1.physical)
            .then_with(|| b.1.files.cmp(&a.1.files))
            .then_with(|| a.0.cmp(b.0))
    });
    for (extension, metric) in extension_order {
        println!(
            "EXTENSION\tname={}\tprivate={}\tallocated={}\tlogical={}\tfiles={}\ttiny={}\tsmall={}\tsmall_text={}",
            extension,
            format_size(metric.private),
            format_size(metric.physical),
            format_size(metric.logical),
            metric.files,
            metric.tiny,
            metric.small,
            metric.small_text,
        );
    }

    let now_seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0);
    let mut environment_ids: Vec<u32> = state.records.iter().enumerate()
        .filter_map(|(id, record)| record.environment_kind.map(|_| id as u32))
        .collect();
    environment_ids.sort_unstable_by(|a, b| state.records[*b as usize].total.private.cmp(&state.records[*a as usize].total.private));
    for id in environment_ids.into_iter().take(ENVIRONMENT_TOP_K) {
        let record = &state.records[id as usize];
        let newest = record.total.newest_modified_seconds;
        let age_days = if newest > 0 && now_seconds >= newest { Some((now_seconds - newest) / 86_400) } else { None };
        let age_label = age_days.map(|value| value.to_string()).unwrap_or_else(|| "unknown".to_string());
        let stale = age_days.map(|value| if value >= STALE_REVIEW_DAYS { "true" } else { "false" }).unwrap_or("unknown");
        println!(
            "ENVIRONMENT\tkind={}\tprivate_bytes={}\tallocated_bytes={}\tlogical_bytes={}\tprivate={}\tallocated={}\tlogical={}\tfiles={}\ttiny={}\tsmall_text={}\tnewest_modified_epoch={}\tage_days={}\tstale_review={}\tpath={}",
            record.environment_kind.unwrap_or("unknown"),
            record.total.private,
            record.total.physical,
            record.total.logical,
            format_size(record.total.private),
            format_size(record.total.physical),
            format_size(record.total.logical),
            record.total.files,
            record.total.tiny,
            record.total.small_text,
            newest,
            age_label,
            stale,
            escape_path(&path_for(&root, &state.records, id)),
        );
    }

    let mut project_ids: Vec<u32> = state.records.iter().enumerate()
        .filter_map(|(id, record)| record.project_kind.map(|_| id as u32))
        .collect();
    project_ids.sort_unstable_by(|a, b| state.records[*b as usize].total.private.cmp(&state.records[*a as usize].total.private));
    println!(
        "EVIDENCE_SUMMARY\tenvironments_total={}\tenvironments_reported={}\tprojects_total={}\tprojects_reported={}\tgit_repositories_total={}\tgit_repositories_reported={}\tversion_clusters_total={}\tversion_clusters_reported={}",
        state.records.iter().filter(|record| record.environment_kind.is_some()).count(),
        state.records.iter().filter(|record| record.environment_kind.is_some()).take(ENVIRONMENT_TOP_K).count(),
        state.records.iter().filter(|record| record.project_kind.is_some()).count(),
        state.records.iter().filter(|record| record.project_kind.is_some()).take(PROJECT_TOP_K).count(),
        state.records.iter().filter(|record| record.git_evidence.is_some()).count(),
        state.records.iter().filter(|record| record.git_evidence.is_some()).take(PROJECT_TOP_K).count(),
        state.version_cluster_count,
        state.version_clusters.len(),
    );
    for id in project_ids.into_iter().take(PROJECT_TOP_K) {
        let record = &state.records[id as usize];
        let newest = record.total.newest_source_modified_seconds;
        let generated_newest = record.total.newest_generated_modified_seconds;
        let git_ref_activity = record.git_evidence.as_ref().map(|evidence| evidence.ref_activity_seconds).unwrap_or(0);
        let activity = newest.max(git_ref_activity);
        let source_age_days = if newest > 0 && now_seconds >= newest { Some((now_seconds - newest) / 86_400) } else { None };
        let activity_age_days = if activity > 0 && now_seconds >= activity { Some((now_seconds - activity) / 86_400) } else { None };
        let generated_age_days = if generated_newest > 0 && now_seconds >= generated_newest { Some((now_seconds - generated_newest) / 86_400) } else { None };
        let source_age_label = source_age_days.map(|value| value.to_string()).unwrap_or_else(|| "unknown".to_string());
        let activity_age_label = activity_age_days.map(|value| value.to_string()).unwrap_or_else(|| "unknown".to_string());
        let generated_age_label = generated_age_days.map(|value| value.to_string()).unwrap_or_else(|| "unknown".to_string());
        let stale = match (source_age_days, activity_age_days, git_ref_activity > 0) {
            (Some(source), Some(activity), true) if source >= STALE_REVIEW_DAYS && activity < STALE_REVIEW_DAYS => "review",
            (Some(source), _, _) => if source >= STALE_REVIEW_DAYS { "true" } else { "false" },
            _ => "unknown",
        };
        let activity_basis = match (newest > 0, git_ref_activity > 0) {
            (true, true) => "source+git_ref",
            (true, false) => "source",
            (false, true) => "git_ref",
            (false, false) => "unknown",
        };
        let git_branch = record.git_evidence.as_ref().map(|evidence| evidence.branch.as_str()).unwrap_or("unknown");
        let git_head_ref = record.git_evidence.as_ref().map(|evidence| evidence.head_ref.as_str()).unwrap_or("unknown");
        let git_metadata_bytes = record.git_evidence.as_ref().map(|evidence| evidence.metadata_bytes).unwrap_or(0);
        let repository_id = repository_root_id(&state.records, id);
        let repository_root = repository_id.map(|root_id| escape_path(&path_for(&root, &state.records, root_id))).unwrap_or_else(|| "unknown".to_string());
        let project_overlap = repository_id.map(|root_id| root_id != id).unwrap_or(false);
        println!(
            "PROJECT\tkind={}\tgit_repo={}\trepository_root={}\tproject_overlap={}\tgit_branch={}\tgit_head_ref={}\tgit_ref_activity_epoch={}\tgit_metadata_bytes={}\tactivity_basis={}\tprivate_bytes={}\tallocated_bytes={}\tlogical_bytes={}\tprivate={}\tallocated={}\tlogical={}\tfiles={}\tsource_files={}\tgenerated_files={}\tnewest_source_modified_epoch={}\tsource_age_days={}\tactivity_age_days={}\tnewest_generated_modified_epoch={}\tgenerated_age_days={}\tstale_review={}\tpath={}",
            record.project_kind.unwrap_or("project"),
            record.git_evidence.is_some(),
            repository_root,
            project_overlap,
            git_branch,
            git_head_ref,
            git_ref_activity,
            git_metadata_bytes,
            activity_basis,
            record.total.private,
            record.total.physical,
            record.total.logical,
            format_size(record.total.private),
            format_size(record.total.physical),
            format_size(record.total.logical),
            record.total.files,
            record.total.source_files,
            record.total.generated_files,
            newest,
            source_age_label,
            activity_age_label,
            generated_newest,
            generated_age_label,
            stale,
            escape_path(&path_for(&root, &state.records, id)),
        );
        if let Some(git) = record.git_evidence.as_ref() {
            println!(
                "GIT_REPOSITORY\troot={}\tbranch={}\thead_ref={}\thead_oid={}\tcommon_git_dir={}\tworktree_state={}\tref_activity_epoch={}\tindex_modified_epoch={}\tindex_entries={}\tmodified_tracked_files={}\tdeleted_tracked_files={}\tworktree_count={}\tlocked_worktree_count={}\tprunable_worktree_count={}\tremote_count={}\tsubmodule_count={}\tmetadata_bytes={}",
                escape_path(&path_for(&root, &state.records, id)),
                git.branch,
                git.head_ref,
                git.head_oid,
                git.common_git_dir,
                git.worktree_state,
                git.ref_activity_seconds,
                git.index_modified_seconds,
                git.index_entries,
                git.modified_tracked_files,
                git.deleted_tracked_files,
                git.worktree_count,
                git.locked_worktree_count,
                git.prunable_worktree_count,
                git.remote_count,
                git.submodule_count,
                git.metadata_bytes,
            );
        }
    }

    for (cluster_id, cluster) in state.version_clusters.iter().enumerate() {
        let suggested_keep = cluster.members.get(cluster.suggested_keep).map(|member| escape_path(&member.path)).unwrap_or_default();
        println!(
            "VERSION_CLUSTER\tid={}\tkey={}\tconfidence={}\tevidence_quality={}\tmembers={}\treview_reclaim_private_bytes={}\treview_reclaim_allocated_bytes={}\treview_reclaim_private={}\treview_reclaim_allocated={}\tcreated_span_days={}\tmodified_span_days={}\treason={}\tsuggested_keep={}",
            cluster_id,
            escape_tsv(&cluster.key),
            cluster.confidence,
            cluster.evidence_quality,
            cluster.members.len(),
            cluster.review_reclaim_private,
            cluster.review_reclaim_physical,
            format_size(cluster.review_reclaim_private),
            format_size(cluster.review_reclaim_physical),
            cluster.created_span_days,
            cluster.modified_span_days,
            escape_tsv(&cluster.reason),
            suggested_keep,
        );
        for (member_id, member) in cluster.members.iter().enumerate() {
            println!(
                "VERSION_MEMBER\tcluster_id={}\tmember_id={}\tversion_rank={}\tcreated_epoch={}\tmodified_epoch={}\tprivate_bytes={}\tallocated_bytes={}\tlogical_bytes={}\tprivate={}\tallocated={}\tlogical={}\tpath={}",
                cluster_id,
                member_id,
                member.version_rank,
                member.created_seconds,
                member.modified_seconds,
                member.private,
                member.physical,
                member.logical,
                format_size(member.private),
                format_size(member.physical),
                format_size(member.logical),
                escape_path(&member.path),
            );
        }
    }

    print_top("TOP_ALLOCATED", allocated_top, &root, &state.records);
    print_top("TOP_PRIVATE", private_top, &root, &state.records);
    print_top("TOP_FILE_COUNT", files_top, &root, &state.records);
    print_top("TOP_TINY_FILES", tiny_top, &root, &state.records);
    print_top("TOP_SMALL_TEXT", small_text_top, &root, &state.records);
    print_top("TOP_ALLOCATION_SLACK", slack_top, &root, &state.records);

    for (path, reason) in &state.error_paths {
        println!("ERROR_PATH\tpath={}\treason={}", escape_path(path), escape_tsv(reason));
    }
    println!(
        "HARDLINK_SUMMARY\tduplicates={}\ttracking_saturated={}\tattribution=first_observed\tdeterministic=false",
        hardlinks.duplicates.load(Ordering::Relaxed),
        hardlinks.saturated(),
    );
    if (state.permission_errors > 0 || state.partial_directories > 0) && std::env::var_os("DISK_SCOUT_ALLOW_INCOMPLETE").is_none() {
        eprintln!("ERROR\tscan incomplete; set DISK_SCOUT_ALLOW_INCOMPLETE=1 only when best-effort output is explicitly acceptable");
        std::process::exit(1);
    }
}
