//! Binary orchestration for scanning, reporting, exporting, benchmarking, and TUI handoff.

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::Ordering;
use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

use crate::bench::{self, BenchRun, BenchSummary};
use crate::cli::Cli;
use crate::constants;
use crate::format::{self, human_size};
use crate::metric::Metric;
use crate::node::Node;
use crate::scan::{scanner_for, ScanError, ScanOptions, ScanProgress, Scanner};
use crate::tui::TuiOutcome;
use crate::{export, reclaim, report, tui};

pub fn run(cli: Cli) -> ExitCode {
    build_thread_pool(cli.threads);

    let root_path = clean_path(&cli.path);
    if !root_path.exists() {
        eprintln!("error: path does not exist: {}", root_path.display());
        return ExitCode::FAILURE;
    }

    let opts = build_options_from(&cli);
    let scanner = scanner_for();

    if let Some(runs) = cli.bench {
        return run_benchmark_from(&cli, scanner.as_ref(), &root_path, &opts, runs);
    }

    run_scan_workflow(&cli, scanner.as_ref(), root_path, opts)
}

pub fn clean_path(path: &Path) -> PathBuf {
    let Ok(canonical) = std::fs::canonicalize(path) else {
        return path.to_path_buf();
    };
    strip_verbatim_prefix_from(canonical)
}

pub fn build_options_from(cli: &Cli) -> ScanOptions {
    ScanOptions {
        cluster_size: cli.cluster,
        follow_links: cli.follow_links,
        excluded_paths: excluded_paths_from(cli),
        prune_zero_size_dirs: cli.prune_zero_size,
    }
}

fn excluded_paths_from(cli: &Cli) -> Vec<PathBuf> {
    cli.exclude.iter().map(|path| clean_path(path)).collect()
}

fn strip_verbatim_prefix_from(path: PathBuf) -> PathBuf {
    let display = path.to_string_lossy();
    if let Some(stripped) = display.strip_prefix(constants::scan::VERBATIM_WINDOWS_PREFIX) {
        return PathBuf::from(stripped);
    }
    path
}

fn build_thread_pool(threads: usize) {
    let mut builder =
        rayon::ThreadPoolBuilder::new().stack_size(constants::scan::DEFAULT_WORKER_STACK_SIZE);
    if let Some(worker_threads) = worker_threads_for(threads) {
        builder = builder.num_threads(worker_threads);
    }
    let _ = builder.build_global();
}

pub fn worker_threads_for(requested_threads: usize) -> Option<usize> {
    if requested_threads > constants::cli::DEFAULT_THREADS {
        return Some(requested_threads);
    }

    None
}

fn run_benchmark_from(
    cli: &Cli,
    scanner: &dyn Scanner,
    root: &Path,
    opts: &ScanOptions,
    runs: usize,
) -> ExitCode {
    let summary = match bench::run_benchmark(
        scanner,
        root,
        opts,
        runs,
        cli.bench_warmup,
        cli.threads,
        |sample| print_benchmark_sample(sample, runs),
    ) {
        Ok(summary) => summary,
        Err(error) => {
            eprintln!("error: scan failed: {error}");
            return ExitCode::FAILURE;
        }
    };

    print_benchmark_summary(&summary);
    if let Some(path) = &cli.bench_json {
        if let Err(error) = bench::write_summary_json(&summary, path) {
            eprintln!("error writing benchmark JSON: {error}");
            return ExitCode::FAILURE;
        }
        println!("Wrote benchmark JSON: {}", path.display());
    }
    ExitCode::SUCCESS
}

fn print_benchmark_sample(sample: &BenchRun, runs: usize) {
    println!(
        "  run {}/{}: {:.6}s  {}: {}",
        sample.index,
        runs,
        sample.elapsed_seconds,
        constants::scan::ENGINE_STATUS_LABEL,
        sample.engine
    );
}

