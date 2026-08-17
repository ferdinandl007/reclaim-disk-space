use std::cmp::Reverse;
use std::env;
use std::ffi::CString;
use std::fs;
use std::io;
use std::os::raw::c_char;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

const ABSOLUTE_WORKER_CAP: usize = 16_384;
const ERROR_SAMPLE_LIMIT: usize = 100;
const SAMPLE_INTERVAL: Duration = Duration::from_millis(750);
const TUNE_WINDOW: Duration = Duration::from_secs(4);

extern "C" {
    fn ds_recommended_worker_limit() -> usize;
    fn ds_free_bytes(path: *const c_char) -> u64;
    fn ds_logical_cpu_count() -> u32;
    fn ds_process_cpu_seconds() -> f64;
    fn ds_host_cpu_busy_fraction() -> f64;
    fn ds_set_interactive_priority() -> i32;
}

struct Config {
    root: PathBuf,
    execute: bool,
    confirmation: Option<PathBuf>,
    requested_workers: Option<usize>,
    keep_root: bool,
    max_throughput: bool,
}

#[derive(Default)]
struct Inventory {
    nodes: Vec<PathBuf>,
    directories: Vec<PathBuf>,
    logical_bytes: u64,
    allocated_bytes: u64,
    scan_errors: Vec<String>,
    mounts_skipped: usize,
}

#[derive(Default)]
struct DeleteStats {
    next: AtomicUsize,
    completed: AtomicUsize,
    errors: AtomicUsize,
    op_nanos: AtomicU64,
    target_workers: AtomicUsize,
    workers_peak: AtomicUsize,
    stop: AtomicBool,
    samples: Mutex<Vec<String>>,
}

fn usage() -> ! {
    eprintln!("usage: disk-clean --root ABSOLUTE_PATH [--execute --confirm ABSOLUTE_PATH] [--workers auto|N] [--profile interactive|max-throughput] [--keep-root]");
    std::process::exit(2);
}

fn parse_args() -> Config {
    let mut arguments = env::args_os().skip(1);
    let mut root = None;
    let mut execute = false;
    let mut confirmation = None;
    let mut requested_workers = None;
    let mut keep_root = false;
    let mut max_throughput = false;

    while let Some(argument) = arguments.next() {
        match argument.to_string_lossy().as_ref() {
            "--root" => root = arguments.next().map(PathBuf::from),
            "--execute" => execute = true,
            "--confirm" => confirmation = arguments.next().map(PathBuf::from),
            "--workers" => {
                let value = arguments.next().unwrap_or_else(|| usage());
                if value != "auto" {
                    requested_workers = Some(
                        value.to_string_lossy().parse::<usize>().unwrap_or_else(|_| usage()),
                    );
                }
            }
            "--keep-root" => keep_root = true,
            "--profile" => {
                let value = arguments.next().unwrap_or_else(|| usage());
                match value.to_string_lossy().as_ref() {
                    "interactive" => max_throughput = false,
                    "max" | "max-throughput" => max_throughput = true,
                    _ => usage(),
                }
            }
            "--help" | "-h" => usage(),
            _ => usage(),
        }
    }

    Config {
        root: root.unwrap_or_else(|| usage()),
        execute,
        confirmation,
        requested_workers,
        keep_root,
        max_throughput,
    }
}

fn canonical_directory(value: &Path) -> io::Result<PathBuf> {
    let metadata = fs::symlink_metadata(value)?;
    if metadata.file_type().is_symlink() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "root may not be a symlink"));
    }
    if !metadata.is_dir() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "root must be a directory"));
    }
    value.canonicalize()
}

fn validate_root(root: &Path) -> io::Result<()> {
    if !root.is_absolute() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "root must be absolute"));
    }
    let protected = [
        Path::new("/"),
        Path::new("/Applications"),
        Path::new("/Library"),
        Path::new("/System"),
        Path::new("/System/Volumes"),
        Path::new("/System/Volumes/Data"),
        Path::new("/Users"),
        Path::new("/Volumes"),
    ];
    if protected.iter().any(|value| *value == root) || root.starts_with("/System") {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "refusing protected root"));
    }
    if let Some(home) = env::var_os("HOME") {
        if Path::new(&home).canonicalize().ok().as_deref() == Some(root) {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "refusing home directory"));
        }
    }
    Ok(())
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    format!("{value:.2} {}", UNITS[unit])
}

