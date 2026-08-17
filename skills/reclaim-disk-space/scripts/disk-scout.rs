use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet, VecDeque};
use std::env;
use std::ffi::{c_char, c_int, c_void, CString, OsStr, OsString};
use std::fs;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::os::unix::fs::MetadataExt;
use std::os::fd::{FromRawFd, IntoRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const CATEGORY_COUNT: usize = 30;
const TOP_K: usize = 50;
const BATCH_SIZE: usize = 32;
const ABSOLUTE_WORKER_CAP: usize = 16_384;

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

#[derive(Clone, Copy, Default)]
struct Metrics {
    logical: u64,
    physical: u64,
    private: u64,
    files: u64,
    tiny: u64,
    small: u64,
    small_text: u64,
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
        let extension_metric = self.extensions.entry(extension).or_default();
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
            let metric = self.extensions.entry(extension).or_default();
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
    errors: Vec<PathBuf>,
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
    error_paths: Vec<PathBuf>,
}

struct HardlinkSet {
    shards: Vec<Mutex<HashSet<(u64, u64)>>>,
    duplicates: AtomicU64,
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
        }
    }

    fn is_first_parts(&self, device: u64, file_id: u64, link_count: u32) -> bool {
        if link_count <= 1 { return true; }
        let key = (device, file_id);
        let shard = ((key.0 ^ key.1) as usize) & (self.shards.len() - 1);
        let mut seen = self.shards[shard].lock().unwrap();
        if seen.insert(key) {
            true
        } else {
            self.duplicates.fetch_add(1, Ordering::Relaxed);
            false
        }
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
    let mut errors = Vec::new();
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
            let _error_code = unsafe { ds_last_errno(scanner.handle) };
            errors.push(task.path.clone());
            break;
        }
        entries_seen += 1;

        let name_length = (entry.name_length as usize).min(NATIVE_NAME_CAPACITY);
        let name = OsString::from_vec(entry.name[..name_length].to_vec());
        if name.is_empty() || name == OsStr::new(".") || name == OsStr::new("..") { continue; }
        if entry.error_code != 0 {
            errors.push(task.path.join(&name));
            continue;
        }
        if entry.device_id != root_device {
            mounts_skipped += 1;
            continue;
        }

        if entry.object_type == VDIR {
            let context = derive_context(task.context, &name);
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
            );
        }
    }
    scanner.close();

    Some(ScanResult { id: task.id, direct, children, errors, mounts_skipped, entries_seen })
}

fn scan_directory_fallback(
    task: &Task,
    root_device: u64,
    hardlinks: &HardlinkSet,
    categories: &mut CategoryTotals,
) -> ScanResult {
    let mut direct = Metrics::default();
    let mut children = Vec::new();
    let mut errors = Vec::new();
    let mut mounts_skipped = 0;
    let mut entries_seen = 0;

    match fs::read_dir(&task.path) {
        Ok(entries) => {
            for result in entries {
                entries_seen += 1;
                let entry = match result {
                    Ok(value) => value,
                    Err(_) => { errors.push(task.path.clone()); continue; }
                };
                let path = entry.path();
                let metadata = match fs::symlink_metadata(&path) {
                    Ok(value) => value,
                    Err(_) => { errors.push(path); continue; }
                };
                let file_type = metadata.file_type();
                if file_type.is_symlink() { continue; }
                if metadata.dev() != root_device { mounts_skipped += 1; continue; }

                if file_type.is_dir() {
                    let context = derive_context(task.context, &entry.file_name());
                    children.push((path, context, None));
                } else if file_type.is_file() {
                    let name = entry.file_name();
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
                    );
                }
            }
        }
        Err(_) => errors.push(task.path.clone()),
    }

    ScanResult { id: task.id, direct, children, errors, mounts_skipped, entries_seen }
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
                            if task.directory_fd.is_some() {
                                queued_fds.fetch_sub(1, Ordering::Relaxed);
                            }
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
            for (child_path, context, directory_fd) in result.children {
                let id = state.records.len() as u32;
                let name = child_path.file_name().unwrap_or_else(|| OsStr::new("")).to_os_string();
                state.records.push(DirectoryRecord {
                    name,
                    parent: Some(result.id),
                    context,
                    direct: Metrics::default(),
                    total: Metrics::default(),
                });
                state.queue.push_back(Task { id, path: child_path, context, directory_fd });
            }
            state.permission_errors += result.errors.len() as u64;
            state.mounts_skipped += result.mounts_skipped;
            for path in result.errors {
                if state.error_paths.len() < 100 { state.error_paths.push(path); }
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
            path_for(root, records, id).display(),
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
    let initial_workers = if auto_tune { 2.min(max_workers) } else { max_workers };

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
        }],
        active: 0,
        done: false,
        permission_errors: 0,
        mounts_skipped: 0,
        error_paths: Vec::new(),
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
        "SUMMARY\troot={}\tprivate={}\tallocated={}\tlogical={}\tdirectories={}\tfiles={}\ttiny={}\tsmall={}\tsmall_text={}\thardlink_duplicates={}\tpermission_errors={}\tmounts_skipped={}\tworker_mode={}\tworkers_initial={}\tworkers_final={}\tworkers_best={}\tworkers_peak={}\tworkers_spawned={}\tworkers_max={}\tworkers_resource_limit={}\tlogical_cpus={}\tcpu_budget_cores={:.2}\tpeak_cpu_cores={:.2}\thost_busy_budget={:.2}\tpeak_host_busy={:.2}\tautotune_probes={}\tautotune_accepted={}\tautotune_rejected={}\tpeak_entries_per_second={:.0}\tmetadata_entries={}\tmetadata_directories={}\tmetadata_worker_seconds={:.2}\tqueued_fd_limit={}\tmetadata_backend=macos_getattrlistbulk_openat\telapsed_seconds={:.2}",
        root.display(),
        format_size(state.records[0].total.private),
        format_size(state.records[0].total.physical),
        format_size(state.records[0].total.logical),
        state.records.len(),
        state.records[0].total.files,
        state.records[0].total.tiny,
        state.records[0].total.small,
        state.records[0].total.small_text,
        hardlinks.duplicates.load(Ordering::Relaxed),
        state.permission_errors,
        state.mounts_skipped,
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

    print_top("TOP_ALLOCATED", allocated_top, &root, &state.records);
    print_top("TOP_PRIVATE", private_top, &root, &state.records);
    print_top("TOP_FILE_COUNT", files_top, &root, &state.records);
    print_top("TOP_TINY_FILES", tiny_top, &root, &state.records);
    print_top("TOP_SMALL_TEXT", small_text_top, &root, &state.records);
    print_top("TOP_ALLOCATION_SLACK", slack_top, &root, &state.records);

    for path in &state.error_paths { println!("ERROR_PATH\t{}", path.display()); }
}
