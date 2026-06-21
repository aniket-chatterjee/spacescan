//! Reproducible benchmark helpers shared by the CLI and benchmark harnesses.

use std::io;
use std::path::{Path, PathBuf};
#[cfg(not(windows))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(not(windows))]
use std::sync::Arc;
#[cfg(not(windows))]
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde::Serialize;
#[cfg(not(windows))]
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

use crate::constants;
use crate::node::Node;
use crate::scan::{ScanError, ScanOptions, ScanProgress, Scanner};

#[derive(Clone, Debug)]
pub struct BenchConfig {
    pub root: PathBuf,
    pub runs: usize,
    pub warmups: usize,
    pub cluster_size: u64,
    pub follow_links: bool,
    pub excluded_paths: Vec<String>,
    pub prune_zero_size_dirs: bool,
    pub threads: usize,
}

impl BenchConfig {
    pub fn new(
        root: PathBuf,
        opts: &ScanOptions,
        runs: usize,
        warmups: usize,
        threads: usize,
    ) -> Self {
        Self {
            root,
            runs: runs.max(constants::bench::MIN_RUNS),
            warmups,
            cluster_size: opts.cluster_size,
            follow_links: opts.follow_links,
            excluded_paths: excluded_path_strings_from(opts),
            prune_zero_size_dirs: opts.prune_zero_size_dirs,
            threads,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchRun {
    pub index: usize,
    pub elapsed_seconds: f64,
    pub engine: &'static str,
    pub files: u64,
    pub dirs: u64,
    pub tree_nodes: u64,
    pub apparent_bytes: u64,
    pub disk_bytes: u64,
    pub empty_dirs: u64,
    pub zero_size_dirs: u64,
    pub dirs_with_files: u64,
    pub leaf_dirs: u64,
    pub single_child_dirs: u64,
    pub parallel_fanout_dirs: u64,
    pub large_fanout_dirs: u64,
    pub huge_fanout_dirs: u64,
    pub max_depth: usize,
    pub max_children: usize,
    pub max_child_dirs: usize,
    pub dirs_per_second: f64,
    pub tree_nodes_per_second: f64,
    pub node_size_bytes: u64,
    pub estimated_tree_node_bytes: u64,
    pub name_bytes: u64,
    pub estimated_tree_storage_bytes: u64,
    pub peak_rss_bytes: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct BenchSummary {
    pub tool: &'static str,
    pub root: String,
    pub os: &'static str,
    pub arch: &'static str,
    pub build_profile: &'static str,
    pub cache_state: &'static str,
    pub engine: &'static str,
    pub threads: usize,
    pub worker_threads: usize,
    pub logical_cpus: usize,
    pub cluster_size: u64,
    pub follow_links: bool,
    pub excluded_paths: Vec<String>,
    pub prune_zero_size_dirs: bool,
    pub memory_source: &'static str,
    pub memory_sample_ms: u64,
    pub warmups: usize,
    pub runs: usize,
    pub files: u64,
    pub dirs: u64,
    pub tree_nodes: u64,
    pub apparent_bytes: u64,
    pub disk_bytes: u64,
    pub empty_dirs: u64,
    pub zero_size_dirs: u64,
    pub dirs_with_files: u64,
    pub leaf_dirs: u64,
    pub single_child_dirs: u64,
    pub parallel_fanout_dirs: u64,
    pub large_fanout_dirs: u64,
    pub huge_fanout_dirs: u64,
    pub max_depth: usize,
    pub max_children: usize,
    pub max_child_dirs: usize,
    pub sample_counts_match: bool,
    pub sample_files_min: u64,
    pub sample_files_max: u64,
    pub sample_dirs_min: u64,
    pub sample_dirs_max: u64,
    pub min_seconds: f64,
    pub median_seconds: f64,
    pub p95_seconds: f64,
    pub files_per_second: f64,
    pub dirs_per_second: f64,
    pub tree_nodes_per_second: f64,
    pub node_size_bytes: u64,
    pub estimated_tree_node_bytes: u64,
    pub name_bytes: u64,
    pub estimated_tree_storage_bytes: u64,
    pub peak_rss_bytes: u64,
    pub samples: Vec<BenchRun>,
}

pub fn run_benchmark(
    scanner: &dyn Scanner,
    root: &Path,
    opts: &ScanOptions,
    runs: usize,
    warmups: usize,
    threads: usize,
    mut on_sample: impl FnMut(&BenchRun),
) -> Result<BenchSummary, ScanError> {
    run_warmups(scanner, root, opts, warmups)?;

    let runs = runs.max(constants::bench::MIN_RUNS);
    let mut samples = Vec::with_capacity(runs);
    let mut totals = TreeTotals::default();

    for index in 1..=runs {
        let measured = measure_scan(scanner, root, opts)?;
        totals = measured.totals;
        let elapsed_seconds = measured.elapsed.as_secs_f64();
        let sample = BenchRun {
            index,
            elapsed_seconds,
            engine: constants::scan::ENGINE_WALK,
            files: measured.totals.files,
            dirs: measured.totals.dirs,
            tree_nodes: measured.shape.tree_nodes,
            apparent_bytes: measured.totals.apparent_bytes,
            disk_bytes: measured.totals.disk_bytes,
            empty_dirs: measured.shape.empty_dirs,
            zero_size_dirs: measured.shape.zero_size_dirs,
            dirs_with_files: measured.shape.dirs_with_files,
            leaf_dirs: measured.shape.leaf_dirs,
            single_child_dirs: measured.shape.single_child_dirs,
            parallel_fanout_dirs: measured.shape.parallel_fanout_dirs,
            large_fanout_dirs: measured.shape.large_fanout_dirs,
            huge_fanout_dirs: measured.shape.huge_fanout_dirs,
            max_depth: measured.shape.max_depth,
            max_children: measured.shape.max_children,
            max_child_dirs: measured.shape.max_child_dirs,
            dirs_per_second: throughput_per_second(measured.totals.dirs, elapsed_seconds),
            tree_nodes_per_second: throughput_per_second(
                measured.shape.tree_nodes,
                elapsed_seconds,
            ),
            node_size_bytes: node_size_bytes(),
            estimated_tree_node_bytes: estimated_tree_node_bytes_for(measured.shape.tree_nodes),
            name_bytes: measured.shape.name_bytes,
            estimated_tree_storage_bytes: estimated_tree_storage_bytes_for(&measured.shape),
            peak_rss_bytes: measured.peak_rss_bytes,
        };
        on_sample(&sample);
        samples.push(sample);
    }

    let context = BenchContext {
        root,
        opts,
        runs,
        warmups,
        threads,
    };
    Ok(summarize_samples(context, totals, samples))
}

pub fn write_summary_json(summary: &BenchSummary, out: &Path) -> io::Result<()> {
    let file = std::fs::File::create(out)?;
    let writer = io::BufWriter::new(file);
    serde_json::to_writer_pretty(writer, summary).map_err(io::Error::other)
}

fn run_warmups(
    scanner: &dyn Scanner,
    root: &Path,
    opts: &ScanOptions,
    warmups: usize,
) -> Result<(), ScanError> {
    for _ in 0..warmups {
        let progress = ScanProgress::disabled();
        let _ = scanner.scan(root, opts, &progress)?;
    }
    Ok(())
}

fn measure_scan(
    scanner: &dyn Scanner,
    root: &Path,
    opts: &ScanOptions,
) -> Result<MeasuredScan, ScanError> {
    let progress = ScanProgress::disabled();
    let memory_sampler = MemorySampler::start();
    let start = Instant::now();
    let outcome = scanner.scan(root, opts, &progress);
    let elapsed = start.elapsed();
    let peak_rss_bytes = memory_sampler.finish();
    let outcome = outcome?;
    let shape = TreeShape::from_tree(&outcome.tree);
    Ok(MeasuredScan {
        elapsed,
        totals: TreeTotals {
            files: outcome.tree.file_count,
            dirs: outcome.tree.dir_count(),
            apparent_bytes: outcome.tree.apparent_size,
            disk_bytes: outcome.tree.disk_size,
        },
        shape,
        peak_rss_bytes,
    })
}

fn summarize_samples(
    context: BenchContext<'_>,
    totals: TreeTotals,
    samples: Vec<BenchRun>,
) -> BenchSummary {
    let mut seconds: Vec<f64> = samples
        .iter()
        .map(|sample| sample.elapsed_seconds)
        .collect();
    seconds.sort_by(f64::total_cmp);
    let min_seconds = seconds[0];
    let median_seconds = seconds[seconds.len() / 2];
    let p95_seconds = percentile_from(&seconds, constants::bench::P95_QUANTILE);
    let latest_shape = latest_shape_from(&samples);
    let files_per_second = throughput_per_second(totals.files, median_seconds);
    let dirs_per_second = throughput_per_second(totals.dirs, median_seconds);
    let tree_nodes_per_second = throughput_per_second(latest_shape.tree_nodes, median_seconds);
    let peak_rss_bytes = peak_rss_bytes_from(&samples);
    let sample_counts = sample_counts_from(&samples);

    BenchSummary {
        tool: constants::app::NAME,
        root: context.root.display().to_string(),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        build_profile: build_profile(),
        cache_state: cache_state_for(context.warmups),
        engine: constants::scan::ENGINE_WALK,
        threads: context.threads,
        worker_threads: rayon::current_num_threads(),
        logical_cpus: logical_cpu_count(),
        cluster_size: context.opts.cluster_size,
        follow_links: context.opts.follow_links,
        excluded_paths: excluded_path_strings_from(context.opts),
        prune_zero_size_dirs: context.opts.prune_zero_size_dirs,
        memory_source: MemorySampler::source(),
        memory_sample_ms: MemorySampler::sample_interval_ms(),
        warmups: context.warmups,
        runs: context.runs,
        files: totals.files,
        dirs: totals.dirs,
        tree_nodes: latest_shape.tree_nodes,
        apparent_bytes: totals.apparent_bytes,
        disk_bytes: totals.disk_bytes,
        empty_dirs: latest_shape.empty_dirs,
        zero_size_dirs: latest_shape.zero_size_dirs,
        dirs_with_files: latest_shape.dirs_with_files,
        leaf_dirs: latest_shape.leaf_dirs,
        single_child_dirs: latest_shape.single_child_dirs,
        parallel_fanout_dirs: latest_shape.parallel_fanout_dirs,
        large_fanout_dirs: latest_shape.large_fanout_dirs,
        huge_fanout_dirs: latest_shape.huge_fanout_dirs,
        max_depth: latest_shape.max_depth,
        max_children: latest_shape.max_children,
        max_child_dirs: latest_shape.max_child_dirs,
        sample_counts_match: sample_counts.match_all,
        sample_files_min: sample_counts.files_min,
        sample_files_max: sample_counts.files_max,
        sample_dirs_min: sample_counts.dirs_min,
        sample_dirs_max: sample_counts.dirs_max,
        min_seconds,
        median_seconds,
        p95_seconds,
        files_per_second,
        dirs_per_second,
        tree_nodes_per_second,
        node_size_bytes: node_size_bytes(),
        estimated_tree_node_bytes: estimated_tree_node_bytes_for(latest_shape.tree_nodes),
        name_bytes: latest_shape.name_bytes,
        estimated_tree_storage_bytes: estimated_tree_storage_bytes_for(&latest_shape),
        peak_rss_bytes,
        samples,
    }
}

struct BenchContext<'a> {
    root: &'a Path,
    opts: &'a ScanOptions,
    runs: usize,
    warmups: usize,
    threads: usize,
}

fn peak_rss_bytes_from(samples: &[BenchRun]) -> u64 {
    samples
        .iter()
        .map(|sample| sample.peak_rss_bytes)
        .max()
        .unwrap_or(0)
}

fn sample_counts_from(samples: &[BenchRun]) -> SampleCounts {
    let first = &samples[0];
    let mut counts = SampleCounts {
        match_all: true,
        files_min: first.files,
        files_max: first.files,
        dirs_min: first.dirs,
        dirs_max: first.dirs,
    };

    for sample in samples {
        counts.files_min = counts.files_min.min(sample.files);
        counts.files_max = counts.files_max.max(sample.files);
        counts.dirs_min = counts.dirs_min.min(sample.dirs);
        counts.dirs_max = counts.dirs_max.max(sample.dirs);
        if sample.files != first.files || sample.dirs != first.dirs {
            counts.match_all = false;
        }
    }

    counts
}

fn latest_shape_from(samples: &[BenchRun]) -> TreeShape {
    let latest = &samples[samples.len() - 1];
    TreeShape {
        tree_nodes: latest.tree_nodes,
        name_bytes: latest.name_bytes,
        empty_dirs: latest.empty_dirs,
        zero_size_dirs: latest.zero_size_dirs,
        dirs_with_files: latest.dirs_with_files,
        leaf_dirs: latest.leaf_dirs,
        single_child_dirs: latest.single_child_dirs,
        parallel_fanout_dirs: latest.parallel_fanout_dirs,
        large_fanout_dirs: latest.large_fanout_dirs,
        huge_fanout_dirs: latest.huge_fanout_dirs,
        max_depth: latest.max_depth,
        max_children: latest.max_children,
        max_child_dirs: latest.max_child_dirs,
    }
}

fn cache_state_for(warmups: usize) -> &'static str {
    if warmups == 0 {
        return constants::bench::CACHE_STATE_NO_BENCHMARK_WARMUP;
    }
    constants::bench::CACHE_STATE_BENCHMARK_WARMED
}

fn build_profile() -> &'static str {
    if cfg!(debug_assertions) {
        return constants::bench::BUILD_PROFILE_DEBUG;
    }
    constants::bench::BUILD_PROFILE_RELEASE
}

fn logical_cpu_count() -> usize {
    std::thread::available_parallelism().map_or(0, |count| count.get())
}

fn excluded_path_strings_from(opts: &ScanOptions) -> Vec<String> {
    opts.excluded_paths
        .iter()
        .map(|path| path.display().to_string())
        .collect()
}

fn percentile_from(sorted_seconds: &[f64], quantile: f64) -> f64 {
    let last_index = sorted_seconds.len().saturating_sub(1);
    let rank = (last_index as f64 * quantile).ceil() as usize;
    sorted_seconds[rank.min(last_index)]
}

#[derive(Clone, Copy, Debug, Default)]
struct TreeTotals {
    files: u64,
    dirs: u64,
    apparent_bytes: u64,
    disk_bytes: u64,
}

#[derive(Clone, Copy, Debug, Default)]
struct TreeShape {
    tree_nodes: u64,
    name_bytes: u64,
    empty_dirs: u64,
    zero_size_dirs: u64,
    dirs_with_files: u64,
    leaf_dirs: u64,
    single_child_dirs: u64,
    parallel_fanout_dirs: u64,
    large_fanout_dirs: u64,
    huge_fanout_dirs: u64,
    max_depth: usize,
    max_children: usize,
    max_child_dirs: usize,
}

impl TreeShape {
    fn from_tree(root: &Node) -> Self {
        let mut shape = Self::default();
        visit_node_shape_for(root, constants::scan::ROOT_DEPTH, true, &mut shape);
        shape
    }
}

struct MeasuredScan {
    elapsed: Duration,
    totals: TreeTotals,
    shape: TreeShape,
    peak_rss_bytes: u64,
}

fn visit_node_shape_for(node: &Node, depth: usize, is_root: bool, shape: &mut TreeShape) {
    shape.tree_nodes += 1;
    shape.name_bytes += node.name.len() as u64;
    shape.max_depth = shape.max_depth.max(depth);
    if !node.is_dir() {
        return;
    }

    update_directory_shape_for(node, is_root, shape);
    for child in &node.children {
        visit_node_shape_for(child, depth + 1, false, shape);
    }
}

fn update_directory_shape_for(node: &Node, is_root: bool, shape: &mut TreeShape) {
    let child_dir_count = node.children.iter().filter(|child| child.is_dir()).count();
    let direct_file_count = node.children.len().saturating_sub(child_dir_count);

    shape.max_children = shape.max_children.max(node.children.len());
    shape.max_child_dirs = shape.max_child_dirs.max(child_dir_count);

    if is_root {
        return;
    }

    update_directory_distribution_for(child_dir_count, direct_file_count, shape);

    if node.children.is_empty() {
        shape.empty_dirs += 1;
    }

    if directory_subtree_is_zero_size(node) {
        shape.zero_size_dirs += 1;
    }
}

fn update_directory_distribution_for(
    child_dir_count: usize,
    direct_file_count: usize,
    shape: &mut TreeShape,
) {
    if direct_file_count > 0 {
        shape.dirs_with_files += 1;
    }

    if child_dir_count == 0 {
        shape.leaf_dirs += 1;
        return;
    }

    if child_dir_count == 1 {
        shape.single_child_dirs += 1;
    }

    if child_dir_count >= constants::scan::PARALLEL_FANOUT {
        shape.parallel_fanout_dirs += 1;
    }

    if child_dir_count >= constants::bench::LARGE_CHILD_DIR_FANOUT {
        shape.large_fanout_dirs += 1;
    }

    if child_dir_count >= constants::bench::HUGE_CHILD_DIR_FANOUT {
        shape.huge_fanout_dirs += 1;
    }
}

fn directory_subtree_is_zero_size(node: &Node) -> bool {
    node.file_count == 0 && node.apparent_size == 0 && node.disk_size == 0
}

fn throughput_per_second(count: u64, seconds: f64) -> f64 {
    count as f64 / seconds.max(constants::bench::MIN_ELAPSED_SECS)
}

fn node_size_bytes() -> u64 {
    std::mem::size_of::<Node>() as u64
}

fn estimated_tree_node_bytes_for(tree_nodes: u64) -> u64 {
    tree_nodes.saturating_mul(node_size_bytes())
}

fn estimated_tree_storage_bytes_for(shape: &TreeShape) -> u64 {
    estimated_tree_node_bytes_for(shape.tree_nodes).saturating_add(shape.name_bytes)
}

struct SampleCounts {
    match_all: bool,
    files_min: u64,
    files_max: u64,
    dirs_min: u64,
    dirs_max: u64,
}

struct MemorySampler {
    #[cfg(not(windows))]
    stop: Arc<AtomicBool>,
    #[cfg(not(windows))]
    handle: Option<JoinHandle<u64>>,
}

impl MemorySampler {
    fn start() -> Self {
        #[cfg(windows)]
        {
            Self {}
        }
        #[cfg(not(windows))]
        {
            Self::start_polling()
        }
    }