fn print_benchmark_summary(summary: &BenchSummary) {
    println!(
        "bench: {} runs  min {:.6}s  median {:.6}s  p95 {:.6}s  |  {} files  {} dirs  {} {}  {:.0} {}  {:.0} {}  {:.0} {} (median)  |  {} {}  {} {}  {} {}  peak RSS {}  |  {}: {}",
        summary.runs,
        summary.min_seconds,
        summary.median_seconds,
        summary.p95_seconds,
        summary.files,
        summary.dirs,
        summary.tree_nodes,
        constants::bench::TREE_NODES_LABEL,
        summary.files_per_second,
        constants::bench::FILES_PER_SECOND_LABEL,
        summary.dirs_per_second,
        constants::bench::DIRS_PER_SECOND_LABEL,
        summary.tree_nodes_per_second,
        constants::bench::TREE_NODES_PER_SECOND_LABEL,
        constants::bench::ESTIMATED_TREE_NODE_BYTES_LABEL,
        human_size(summary.estimated_tree_node_bytes),
        constants::bench::TREE_NAME_BYTES_LABEL,
        human_size(summary.name_bytes),
        constants::bench::ESTIMATED_TREE_STORAGE_BYTES_LABEL,
        human_size(summary.estimated_tree_storage_bytes),
        human_size(summary.peak_rss_bytes),
        constants::scan::ENGINE_STATUS_LABEL,
        summary.engine
    );
}

