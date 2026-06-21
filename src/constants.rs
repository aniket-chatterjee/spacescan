//! Shared constants for defaults, labels, filenames, and UI dimensions.
//!
//! Keeping reusable literals here makes behavior easier to audit and keeps the
//! calling code focused on intent rather than incidental strings or numbers.

pub mod app {
    pub const NAME: &str = "spacescan";
    pub const DESCRIPTION: &str = "Fast parallel disk-usage analyzer with an interactive TUI";
    pub const DEFAULT_PATH: &str = ".";
}

pub mod cli {
    pub const DEFAULT_CLUSTER_SIZE: u64 = 4096;
    pub const DEFAULT_THREADS: usize = 0;
    pub const DEFAULT_TOP_ROWS: usize = 20;
    pub const DEFAULT_MIN_SIZE: &str = "100MB";
    pub const SIZE_VALUE_NAME: &str = "SIZE";
    pub const FILE_VALUE_NAME: &str = "FILE";
    pub const PATH_VALUE_NAME: &str = "PATH";
    pub const RUNS_VALUE_NAME: &str = "RUNS";
}

pub mod scan {
    pub const DEFAULT_WORKER_STACK_SIZE: usize = 16 * 1024 * 1024;
    pub const ROOT_DEPTH: usize = 0;
    pub const PROGRESS_REFRESH_MS: u64 = 90;
    pub const PARALLEL_FANOUT: usize = 4;
    pub const WINDOWS_UTF8_CODE_PAGE: u32 = 65001;
    pub const WINDOWS_REPARSE_POINT_ATTRIBUTE: u32 = 0x0000_0400;
    pub const VERBATIM_WINDOWS_PREFIX: &str = r"\\?\";
    pub const VERBATIM_WINDOWS_UNC_PREFIX: &str = r"\\?\UNC\";
    pub const WINDOWS_UNC_PREFIX: &str = r"\\";
    pub const ENGINE_WALK: &str = "walk";
    pub const ENGINE_STATUS_LABEL: &str = "engine";
    pub const PROGRESS_TEMPLATE: &str = "{spinner:.green} scanning  {msg}";
    pub const SPINNER_TICKS: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
}

pub mod node {
    pub const FILE_DIR_COUNT_SENTINEL: u64 = u64::MAX;
}

pub mod bench {
    pub const DEFAULT_WARMUP_RUNS: usize = 1;
    pub const MIN_RUNS: usize = 1;
    pub const P95_QUANTILE: f64 = 0.95;
    pub const MIN_ELAPSED_SECS: f64 = 1e-9;
    pub const LARGE_CHILD_DIR_FANOUT: usize = 256;
    pub const HUGE_CHILD_DIR_FANOUT: usize = 16_384;
    pub const MEMORY_SAMPLE_MS: u64 = 100;
    pub const MEMORY_SAMPLE_MS_NOT_APPLICABLE: u64 = 0;
    pub const MEMORY_SOURCE_POLLING: &str = "polling";
    pub const MEMORY_SOURCE_WINDOWS_PEAK_WORKING_SET: &str = "windows_peak_working_set";
    pub const CACHE_STATE_NO_BENCHMARK_WARMUP: &str = "no_benchmark_warmup";
    pub const CACHE_STATE_BENCHMARK_WARMED: &str = "benchmark_warmed";
    pub const BUILD_PROFILE_DEBUG: &str = "debug";
    pub const BUILD_PROFILE_RELEASE: &str = "release";
    pub const FILES_PER_SECOND_LABEL: &str = "files/sec";
    pub const DIRS_PER_SECOND_LABEL: &str = "dirs/sec";
    pub const TREE_NODES_LABEL: &str = "tree nodes";
    pub const TREE_NODES_PER_SECOND_LABEL: &str = "nodes/sec";
    pub const ESTIMATED_TREE_NODE_BYTES_LABEL: &str = "est node bytes";
    pub const TREE_NAME_BYTES_LABEL: &str = "name bytes";
    pub const ESTIMATED_TREE_STORAGE_BYTES_LABEL: &str = "est tree bytes";
}

pub mod format {
    pub const SIZE_UNITS: [&str; 7] = ["B", "KB", "MB", "GB", "TB", "PB", "EB"];
    pub const BINARY_UNIT: f64 = 1024.0;
    pub const NO_EXTENSION: &str = "(none)";
    pub const DEFAULT_SLUG: &str = "scan";
}

