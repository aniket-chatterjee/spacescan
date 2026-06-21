use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use spacescan::bench::run_benchmark;
use spacescan::node::Node;
use spacescan::scan::{ScanOptions, ScanOutcome, ScanProgress, Scanner};

struct FakeScanner;
struct DriftScanner {
    calls: AtomicUsize,
}

impl Scanner for FakeScanner {
    fn scan(
        &self,
        _root: &Path,
        _opts: &ScanOptions,
        _progress: &ScanProgress,
    ) -> Result<ScanOutcome, spacescan::scan::ScanError> {
        let children = vec![
            Node::file("a.txt".to_string(), 4, 8),
            Node::file("b.txt".to_string(), 6, 8),
            Node::empty_dir("empty".to_string()),
        ];
        let tree = Node::dir_with_children("root".to_string(), 10, 16, 2, 1, children);
        Ok(ScanOutcome::walk(tree))
    }
}

impl DriftScanner {
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
        }
    }
}

impl Scanner for DriftScanner {
    fn scan(
        &self,
        _root: &Path,
        _opts: &ScanOptions,
        _progress: &ScanProgress,
    ) -> Result<ScanOutcome, spacescan::scan::ScanError> {
        let files = 2 + self.calls.fetch_add(1, Ordering::Relaxed) as u64;
        let tree = Node::dir_with_children("root".to_string(), 10, 16, files, 1, Vec::new());
        Ok(ScanOutcome::walk(tree))
    }
}