fn run_scan_workflow(
    cli: &Cli,
    scanner: &dyn Scanner,
    root_path: PathBuf,
    opts: ScanOptions,
) -> ExitCode {
    eprintln!("Scanning {} ...", root_path.display());
    let scan = match scan_with_progress(scanner, &root_path, &opts) {
        Ok(scan) => scan,
        Err(error) => {
            eprintln!("error: scan failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    print_scan_summary(&scan.tree, scan.errors);

    run_exports_for(cli, &scan.tree, &root_path);

    let min_size = match parse_min_size_from(cli) {
        Ok(size) => size,
        Err(error) => {
            eprintln!("error: invalid --min-size: {error}");
            return ExitCode::FAILURE;
        }
    };

    run_reclaim(cli, scanner, &opts, scan.tree, root_path, min_size)
}

pub fn scan_with_progress(
    scanner: &dyn Scanner,
    root: &Path,
    opts: &ScanOptions,
) -> Result<CompletedScan, ScanError> {
    let progress = ScanProgress::default();
    let progress_bar = progress_bar_for_scan();
    let progress_bar_for_thread = progress_bar.clone();

    let outcome = std::thread::scope(|scope| {
        let progress_ref = &progress;
        scope.spawn(move || tick_progress_for(progress_ref, progress_bar_for_thread));
        let outcome = scanner.scan(root, opts, &progress);
        progress.done.store(true, Ordering::Relaxed);
        outcome
    })?;

    progress_bar.finish_and_clear();
    let errors = progress.errors.load(Ordering::Relaxed);
    Ok(CompletedScan {
        tree: outcome.tree,
        errors,
    })
}

fn progress_bar_for_scan() -> ProgressBar {
    let progress_bar = ProgressBar::new_spinner();
    if let Ok(style) = ProgressStyle::with_template(constants::scan::PROGRESS_TEMPLATE) {
        progress_bar.set_style(style.tick_strings(&constants::scan::SPINNER_TICKS));
    }
    progress_bar
}

fn tick_progress_for(progress: &ScanProgress, progress_bar: ProgressBar) {
    while !progress.done.load(Ordering::Relaxed) {
        progress_bar.set_message(progress_message_for(progress));
        progress_bar.tick();
        std::thread::sleep(Duration::from_millis(constants::scan::PROGRESS_REFRESH_MS));
    }
}

fn progress_message_for(progress: &ScanProgress) -> String {
    format!(
        "{} files  {} dirs  {}  ({} skipped)",
        progress.files.load(Ordering::Relaxed),
        progress.dirs.load(Ordering::Relaxed),
        human_size(progress.bytes.load(Ordering::Relaxed)),
        progress.errors.load(Ordering::Relaxed),
    )
}

fn print_scan_summary(tree: &Node, errors: u64) {
    eprintln!(
        "Scanned {} files in {} dirs - {} apparent, {} on disk{}",
        tree.file_count,
        tree.dir_count(),
        human_size(tree.apparent_size),
        human_size(tree.disk_size),
        skipped_entries_suffix_for(errors)
    );
}

fn skipped_entries_suffix_for(errors: u64) -> String {
    if errors == 0 {
        return String::new();
    }
    format!(" ({errors} entries skipped: no access)")
}

fn run_exports_for(cli: &Cli, tree: &Node, root: &Path) {
    write_optional_export(
        cli.json.as_deref(),
        |path| export::write_json(tree, root, path, cli.dirs_only),
        "JSON",
    );

    write_optional_export(
        cli.csv.as_deref(),
        |path| export::write_csv(tree, root, path),
        "CSV",
    );
}

fn write_optional_export(
    path: Option<&Path>,
    write: impl FnOnce(&Path) -> std::io::Result<()>,
    label: &str,
) {
    let Some(path) = path else {
        return;
    };
    match write(path) {
        Ok(()) => eprintln!("Wrote {label}: {}", path.display()),
        Err(error) => eprintln!("error writing {label}: {error}"),
    }
}

fn parse_min_size_from(cli: &Cli) -> Result<u64, String> {
    format::parse_size(&cli.min_size)
}

fn run_reclaim(
    cli: &Cli,
    scanner: &dyn Scanner,
    opts: &ScanOptions,
    mut tree: Node,
    mut root: PathBuf,
    min_size: u64,
) -> ExitCode {
    let metric = Metric::from_on_disk(cli.disk);
    let mut hotspots = reclaim::find_hotspots(&tree, &root, min_size);
    print_reclaim_summary_for(&hotspots, metric);
    write_optional_reclaim_export(cli, &hotspots, metric);

    if cli.no_tui {
        print_text_reports_for(cli, &tree, &root, &hotspots, metric, min_size);
        return ExitCode::SUCCESS;
    }

    loop {
        match run_tui_once(tree, &root, metric, hotspots, min_size) {
            TuiStep::Quit => return ExitCode::SUCCESS,
            TuiStep::Failure => return ExitCode::FAILURE,
            TuiStep::Rescan(next_root) => {
                let Some(next) = rescan_tree_for(scanner, opts, next_root) else {
                    return ExitCode::FAILURE;
                };
                root = next.root;
                tree = next.tree;
                hotspots = reclaim::find_hotspots(&tree, &root, min_size);
            }
        }
    }
}

fn print_reclaim_summary_for(hotspots: &[reclaim::Hotspot], metric: Metric) {
    let reclaimable = reclaim::total_reclaimable(hotspots, metric);
    eprintln!(
        "Reclaimable: {} across {} removable clusters (press 'r' in the TUI)",
        human_size(reclaimable),
        hotspots.len()
    );
}

fn write_optional_reclaim_export(cli: &Cli, hotspots: &[reclaim::Hotspot], metric: Metric) {
    let Some(path) = &cli.reclaim_csv else {
        return;
    };
    match export::write_reclaim_csv(hotspots, metric, path) {
        Ok(()) => eprintln!("Wrote reclaim CSV: {}", path.display()),
        Err(error) => eprintln!("error writing reclaim CSV: {error}"),
    }
}

fn print_text_reports_for(
    cli: &Cli,
    tree: &Node,
    root: &Path,
    hotspots: &[reclaim::Hotspot],
    metric: Metric,
    min_size: u64,
) {
    let aggs = reclaim::summarize(hotspots);
    report::print_report(tree, root, cli.top, metric);
    if !cli.no_reclaim {
        report::print_reclaim(hotspots, &aggs, cli.top, metric, min_size);
    }
}

fn run_tui_once(
    tree: Node,
    root: &Path,
    metric: Metric,
    hotspots: Vec<reclaim::Hotspot>,
    min_size: u64,
) -> TuiStep {
    let aggs = reclaim::summarize(&hotspots);
    match tui::run(tree, root.to_path_buf(), metric, hotspots, aggs, min_size) {
        Ok(TuiOutcome::Quit) => TuiStep::Quit,
        Ok(TuiOutcome::Rescan(path)) => TuiStep::Rescan(path),
        Err(error) => {
            eprintln!("TUI error: {error}");
            TuiStep::Failure
        }
    }
}

fn rescan_tree_for(
    scanner: &dyn Scanner,
    opts: &ScanOptions,
    path: PathBuf,
) -> Option<RescanResult> {
    let root = clean_path(&path);
    if !root.exists() {
        eprintln!("error: path does not exist: {}", root.display());
        return None;
    }
    eprintln!("Scanning {} ...", root.display());
    let scan = match scan_with_progress(scanner, &root, opts) {
        Ok(scan) => scan,
        Err(error) => {
            eprintln!("error: scan failed: {error}");
            return None;
        }
    };
    print_scan_summary(&scan.tree, scan.errors);
    Some(RescanResult {
        root,
        tree: scan.tree,
    })
}

enum TuiStep {
    Quit,
    Rescan(PathBuf),
    Failure,
}

struct RescanResult {
    root: PathBuf,
    tree: Node,
}

pub struct CompletedScan {
    pub tree: Node,
    pub errors: u64,
}