pub mod export {
    pub const TREE_CSV_HEADER: &str = "path,apparent_bytes,disk_bytes,files,dirs";
    pub const RECLAIM_CSV_HEADER: &str =
        "category,safety,apparent_bytes,disk_bytes,files,dirs,path";
}

pub mod files {
    pub const EXPORT_JSON_PREFIX: &str = "spacescan-";
    pub const EXPORT_JSON_EXT: &str = ".json";
    pub const EXPORT_CSV_EXT: &str = ".csv";
    pub const RECLAIM_EXPORT_CSV: &str = "spacescan-reclaim.csv";
    pub const SCAN_REPORT_GLOB: &str = "scan-report*.txt";
}

pub mod report {
    pub const ASCII_BAR_WIDTH: usize = 10;
    pub const NONE_ROW: &str = "  (none)";
    pub const NONE_ABOVE_THRESHOLD_ROW: &str = "    (none above threshold)";
    pub const NO_RECLAIM_CLUSTERS: &str = "  No reclaimable clusters detected.";
}

pub mod tui {
    pub const BROWSER_BAR_WIDTH: usize = 12;
    pub const RECLAIM_BAR_WIDTH: usize = 10;
    pub const SUMMARY_BAR_WIDTH: usize = 8;
    pub const TOP_FILES_LIMIT: usize = 25;
    pub const FILE_TYPES_LIMIT: usize = 64;
    pub const EVENT_POLL_MS: u64 = 250;
    pub const DELETE_EVENT_POLL_MS: u64 = 80;
    pub const MOUSE_ROW_CHROME: u16 = 2;
    pub const HEAT_HIGH: f64 = 0.66;
    pub const HEAT_MID: f64 = 0.33;
    pub const HELP_WIDTH: u16 = 66;
    pub const HELP_HEIGHT: u16 = 22;
    pub const CONFIRM_WIDTH: u16 = 66;
    pub const CONFIRM_HEIGHT: u16 = 13;
    pub const DELETE_CHOICE_HEIGHT: u16 = 12;
    pub const DELETE_PROGRESS_WIDTH: u16 = 70;
    pub const DELETE_PROGRESS_HEIGHT: u16 = 9;
    pub const DELETE_PROGRESS_BAR_WIDTH: usize = 34;
    pub const PICKER_WIDTH: u16 = 70;
    pub const PICKER_HEIGHT: u16 = 20;
    /// Concise, always-visible key hints for the browser footer. The full key
    /// list lives in the help overlay (`?`).
    pub const BROWSER_HINTS: &[(&str, &str)] = &[
        ("↑↓", "move"),
        ("→", "open"),
        ("←", "up"),
        ("/", "filter"),
        ("d", "delete"),
        ("r", "reclaim"),
        ("?", "help"),
        ("q", "quit"),
    ];
    /// Concise key hints for the reclaim footer.
    pub const RECLAIM_HINTS: &[(&str, &str)] = &[
        ("↑↓", "move"),
        ("O", "open"),
        ("e", "export"),
        ("r", "back"),
        ("?", "help"),
        ("q", "quit"),
    ];
    pub const EMPTY_DIRECTORY: &str = "(empty directory)";
    pub const FILTER_CURSOR: &str = "_";
}

pub mod messages {
    pub const EXPORT_FAILED: &str = "Export failed (check write permissions in current folder)";
    pub const RECLAIM_EXPORT_FAILED: &str = "Reclaim export failed (check write permissions)";
    pub const TYPE_EXACT_NAME: &str = "type the exact name to confirm";
    pub const DELETE_CHOICE_CONFIRM: &str = "Press t for Trash, p for permanent, Esc to cancel";
    pub const RECOVERABLE_DELETE: &str = "Recoverable from the Recycle Bin / Trash.";
    pub const PERMANENT_DELETE_WARNING: &str = "This CANNOT be undone.";
    pub const TRASH_CONFIRM: &str = "Press y to confirm, n or Esc to cancel";
    pub const PERMANENT_CONFIRM: &str = "Enter to delete, Esc to cancel";
    pub const TRASH_ABORTED_HINT: &str =
        "move to Trash failed (too large, locked, or path too long)";
    pub const TRASH_FALLBACK_PROMPT: &str =
        "move to Trash failed; confirm permanent delete or Esc to cancel";
}

pub mod platform {
    pub const WINDOWS_FILE_MANAGER: &str = "explorer";
    pub const MACOS_FILE_MANAGER: &str = "open";
    pub const UNIX_FILE_MANAGER: &str = "xdg-open";
}