    fn source() -> &'static str {
        #[cfg(windows)]
        {
            constants::bench::MEMORY_SOURCE_WINDOWS_PEAK_WORKING_SET
        }
        #[cfg(not(windows))]
        {
            constants::bench::MEMORY_SOURCE_POLLING
        }
    }

    fn sample_interval_ms() -> u64 {
        #[cfg(windows)]
        {
            constants::bench::MEMORY_SAMPLE_MS_NOT_APPLICABLE
        }
        #[cfg(not(windows))]
        {
            constants::bench::MEMORY_SAMPLE_MS
        }
    }

    fn finish(self) -> u64 {
        #[cfg(windows)]
        {
            windows_peak_working_set_bytes().unwrap_or(0)
        }
        #[cfg(not(windows))]
        {
            self.finish_polling()
        }
    }

    #[cfg(not(windows))]
    fn start_polling() -> Self {
        let Ok(pid) = sysinfo::get_current_pid() else {
            return Self::disabled();
        };
        let stop = Arc::new(AtomicBool::new(false));
        let stop_for_thread = Arc::clone(&stop);
        let handle = std::thread::spawn(move || sample_peak_memory_for(pid, stop_for_thread));

        Self {
            stop,
            handle: Some(handle),
        }
    }

    #[cfg(not(windows))]
    fn disabled() -> Self {
        Self {
            stop: Arc::new(AtomicBool::new(true)),
            handle: None,
        }
    }