fn read_directory_retry(directory: &Path) -> io::Result<fs::ReadDir> {
    for _ in 0..8 {
        match fs::read_dir(directory) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
    Err(io::Error::new(io::ErrorKind::Interrupted, "directory read interrupted repeatedly"))
}

fn metadata_retry(target: &Path) -> io::Result<fs::Metadata> {
    for _ in 0..8 {
        match fs::symlink_metadata(target) {
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
    Err(io::Error::new(io::ErrorKind::Interrupted, "metadata read interrupted repeatedly"))
}

fn inventory(root: &Path) -> io::Result<Inventory> {
    let root_metadata = fs::symlink_metadata(root)?;
    let root_device = root_metadata.dev();
    let mut result = Inventory::default();
    let mut pending = vec![root.to_path_buf()];
    result.directories.push(root.to_path_buf());

    while let Some(directory) = pending.pop() {
        let mut entries = match read_directory_retry(&directory) {
            Ok(value) => value,
            Err(error) => {
                result.scan_errors.push(format!("{}: {error}", directory.display()));
                continue;
            }
        };
        while let Some(entry_result) = entries.next() {
            let entry = match entry_result {
                Ok(value) => value,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    result.scan_errors.push(format!("{}: {error}", directory.display()));
                    continue;
                }
            };
            let child = entry.path();
            let metadata = match metadata_retry(&child) {
                Ok(value) => value,
                Err(error) => {
                    result.scan_errors.push(format!("{}: {error}", child.display()));
                    continue;
                }
            };
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                if metadata.dev() != root_device {
                    result.mounts_skipped += 1;
                    continue;
                }
                result.directories.push(child.clone());
                pending.push(child);
            } else {
                result.logical_bytes = result.logical_bytes.saturating_add(metadata.size());
                result.allocated_bytes = result
                    .allocated_bytes
                    .saturating_add(metadata.blocks().saturating_mul(512));
                result.nodes.push(child);
            }
        }
    }
    Ok(result)
}

fn record_error(stats: &DeleteStats, target: &Path, error: &io::Error) {
    stats.errors.fetch_add(1, Ordering::Relaxed);
    let mut samples = stats.samples.lock().unwrap();
    if samples.len() < ERROR_SAMPLE_LIMIT {
        samples.push(format!("{}: {error}", target.display()));
    }
}

fn remove_node(target: &Path) -> io::Result<()> {
    for _ in 0..8 {
        match fs::remove_file(target) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(io::ErrorKind::Interrupted, "interrupted repeatedly"))
}

fn remove_directory(target: &Path) -> io::Result<()> {
    for _ in 0..8 {
        match fs::remove_dir(target) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(io::ErrorKind::Interrupted, "interrupted repeatedly"))
}

fn spawn_worker(
    worker_id: usize,
    nodes: Arc<Vec<PathBuf>>,
    stats: Arc<DeleteStats>,
) -> io::Result<thread::JoinHandle<()>> {
    thread::Builder::new()
        .name(format!("disk-clean-{worker_id}"))
        .spawn(move || {
            while !stats.stop.load(Ordering::Relaxed) {
                if worker_id >= stats.target_workers.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(20));
                    continue;
                }
                let index = stats.next.fetch_add(1, Ordering::Relaxed);
                if index >= nodes.len() {
                    break;
                }
                let started = Instant::now();
                if let Err(error) = remove_node(&nodes[index]) {
                    record_error(&stats, &nodes[index], &error);
                }
                stats.op_nanos.fetch_add(started.elapsed().as_nanos() as u64, Ordering::Relaxed);
                stats.completed.fetch_add(1, Ordering::Release);
            }
        })
}

fn free_bytes(root: &Path) -> u64 {
    let Ok(value) = CString::new(root.as_os_str().as_bytes()) else { return 0 };
    unsafe { ds_free_bytes(value.as_ptr() as *const c_char) }
}