#[test]
fn benchmark_summary_contains_machine_readable_counters() -> Result<(), Box<dyn std::error::Error>>
{
    let opts = ScanOptions {
        cluster_size: 4096,
        follow_links: false,
        excluded_paths: Vec::new(),
        prune_zero_size_dirs: false,
    };
    let mut sample_count = 0;

    let summary = run_benchmark(&FakeScanner, Path::new("C:/repo"), &opts, 3, 1, 0, |_| {
        sample_count += 1
    })?;

    assert_eq!(sample_count, 3);
    assert_eq!(summary.runs, 3);
    assert_eq!(summary.warmups, 1);
    assert_eq!(summary.engine, "walk");
    assert!(summary.worker_threads >= 1);
    assert!(summary.logical_cpus >= 1);
    assert!(!summary.build_profile.is_empty());
    assert_eq!(summary.cache_state, "benchmark_warmed");
    assert!(summary.excluded_paths.is_empty());
    assert!(!summary.prune_zero_size_dirs);
    #[cfg(windows)]
    {
        assert_eq!(summary.memory_source, "windows_peak_working_set");
        assert_eq!(summary.memory_sample_ms, 0);
    }
    #[cfg(not(windows))]
    {
        assert_eq!(summary.memory_source, "polling");
        assert_eq!(summary.memory_sample_ms, 100);
    }
    assert_eq!(summary.files, 2);
    assert_eq!(summary.dirs, 1);
    assert_eq!(summary.tree_nodes, 4);
    assert_eq!(summary.apparent_bytes, 10);
    assert_eq!(summary.disk_bytes, 16);
    assert_eq!(summary.empty_dirs, 1);
    assert_eq!(summary.zero_size_dirs, 1);
    assert_eq!(summary.dirs_with_files, 0);
    assert_eq!(summary.leaf_dirs, 1);
    assert_eq!(summary.single_child_dirs, 0);
    assert_eq!(summary.parallel_fanout_dirs, 0);
    assert_eq!(summary.large_fanout_dirs, 0);
    assert_eq!(summary.huge_fanout_dirs, 0);
    assert_eq!(summary.max_depth, 1);
    assert_eq!(summary.max_children, 3);
    assert_eq!(summary.max_child_dirs, 1);
    assert!(summary.sample_counts_match);
    assert_eq!(summary.sample_files_min, 2);
    assert_eq!(summary.sample_files_max, 2);
    assert_eq!(summary.sample_dirs_min, 1);
    assert_eq!(summary.sample_dirs_max, 1);
    assert!(summary.files_per_second > 0.0);
    assert!(summary.dirs_per_second > 0.0);
    assert!(summary.tree_nodes_per_second > 0.0);
    assert_eq!(summary.node_size_bytes, std::mem::size_of::<Node>() as u64);
    assert_eq!(
        summary.estimated_tree_node_bytes,
        summary.tree_nodes * summary.node_size_bytes
    );
    assert_eq!(summary.name_bytes, 19);
    assert_eq!(
        summary.estimated_tree_storage_bytes,
        summary.estimated_tree_node_bytes + summary.name_bytes
    );
    assert_eq!(summary.samples.len(), 3);
    assert_eq!(summary.samples[0].files, 2);
    assert_eq!(summary.samples[0].dirs, 1);
    assert_eq!(summary.samples[0].tree_nodes, 4);
    assert_eq!(summary.samples[0].apparent_bytes, 10);
    assert_eq!(summary.samples[0].disk_bytes, 16);
    assert_eq!(summary.samples[0].empty_dirs, 1);
    assert_eq!(summary.samples[0].zero_size_dirs, 1);
    assert_eq!(summary.samples[0].dirs_with_files, summary.dirs_with_files);
    assert_eq!(summary.samples[0].leaf_dirs, summary.leaf_dirs);
    assert_eq!(
        summary.samples[0].single_child_dirs,
        summary.single_child_dirs
    );
    assert_eq!(
        summary.samples[0].parallel_fanout_dirs,
        summary.parallel_fanout_dirs
    );
    assert_eq!(
        summary.samples[0].large_fanout_dirs,
        summary.large_fanout_dirs
    );
    assert_eq!(
        summary.samples[0].huge_fanout_dirs,
        summary.huge_fanout_dirs
    );
    assert_eq!(summary.samples[0].max_depth, 1);
    assert_eq!(summary.samples[0].max_children, 3);
    assert_eq!(summary.samples[0].max_child_dirs, 1);
    assert!(summary.samples[0].dirs_per_second > 0.0);
    assert!(summary.samples[0].tree_nodes_per_second > 0.0);
    assert_eq!(summary.samples[0].node_size_bytes, summary.node_size_bytes);
    assert_eq!(
        summary.samples[0].estimated_tree_node_bytes,
        summary.samples[0].tree_nodes * summary.samples[0].node_size_bytes
    );
    assert_eq!(summary.samples[0].name_bytes, summary.name_bytes);
    assert_eq!(
        summary.samples[0].estimated_tree_storage_bytes,
        summary.samples[0].estimated_tree_node_bytes + summary.samples[0].name_bytes
    );
    assert_eq!(summary.samples[0].engine, "walk");
    assert!(summary.peak_rss_bytes >= summary.samples[0].peak_rss_bytes);
    Ok(())
}

#[test]
fn benchmark_summary_reports_sample_count_drift() -> Result<(), Box<dyn std::error::Error>> {
    let opts = ScanOptions {
        cluster_size: 4096,
        follow_links: false,
        excluded_paths: Vec::new(),
        prune_zero_size_dirs: false,
    };
    let scanner = DriftScanner::new();

    let summary = run_benchmark(&scanner, Path::new("C:/repo"), &opts, 3, 0, 0, |_| {})?;

    assert!(!summary.sample_counts_match);
    assert_eq!(summary.sample_files_min, 2);
    assert_eq!(summary.sample_files_max, 4);
    assert_eq!(summary.sample_dirs_min, 1);
    assert_eq!(summary.sample_dirs_max, 1);
    assert_eq!(summary.files, 4);
    assert_eq!(summary.samples[0].files, 2);
    assert_eq!(summary.samples[1].files, 3);
    assert_eq!(summary.samples[2].files, 4);
    Ok(())
}