    #[cfg(not(windows))]
    fn finish_polling(self) -> u64 {
        self.stop.store(true, Ordering::Relaxed);
        let Some(handle) = self.handle else {
            return 0;
        };
        handle.join().unwrap_or(0)
    }
}

#[cfg(windows)]
fn windows_peak_working_set_bytes() -> Option<u64> {
    use windows_sys::Win32::System::ProcessStatus::{
        GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    let mut counters = PROCESS_MEMORY_COUNTERS {
        cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
        PageFaultCount: 0,
        PeakWorkingSetSize: 0,
        WorkingSetSize: 0,
        QuotaPeakPagedPoolUsage: 0,
        QuotaPagedPoolUsage: 0,
        QuotaPeakNonPagedPoolUsage: 0,
        QuotaNonPagedPoolUsage: 0,
        PagefileUsage: 0,
        PeakPagefileUsage: 0,
    };
    let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, counters.cb) };
    if ok == 0 {
        return None;
    }

    Some(counters.PeakWorkingSetSize as u64)
}

#[cfg(not(windows))]
fn sample_peak_memory_for(pid: sysinfo::Pid, stop: Arc<AtomicBool>) -> u64 {
    let mut system = System::new();
    let mut peak = refresh_memory_for(&mut system, pid);

    while !stop.load(Ordering::Relaxed) {
        peak = peak.max(refresh_memory_for(&mut system, pid));
        std::thread::sleep(Duration::from_millis(constants::bench::MEMORY_SAMPLE_MS));
    }

    peak.max(refresh_memory_for(&mut system, pid))
}

#[cfg(not(windows))]
fn refresh_memory_for(system: &mut System, pid: sysinfo::Pid) -> u64 {
    system.refresh_processes_specifics(
        ProcessesToUpdate::Some(&[pid]),
        true,
        ProcessRefreshKind::new().with_memory(),
    );
    system
        .process(pid)
        .map(|process| process.memory())
        .unwrap_or(0)
}