fn delete_nodes(root: &Path, nodes: Vec<PathBuf>, maximum: usize, interactive: bool, logical_cpus: u32) -> DeleteStats {
    let nodes = Arc::new(nodes);
    let stats = Arc::new(DeleteStats::default());
    let initial = maximum.min(2).max(1);
    stats.target_workers.store(initial, Ordering::Relaxed);
    let mut workers = Vec::new();
    for worker_id in 0..initial {
        workers.push(spawn_worker(worker_id, nodes.clone(), stats.clone()).expect("worker spawn failed"));
    }
    stats.workers_peak.store(workers.len(), Ordering::Relaxed);

    let started = Instant::now();
    let mut last = Instant::now();
    let mut last_completed = 0;
    let mut last_nanos = 0;
    let mut last_cpu_seconds = unsafe { ds_process_cpu_seconds() };
    let mut window_started = Instant::now();
    let mut window_completed = 0;
    let mut best_target = initial;
    let mut best_rate = 0.0_f64;
    let mut hold_windows = 0_u8;
    while stats.completed.load(Ordering::Acquire) < nodes.len() {
        thread::sleep(SAMPLE_INTERVAL);
        let now = Instant::now();
        let completed = stats.completed.load(Ordering::Acquire);
        let op_nanos = stats.op_nanos.load(Ordering::Relaxed);
        let elapsed = now.duration_since(last).as_secs_f64().max(0.001);
        let delta = completed.saturating_sub(last_completed);
        let rate = delta as f64 / elapsed;
        let cpu_seconds = unsafe { ds_process_cpu_seconds() };
        let cpu_cores = (cpu_seconds - last_cpu_seconds).max(0.0) / elapsed;
        last_cpu_seconds = cpu_seconds;
        let system_load = unsafe { ds_host_cpu_busy_fraction() };
        let cpu_budget = (logical_cpus as f64 * 0.25).max(4.0);
        let load_budget = 0.85;
        let average_ms = if delta == 0 { 0.0 } else { (op_nanos - last_nanos) as f64 / delta as f64 / 1_000_000.0 };
        let available = free_bytes(root);
        let errors = stats.errors.load(Ordering::Relaxed);
        let current = stats.target_workers.load(Ordering::Relaxed);
        let mut target = current;

        let pressure = if interactive {
            (cpu_cores / cpu_budget).max(system_load / load_budget)
        } else {
            0.0
        };
        if pressure > 1.05 {
            target = ((current as f64 / pressure) * 0.80).floor().max(1.0) as usize;
            best_target = best_target.min(target);
            hold_windows = 2;
        } else if available > 0 && available < 2 * 1024 * 1024 * 1024 {
            target = 1;
            best_target = 1;
        } else if available > 0 && available < 5 * 1024 * 1024 * 1024 {
            target = target.min(2);
            best_target = best_target.min(2);
        } else if errors > 0 {
            target = (current / 2).max(1);
            best_target = best_target.min(target);
        } else if now.duration_since(window_started) >= TUNE_WINDOW {
            let window_elapsed = now.duration_since(window_started).as_secs_f64().max(0.001);
            let window_rate = completed.saturating_sub(window_completed) as f64 / window_elapsed;
            if current > best_target {
                if best_rate == 0.0 || window_rate >= best_rate * 1.03 {
                    best_target = current;
                    best_rate = window_rate;
                    target = current.saturating_mul(2).min(maximum);
                } else {
                    target = best_target;
                    hold_windows = 2;
                }
            } else {
                best_target = current;
                best_rate = if best_rate == 0.0 {
                    window_rate
                } else {
                    best_rate * 0.70 + window_rate * 0.30
                };
                if hold_windows > 0 {
                    hold_windows -= 1;
                } else if current < maximum {
                    target = current.saturating_mul(2).min(maximum);
                }
            }
            window_started = now;
            window_completed = completed;
        }

        if interactive && target > current {
            if cpu_cores > 0.05 {
                let projected = ((current as f64 * cpu_budget / cpu_cores) * 0.90)
                    .floor()
                    .max(current as f64) as usize;
                target = target.min(projected);
            }
            if system_load > 0.05 {
                let projected = ((current as f64 * load_budget / system_load) * 0.90)
                    .floor()
                    .max(current as f64) as usize;
                target = target.min(projected);
            }
        }

        while workers.len() < target {
            let worker_id = workers.len();
            match spawn_worker(worker_id, nodes.clone(), stats.clone()) {
                Ok(worker) => workers.push(worker),
                Err(_) => {
                    target = workers.len().max(1);
                    break;
                }
            }
        }
        stats.workers_peak.fetch_max(workers.len(), Ordering::Relaxed);
        stats.target_workers.store(target.min(workers.len()).max(1), Ordering::Relaxed);
        eprintln!(
            "PROGRESS\tdeleted={}\ttotal={}\trate={:.0}/s\tavg_ms={:.2}\tworkers={}\tspawned={}\tcpu_cores={:.2}\thost_busy={:.0}%\tfree={}\terrors={}\telapsed={:.1}s",
            completed, nodes.len(), rate, average_ms, stats.target_workers.load(Ordering::Relaxed),
            workers.len(), cpu_cores, system_load * 100.0, format_size(available), errors, started.elapsed().as_secs_f64()
        );
        last = now;
        last_completed = completed;
        last_nanos = op_nanos;
    }
    stats.stop.store(true, Ordering::Relaxed);
    for worker in workers { let _ = worker.join(); }
    Arc::try_unwrap(stats).unwrap_or_else(|_| panic!("worker state still shared"))
}

