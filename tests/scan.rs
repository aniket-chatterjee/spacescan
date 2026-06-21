//! Unit tests for the `scan` module's pure helpers.

use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use spacescan::constants;
use spacescan::scan::{round_up, scanner_for, ScanOptions, ScanProgress};

type TestResult = Result<(), Box<dyn std::error::Error>>;

#[test]
fn round_up_passes_through_when_rounding_disabled() {
    assert_eq!(round_up(100, 0), 100);
    assert_eq!(round_up(100, 1), 100);
}

#[test]
fn round_up_rounds_to_cluster_multiples() {
    assert_eq!(round_up(0, 4096), 0);
    assert_eq!(round_up(1, 4096), 4096);
    assert_eq!(round_up(4096, 4096), 4096);
    assert_eq!(round_up(4097, 4096), 8192);
}

#[test]
fn round_up_handles_non_power_of_two_clusters() {
    assert_eq!(round_up(0, 10), 0);
    assert_eq!(round_up(1, 10), 10);
    assert_eq!(round_up(10, 10), 10);
    assert_eq!(round_up(11, 10), 20);
}

#[test]
fn walk_engine_excludes_directory_subtree() -> TestResult {
    let root = temp_scan_fixture_for("spacescan-exclude")?;
    let excluded = root.join("skip");
    fs::create_dir_all(excluded.join("nested"))?;
    fs::write(excluded.join("nested").join("ignored.bin"), b"ignored")?;

    let opts = ScanOptions {
        cluster_size: constants::cli::DEFAULT_CLUSTER_SIZE,
        follow_links: false,
        excluded_paths: vec![excluded],
        prune_zero_size_dirs: false,
    };
    let outcome = scanner_for().scan(&root, &opts, &ScanProgress::default())?;
    let _ = fs::remove_dir_all(root);

    assert_eq!(outcome.tree.file_count, 3);
    assert_eq!(outcome.tree.dir_count(), 3);
    assert!(!outcome
        .tree
        .children
        .iter()
        .any(|child| child.name.as_ref() == "skip"));
    Ok(())
}

#[test]
fn walk_engine_prunes_zero_size_directory_subtrees() -> TestResult {
    let root = temp_scan_fixture_for("spacescan-prune-zero")?;
    fs::create_dir_all(root.join("empty-parent").join("empty-child"))?;

    let opts = ScanOptions {
        cluster_size: constants::cli::DEFAULT_CLUSTER_SIZE,
        follow_links: false,
        excluded_paths: Vec::new(),
        prune_zero_size_dirs: true,
    };
    let outcome = scanner_for().scan(&root, &opts, &ScanProgress::default())?;
    let _ = fs::remove_dir_all(root);

    assert_eq!(outcome.tree.file_count, 3);
    assert_eq!(outcome.tree.dir_count(), 3);
    assert!(!outcome
        .tree
        .children
        .iter()
        .any(|child| child.name.as_ref() == "empty-parent"));
    Ok(())
}

fn temp_scan_fixture_for(prefix: &str) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let root = empty_temp_scan_root_for(prefix)?;
    write_basic_scan_fixture_to(&root)?;
    Ok(root)
}

fn empty_temp_scan_root_for(
    prefix: &str,
) -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = std::env::temp_dir().join(format!("{prefix}-{stamp}"));
    Ok(root)
}

fn write_basic_scan_fixture_to(root: &Path) -> Result<(), Box<dyn std::error::Error>> {
    fs::create_dir_all(root.join("alpha").join("nested"))?;
    fs::create_dir_all(root.join("beta"))?;
    fs::write(root.join("alpha").join("a.txt"), b"alpha")?;
    fs::write(root.join("alpha").join("nested").join("b.txt"), b"nested")?;
    fs::write(root.join("beta").join("c.bin"), b"beta")?;
    Ok(())
}
