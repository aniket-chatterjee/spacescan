//! Command-line interface definition (parsed with `clap`).

use std::path::PathBuf;

use clap::Parser;

use crate::constants;

#[derive(Parser, Debug)]
#[command(
    name = constants::app::NAME,
    version,
    about = constants::app::DESCRIPTION
)]
pub struct Cli {
    /// Directory to scan.
    #[arg(default_value = constants::app::DEFAULT_PATH)]
    pub path: PathBuf,

    /// Cluster size in bytes used to compute on-disk size (0 disables rounding).
    #[arg(long, default_value_t = constants::cli::DEFAULT_CLUSTER_SIZE)]
    pub cluster: u64,

    /// Follow symlinks and reparse points (may cause cycles).
    #[arg(long)]
    pub follow_links: bool,

    /// Exclude a file or directory subtree from the scan. Repeat for multiple paths.
    #[arg(long, value_name = constants::cli::PATH_VALUE_NAME)]
    pub exclude: Vec<PathBuf>,

    /// Omit zero-size directory subtrees from the represented tree.
    #[arg(long)]
    pub prune_zero_size: bool,

    /// Write the full tree to a JSON file.
    #[arg(long, value_name = constants::cli::FILE_VALUE_NAME)]
    pub json: Option<PathBuf>,

    /// Write per-directory rows to a CSV file.
    #[arg(long, value_name = constants::cli::FILE_VALUE_NAME)]
    pub csv: Option<PathBuf>,

    /// Limit exports to directories only (skip individual files).
    #[arg(long)]
    pub dirs_only: bool,

    /// Worker threads (0 = Rayon auto-detect).
    #[arg(long, default_value_t = constants::cli::DEFAULT_THREADS)]
    pub threads: usize,

    /// Print a text report instead of launching the interactive TUI.
    #[arg(long)]
    pub no_tui: bool,

    /// Use on-disk size (instead of apparent) as the default metric.
    #[arg(long)]
    pub disk: bool,

    /// Number of rows in report / breakdown lists.
    #[arg(long, default_value_t = constants::cli::DEFAULT_TOP_ROWS)]
    pub top: usize,

    /// Minimum size for a folder to count as a removable "spot"
    /// (e.g. 100MB, 1.5G, 512k). Filters the reclaim list and app-bundle
    /// detection.
    #[arg(long, value_name = constants::cli::SIZE_VALUE_NAME, default_value = constants::cli::DEFAULT_MIN_SIZE)]
    pub min_size: String,

    /// Write the list of removable clusters to a CSV file.
    #[arg(long, value_name = constants::cli::FILE_VALUE_NAME)]
    pub reclaim_csv: Option<PathBuf>,

    /// Hide the reclaimable-space section in the text report.
    #[arg(long)]
    pub no_reclaim: bool,

    /// Benchmark mode: scan the path this many times and print throughput
    /// counters instead of the report or TUI.
    #[arg(long, value_name = constants::cli::RUNS_VALUE_NAME)]
    pub bench: Option<usize>,

    /// Write benchmark results as machine-readable JSON.
    #[arg(long, value_name = constants::cli::FILE_VALUE_NAME)]
    pub bench_json: Option<PathBuf>,

    /// Warmup scans to run before benchmark samples.
    #[arg(long, default_value_t = constants::bench::DEFAULT_WARMUP_RUNS)]
    pub bench_warmup: usize,
}
