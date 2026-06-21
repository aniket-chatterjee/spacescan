//! Unit tests for the `deletion` module's pure logic.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use spacescan::deletion::{
    guard_for, remove_subtree, run_with_progress, DeleteMode, DeleteProgress, Refusal, Removed,
};
use spacescan::node::Node;

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct TempTree(PathBuf);

impl TempTree {
    fn new(label: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should be after epoch")
            .as_nanos();
        Self(std::env::temp_dir().join(format!("spacescan-{label}-{}-{nonce}", std::process::id())))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
        let _ = fs::remove_file(&self.0);
    }
}

fn file(name: &str, apparent: u64, disk: u64) -> Node {
    Node::file(name.to_string(), apparent, disk)
}

fn dir(name: &str, apparent: u64, disk: u64, files: u64, dirs: u64, children: Vec<Node>) -> Node {
    Node::dir_with_children(name.to_string(), apparent, disk, files, dirs, children)
}

/// root(1000/1024, 4f/1d)
///   [0] a.txt 100/128
///   [1] sub  600/640 (2f/0d) -> [x 250/256, y 350/384]
///   [2] b.txt 300/256
fn sample() -> Node {
    let sub = dir(
        "sub",
        600,
        640,
        2,
        0,
        vec![file("x", 250, 256), file("y", 350, 384)],
    );
    dir(
        "root",
        1000,
        1024,
        4,
        1,
        vec![file("a.txt", 100, 128), sub, file("b.txt", 300, 256)],
    )
}

#[test]
fn guard_blocks_root_outside_and_system() {
    let root = Path::new("base");
    assert_eq!(guard_for(root, root), Err(Refusal::ScanRoot));
    assert_eq!(
        guard_for(Path::new("other/x"), root),
        Err(Refusal::ScanRoot)
    );
    assert_eq!(
        guard_for(Path::new("base/Windows/y"), Path::new("base")),
        Err(Refusal::SystemPath)
    );
}

#[test]
fn guard_allows_normal_descendant() {
    assert_eq!(
        guard_for(Path::new("base/proj/target"), Path::new("base")),
        Ok(())
    );
}

#[test]
fn remove_directory_updates_root_totals() -> TestResult {
    let mut root = sample();
    let removed = remove_subtree(&mut root, &[], 1).ok_or("expected directory removal")?;
    assert_eq!(
        removed,
        Removed {
            apparent: 600,
            disk: 640,
            files: 2,
            dirs: 1
        }
    );
    assert_eq!(root.apparent_size, 400);
    assert_eq!(root.disk_size, 384);
    assert_eq!(root.file_count, 2);
    assert_eq!(root.dir_count(), 0);
    assert_eq!(root.children.len(), 2);
    assert_eq!(root.children[0].name.as_ref(), "a.txt");
    assert_eq!(root.children[1].name.as_ref(), "b.txt");
    Ok(())
}

#[test]
fn remove_file_updates_root_totals() -> TestResult {
    let mut root = sample();
    let removed = remove_subtree(&mut root, &[], 0).ok_or("expected file removal")?;
    assert_eq!(
        removed,
        Removed {
            apparent: 100,
            disk: 128,
            files: 1,
            dirs: 0
        }
    );
    assert_eq!(root.apparent_size, 900);
    assert_eq!(root.disk_size, 896);
    assert_eq!(root.file_count, 3);
    assert_eq!(root.dir_count(), 1);
    Ok(())
}

#[test]
fn remove_nested_file_updates_all_ancestors() -> TestResult {
    let mut root = sample();
    // Remove "x" from "sub" (root.children[1]).
    let removed = remove_subtree(&mut root, &[1], 0).ok_or("expected nested file removal")?;
    assert_eq!(
        removed,
        Removed {
            apparent: 250,
            disk: 256,
            files: 1,
            dirs: 0
        }
    );
    // Root totals drop.
    assert_eq!(root.apparent_size, 750);
    assert_eq!(root.disk_size, 768);
    assert_eq!(root.file_count, 3);
    assert_eq!(root.dir_count(), 1);
    // The subdirectory totals drop too.
    let sub = &root.children[1];
    assert_eq!(sub.apparent_size, 350);
    assert_eq!(sub.disk_size, 384);
    assert_eq!(sub.file_count, 1);
    assert_eq!(sub.children.len(), 1);
    assert_eq!(sub.children[0].name.as_ref(), "y");
    Ok(())
}

#[test]
fn remove_out_of_range_returns_none() {
    let mut root = sample();
    assert!(remove_subtree(&mut root, &[], 9).is_none());
    assert!(remove_subtree(&mut root, &[9], 0).is_none());
}

#[test]
fn permanent_delete_reports_progress_for_tree() -> TestResult {
    let temp = TempTree::new("delete-progress");
    fs::create_dir_all(temp.path().join("sub"))?;
    fs::write(temp.path().join("a.txt"), b"a")?;
    fs::write(temp.path().join("sub").join("b.txt"), b"b")?;

    let progress = DeleteProgress::new(4);
    run_with_progress(temp.path(), DeleteMode::Permanent, &progress);
    let result = progress
        .take_result()
        .ok_or("delete did not store a result")?;

    result?;
    assert!(progress.is_finished());
    assert_eq!(progress.done(), 4);
    assert_eq!(progress.fraction(), Some(1.0));
    assert!(!temp.path().exists());
    Ok(())
}