fn main() {
    let config = parse_args();
    let root = canonical_directory(&config.root).unwrap_or_else(|error| {
        eprintln!("ERROR\troot={}\t{error}", config.root.display());
        std::process::exit(2);
    });
    validate_root(&root).unwrap_or_else(|error| {
        eprintln!("ERROR\troot={}\t{error}", root.display());
        std::process::exit(2);
    });
    if !config.max_throughput { unsafe { ds_set_interactive_priority(); } }
    if config.execute {
        let confirmed = config.confirmation.as_deref().and_then(|value| value.canonicalize().ok());
        if confirmed.as_deref() != Some(root.as_path()) {
            eprintln!("ERROR\t--execute requires --confirm matching canonical root: {}", root.display());
            std::process::exit(2);
        }
    }

    eprintln!("ENUMERATING\troot={}", root.display());
    let mut found = inventory(&root).unwrap_or_else(|error| {
        eprintln!("ERROR\troot={}\t{error}", root.display());
        std::process::exit(1);
    });
    println!(
        "PLAN\troot={}\tnodes={}\tdirectories={}\tlogical={}\tallocated={}\tscan_errors={}\tmounts_skipped={}\tmode={}",
        root.display(), found.nodes.len(), found.directories.len(), format_size(found.logical_bytes),
        format_size(found.allocated_bytes), found.scan_errors.len(), found.mounts_skipped,
        if config.execute { "execute" } else { "dry-run" }
    );
    for error in found.scan_errors.iter().take(ERROR_SAMPLE_LIMIT) {
        eprintln!("SCAN_ERROR\t{error}");
    }
    if !config.execute {
        return;
    }
    if !found.scan_errors.is_empty() || found.mounts_skipped > 0 {
        eprintln!("ERROR\trefusing incomplete deletion plan; resolve scan errors or nested mounts first");
        std::process::exit(1);
    }

    let hardware_limit = unsafe { ds_recommended_worker_limit() }.clamp(1, ABSOLUTE_WORKER_CAP);
    let maximum = config.requested_workers.unwrap_or(hardware_limit).clamp(1, hardware_limit);
    let logical_cpus = unsafe { ds_logical_cpu_count() }.max(1);
    let stats = delete_nodes(&root, std::mem::take(&mut found.nodes), maximum, !config.max_throughput, logical_cpus);

    found.directories.sort_unstable_by_key(|value| Reverse((value.components().count(), value.as_os_str().len())));
    let mut directory_errors = 0;
    for directory in &found.directories {
        if config.keep_root && directory == &root { continue; }
        if let Err(error) = remove_directory(directory) {
            directory_errors += 1;
            record_error(&stats, directory, &error);
        }
    }
    let total_errors = stats.errors.load(Ordering::Relaxed);
    println!(
        "SUMMARY\troot={}\tnodes_deleted={}\tdirectories_attempted={}\terrors={}\tdirectory_errors={}\tprofile={}\tlogical_cpus={}\tworkers_peak={}\tworker_ceiling={}\tfree={}",
        root.display(), stats.completed.load(Ordering::Relaxed), found.directories.len(), total_errors,
        directory_errors, if config.max_throughput { "max-throughput" } else { "interactive" }, logical_cpus,
        stats.workers_peak.load(Ordering::Relaxed), maximum,
        format_size(free_bytes(root.parent().unwrap_or(Path::new("/"))))
    );
    for error in stats.samples.into_inner().unwrap() {
        eprintln!("DELETE_ERROR\t{error}");
    }
    if total_errors > 0 { std::process::exit(1); }
}
