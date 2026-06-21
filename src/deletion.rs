//! In-TUI deletion: safety guards, the actual remove, and the in-memory tree
//! fix-up so sizes stay correct without a full re-scan.
//!
//! The pure parts (`guard_for`, `remove_subtree`) carry the risk and are unit
//! tested; `run_with_progress` performs the actual removal (the only side
//! effect) on a worker thread, reporting live progress to the UI.

use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use crate::node::Node;
use crate::reclaim::is_system_path;

/// Where a delete sends its target.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DeleteMode {
    /// Move to the OS recycle bin / trash (recoverable).
    Trash,
    /// Remove permanently (unrecoverable).
    Permanent,
}

impl DeleteMode {
    pub fn label(self) -> &'static str {
        match self {
            DeleteMode::Trash => "Trash",
            DeleteMode::Permanent => "PERMANENTLY delete",
        }
    }
}

/// Why a delete was refused by the guard.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Refusal {
    /// The target is the scan root (or outside it).
    ScanRoot,
    /// The target sits under a protected system location.
    SystemPath,
}

impl Refusal {
    pub fn reason(self) -> &'static str {
        match self {
            Refusal::ScanRoot => "refusing to delete the scan root",
            Refusal::SystemPath => "refusing to delete a protected system path",
        }
    }
}

/// Decide whether `target` may be deleted, given the `scan_root`. Blocks the
/// root itself, anything outside it, and protected system locations.
pub fn guard_for(target: &Path, scan_root: &Path) -> Result<(), Refusal> {
    if target == scan_root || !target.starts_with(scan_root) {
        return Err(Refusal::ScanRoot);
    }
    if is_system_path(target) {
        return Err(Refusal::SystemPath);
    }
    Ok(())
}

/// Totals removed from the tree by a deletion (used to fix up ancestors).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Removed {
    pub apparent: u64,
    pub disk: u64,
    pub files: u64,
    pub dirs: u64,
}

fn node_ref<'a>(root: &'a Node, path: &[usize]) -> Option<&'a Node> {
    let mut n = root;
    for &i in path {
        n = n.children.get(i)?;
    }
    Some(n)
}

fn node_ref_mut<'a>(root: &'a mut Node, path: &[usize]) -> Option<&'a mut Node> {
    let mut n = root;
    for &i in path {
        n = n.children.get_mut(i)?;
    }
    Some(n)
}

/// Remove child `child_idx` from the directory at `parent_path`, subtracting its
/// totals from every ancestor so the aggregated tree stays consistent. Returns
/// what was removed, or `None` if the indices are out of range.
pub fn remove_subtree(root: &mut Node, parent_path: &[usize], child_idx: usize) -> Option<Removed> {
    let removed = {
        let parent = node_ref(root, parent_path)?;
        let child = parent.children.get(child_idx)?;
        Removed {
            apparent: child.apparent_size,
            disk: child.disk_size,
            // A file contributes 1 to its parent's file_count; a directory
            // contributes its own file_count and (itself + its subdirs).
            files: if child.is_dir() { child.file_count } else { 1 },
            dirs: if child.is_dir() {
                1 + child.dir_count()
            } else {
                0
            },
        }
    };
    // Subtract from the root and every ancestor down to the parent (inclusive).
    for k in 0..=parent_path.len() {
        let node = node_ref_mut(root, &parent_path[..k])?;
        node.subtract_totals_by(removed.apparent, removed.disk, removed.files, removed.dirs);
    }
    remove_child_from(node_ref_mut(root, parent_path)?, child_idx)?;
    Some(removed)
}

fn remove_child_from(parent: &mut Node, child_idx: usize) -> Option<Node> {
    let mut children = Vec::from(std::mem::take(&mut parent.children));
    if child_idx >= children.len() {
        parent.children = children.into();
        return None;
    }

    let removed = children.remove(child_idx);
    parent.children = children.into();
    Some(removed)
}

/// Live progress for an in-flight delete, shared between the worker thread and
/// the UI. A `total` of `0` means the magnitude is unknown (indeterminate),
/// which is the case for Recycle Bin deletes since the OS reports no progress.
pub struct DeleteProgress {
    done: AtomicU64,
    total: u64,
    finished: AtomicBool,
    result: Mutex<Option<io::Result<()>>>,
}

impl DeleteProgress {
    pub fn new(total: u64) -> Self {
        Self {
            done: AtomicU64::new(0),
            total,
            finished: AtomicBool::new(false),
            result: Mutex::new(None),
        }
    }

    /// Files/directories removed so far (only meaningful when `total > 0`).
    pub fn done(&self) -> u64 {
        self.done.load(Ordering::Relaxed)
    }

    /// Total filesystem items expected, or `0` when indeterminate.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// Completion fraction in `0.0..=1.0`, or `None` when indeterminate.
    pub fn fraction(&self) -> Option<f64> {
        if self.total == 0 {
            return None;
        }
        Some((self.done() as f64 / self.total as f64).clamp(0.0, 1.0))
    }

    /// Whether the worker has finished (successfully or not).
    pub fn is_finished(&self) -> bool {
        self.finished.load(Ordering::Acquire)
    }

    /// Take the worker's result once finished; `None` if not stored yet.
    pub fn take_result(&self) -> Option<io::Result<()>> {
        self.result.lock().unwrap().take()
    }

    fn add(&self, n: u64) {
        self.done.fetch_add(n, Ordering::Relaxed);
    }
}

/// Run a delete to completion on the calling thread, updating `progress`. The
/// progress is *always* marked finished on return — even on panic — so the UI
/// can never wait forever. Intended to be called from a worker thread.
pub fn run_with_progress(target: &Path, mode: DeleteMode, progress: &DeleteProgress) {
    // Ensure `finished` is set however this function exits (including panic).
    struct FinishGuard<'a>(&'a DeleteProgress);
    impl Drop for FinishGuard<'_> {
        fn drop(&mut self) {
            self.0.finished.store(true, Ordering::Release);
        }
    }
    let _guard = FinishGuard(progress);

    let result = match mode {
        DeleteMode::Trash => trash::delete(target).map_err(io::Error::other),
        DeleteMode::Permanent => remove_counting(target, progress),
    };
    *progress.result.lock().unwrap() = Some(result);
}

/// Permanent recursive delete that counts each removed file/directory into
/// `progress`.
/// Symbolic links and junctions are never followed: the link itself is removed.
fn remove_counting(target: &Path, progress: &DeleteProgress) -> io::Result<()> {
    let meta = std::fs::symlink_metadata(target)?;
    if meta.file_type().is_symlink() {
        remove_link(target)?;
        progress.add(1);
        return Ok(());
    }
    if !meta.is_dir() {
        std::fs::remove_file(target)?;
        progress.add(1);
        return Ok(());
    }
    // Real directory: use the canonical (verbatim on Windows) form so deeply
    // nested paths beyond MAX_PATH still resolve.
    let dir = std::fs::canonicalize(target)?;
    remove_dir_counting(&dir, progress)
}

fn remove_dir_counting(dir: &Path, progress: &DeleteProgress) -> io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            remove_link(&path)?;
            progress.add(1);
        } else if ft.is_dir() {
            remove_dir_counting(&path, progress)?;
        } else {
            std::fs::remove_file(&path)?;
            progress.add(1);
        }
    }
    std::fs::remove_dir(dir)?;
    progress.add(1);
    Ok(())
}

/// Remove a symlink/junction without following it. On Windows a directory
/// symlink or junction must be removed with `remove_dir`, files with
/// `remove_file`; try the file form first and fall back.
fn remove_link(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(_) => std::fs::remove_dir(path),
    }
}
