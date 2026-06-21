//! Parallel filesystem scanner that builds the aggregated size tree.

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use rayon::prelude::*;

use crate::constants;
use crate::node::Node;

/// Options controlling a scan.
pub struct ScanOptions {
    /// Cluster size in bytes used to round up file sizes to their on-disk size.
    /// A value of 0 or 1 disables rounding (on-disk == apparent).
    pub cluster_size: u64,
    /// Follow symlinks / reparse points. Dangerous: may cause cycles.
    pub follow_links: bool,
    /// File or directory paths to skip. Empty means an exact full scan.
    pub excluded_paths: Vec<PathBuf>,
    /// Omit directory subtrees that contain no files and no bytes.
    pub prune_zero_size_dirs: bool,
}

impl ScanOptions {
    pub fn has_excluded_paths(&self) -> bool {
        !self.excluded_paths.is_empty()
    }

    pub fn excludes_path(&self, path: &Path) -> bool {
        if !self.has_excluded_paths() {
            return false;
        }

        self.excluded_paths
            .iter()
            .any(|excluded| path_matches_excluded_path(path, excluded))
    }
}

/// Live progress counters shared with a status thread during a scan.
pub struct ScanProgress {
    enabled: bool,
    pub files: AtomicU64,
    pub dirs: AtomicU64,
    pub bytes: AtomicU64,
    pub errors: AtomicU64,
    pub done: AtomicBool,
}

impl Default for ScanProgress {
    fn default() -> Self {
        Self::enabled()
    }
}

impl ScanProgress {
    pub fn enabled() -> Self {
        Self {
            enabled: true,
            files: AtomicU64::new(0),
            dirs: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            done: AtomicBool::new(false),
        }
    }

    pub fn disabled() -> Self {
        Self {
            enabled: false,
            files: AtomicU64::new(0),
            dirs: AtomicU64::new(0),
            bytes: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            done: AtomicBool::new(false),
        }
    }

    pub fn add_dir(&self) {
        if !self.enabled {
            return;
        }
        self.dirs.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_error(&self) {
        if !self.enabled {
            return;
        }
        self.errors.fetch_add(1, Ordering::Relaxed);
    }

    pub fn add_file_batch(&self, files: u64, bytes: u64) {
        if !self.enabled || files == 0 {
            return;
        }
        self.files.fetch_add(files, Ordering::Relaxed);
        self.bytes.fetch_add(bytes, Ordering::Relaxed);
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn publish_tree(&self, tree: &Node) {
        if !self.enabled {
            return;
        }
        self.files.store(tree.file_count, Ordering::Relaxed);
        self.dirs
            .store(tree.dir_count().saturating_add(1), Ordering::Relaxed);
        self.bytes.store(tree.apparent_size, Ordering::Relaxed);
    }
}

#[inline]
pub fn round_up(size: u64, cluster: u64) -> u64 {
    SizeRounder::from_cluster(cluster).round(size)
}

#[derive(Clone, Copy)]
pub(crate) enum SizeRounder {
    Exact,
    PowerOfTwo { mask: u64 },
    Generic { cluster: u64 },
}

impl SizeRounder {
    pub(crate) fn from_cluster(cluster: u64) -> Self {
        if cluster <= 1 {
            return Self::Exact;
        }
        if cluster.is_power_of_two() {
            return Self::PowerOfTwo { mask: cluster - 1 };
        }
        Self::Generic { cluster }
    }

    #[inline]
    pub(crate) fn round(self, size: u64) -> u64 {
        match self {
            Self::Exact => size,
            Self::PowerOfTwo { mask } => round_up_with_power_of_two_mask(size, mask)
                .unwrap_or_else(|| {
                    round_up_by_division(size, cluster_from_power_of_two_mask(mask))
                }),
            Self::Generic { cluster } => round_up_by_division(size, cluster),
        }
    }
}

fn cluster_from_power_of_two_mask(mask: u64) -> u64 {
    mask + 1
}

#[inline]
fn round_up_with_power_of_two_mask(size: u64, mask: u64) -> Option<u64> {
    if size > u64::MAX - mask {
        return None;
    }

    Some((size + mask) & !mask)
}

#[inline]
fn round_up_by_division(size: u64, cluster: u64) -> u64 {
    if cluster <= 1 {
        return size;
    }
    size.div_ceil(cluster) * cluster
}

/// A completed scan.
#[derive(Debug)]
pub struct ScanOutcome {
    pub tree: Node,
}

impl ScanOutcome {
    pub fn walk(tree: Node) -> Self {
        Self { tree }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScanError {
    message: String,
}

impl ScanError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ScanError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ScanError {}

/// Scan `root` and return the aggregated tree.
pub fn scan(root: &Path, opts: &ScanOptions, progress: &ScanProgress) -> Node {
    let name = root_name_for(root).into_boxed_str();
    let rounder = SizeRounder::from_cluster(opts.cluster_size);
    scan_dir(root, name, opts, rounder, progress)
}

pub(crate) fn root_name_for(root: &Path) -> String {
    root.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| root.to_string_lossy().into_owned())
}

/// A scanner builds the aggregated tree for a root path.
pub trait Scanner {
    fn scan(
        &self,
        root: &Path,
        opts: &ScanOptions,
        progress: &ScanProgress,
    ) -> Result<ScanOutcome, ScanError>;
}

/// The portable engine: a parallel recursive `read_dir` walk. Also serves as
/// the single production scanner.
pub struct WalkScanner;

impl Scanner for WalkScanner {
    fn scan(
        &self,
        root: &Path,
        opts: &ScanOptions,
        progress: &ScanProgress,
    ) -> Result<ScanOutcome, ScanError> {
        Ok(ScanOutcome::walk(scan(root, opts, progress)))
    }
}

/// Build the production scanner.
pub fn scanner_for() -> Box<dyn Scanner> {
    Box::new(WalkScanner)
}

pub fn publish_tree_to_progress(tree: &Node, progress: &ScanProgress) {
    progress.publish_tree(tree);
}

pub(crate) fn publish_file_progress_for(files: &[Node], progress: &ScanProgress) {
    if files.is_empty() || !progress.is_enabled() {
        return;
    }

    let local_bytes = files.iter().map(|file| file.apparent_size).sum();
    progress.add_file_batch(files.len() as u64, local_bytes);
}

/// What a directory entry turned out to be once classified.
enum EntryKind {
    /// A sub-directory to recurse into: its full path and name.
    Dir(PathBuf, Box<str>),
    /// A file, already turned into a leaf node.
    File(Node),
    /// Nothing to record (symlink, reparse point, unreadable, or other kind).
    Skip,
}

enum CollectedEntries {
    Empty,
    Items {
        files: Vec<Node>,
        subdirs: Vec<(PathBuf, Box<str>)>,
    },
}

impl CollectedEntries {
    fn from_parts(files: Vec<Node>, subdirs: Vec<(PathBuf, Box<str>)>) -> Self {
        if files.is_empty() && subdirs.is_empty() {
            return Self::Empty;
        }

        Self::Items { files, subdirs }
    }
}

#[cfg(windows)]
fn is_reparse_point(entry: &fs::DirEntry) -> bool {
    use std::os::windows::fs::MetadataExt;
    match entry.metadata() {
        Ok(metadata) => {
            metadata.file_attributes() & constants::scan::WINDOWS_REPARSE_POINT_ATTRIBUTE != 0
        }
        Err(_) => false,
    }
}

#[cfg(not(windows))]
fn is_reparse_point(_entry: &fs::DirEntry) -> bool {
    false
}

fn classify_entry_without_following_links(
    entry: &fs::DirEntry,
    rounder: SizeRounder,
    progress: &ScanProgress,
) -> EntryKind {
    let Some(file_type) = file_type_from(entry, progress) else {
        return EntryKind::Skip;
    };

    // `is_dir` and `is_file` are false for symlinks, so no-follow symlinks
    // naturally fall through to `Skip` without a separate hot-path branch.
    if file_type.is_dir() {
        return directory_entry_without_following_links_from(entry);
    }

    if file_type.is_file() {
        return file_entry_without_following_links_from(entry, rounder, progress);
    }

    // Other entry kinds (devices, sockets, ...) are ignored.
    EntryKind::Skip
}

fn classify_entry_following_links(
    entry: &fs::DirEntry,
    rounder: SizeRounder,
    progress: &ScanProgress,
) -> EntryKind {
    let path = entry.path();
    classify_path_following_links_from(entry, path, rounder, progress)
}

fn classify_path_following_links_from(
    entry: &fs::DirEntry,
    path: PathBuf,
    rounder: SizeRounder,
    progress: &ScanProgress,
) -> EntryKind {
    let metadata = match fs::metadata(&path) {
        Ok(metadata) => metadata,
        Err(_) => {
            record_scan_error_in(progress);
            return EntryKind::Skip;
        }
    };

    if metadata.is_dir() {
        return EntryKind::Dir(path, entry_name_box_from(entry));
    }

    if metadata.is_file() {
        return file_entry_from(entry, metadata.len(), rounder);
    }

    EntryKind::Skip
}

#[inline]
fn directory_entry_without_following_links_from(entry: &fs::DirEntry) -> EntryKind {
    if is_reparse_point(entry) {
        return EntryKind::Skip;
    }

    EntryKind::Dir(entry.path(), entry_name_box_from(entry))
}

fn classify_entry_without_following_links_with_excludes(
    entry: &fs::DirEntry,
    opts: &ScanOptions,
    rounder: SizeRounder,
    progress: &ScanProgress,
) -> EntryKind {
    let Some(file_type) = file_type_from(entry, progress) else {
        return EntryKind::Skip;
    };

    if file_type.is_dir() {
        return directory_entry_without_following_links_with_excludes_from(entry, opts);
    }

    if file_type.is_file() {
        return file_entry_without_following_links_with_excludes_from(
            entry, opts, rounder, progress,
        );
    }

    EntryKind::Skip
}

fn classify_entry_following_links_with_excludes(
    entry: &fs::DirEntry,
    opts: &ScanOptions,
    rounder: SizeRounder,
    progress: &ScanProgress,
) -> EntryKind {
    let path = entry.path();
    if opts.excludes_path(&path) {
        return EntryKind::Skip;
    }

    classify_path_following_links_from(entry, path, rounder, progress)
}

fn file_type_from(entry: &fs::DirEntry, progress: &ScanProgress) -> Option<fs::FileType> {
    match entry.file_type() {
        Ok(file_type) => Some(file_type),
        Err(_) => {
            record_scan_error_in(progress);
            None
        }
    }
}

fn directory_entry_without_following_links_with_excludes_from(
    entry: &fs::DirEntry,
    opts: &ScanOptions,
) -> EntryKind {
    if is_reparse_point(entry) {
        return EntryKind::Skip;
    }

    let path = entry.path();
    if opts.excludes_path(&path) {
        return EntryKind::Skip;
    }

    EntryKind::Dir(path, entry_name_box_from(entry))
}

#[inline]
fn file_entry_without_following_links_from(
    entry: &fs::DirEntry,
    rounder: SizeRounder,
    progress: &ScanProgress,
) -> EntryKind {
    let len = match entry.metadata() {
        Ok(metadata) => metadata.len(),
        Err(_) => {
            record_scan_error_in(progress);
            return EntryKind::Skip;
        }
    };

    file_entry_from(entry, len, rounder)
}

fn file_entry_without_following_links_with_excludes_from(
    entry: &fs::DirEntry,
    opts: &ScanOptions,
    rounder: SizeRounder,
    progress: &ScanProgress,
) -> EntryKind {
    if opts.excludes_path(&entry.path()) {
        return EntryKind::Skip;
    }

    file_entry_without_following_links_from(entry, rounder, progress)
}

#[inline]
fn file_entry_from(entry: &fs::DirEntry, len: u64, rounder: SizeRounder) -> EntryKind {
    let disk = rounder.round(len);
    EntryKind::File(Node::file(entry_name_from(entry), len, disk))
}

fn entry_name_from(entry: &fs::DirEntry) -> String {
    entry.file_name().to_string_lossy().into_owned()
}

fn entry_name_box_from(entry: &fs::DirEntry) -> Box<str> {
    entry
        .file_name()
        .to_string_lossy()
        .into_owned()
        .into_boxed_str()
}

fn collect_entries_from(
    rd: fs::ReadDir,
    opts: &ScanOptions,
    rounder: SizeRounder,
    progress: &ScanProgress,
) -> CollectedEntries {
    if opts.follow_links {
        if opts.has_excluded_paths() {
            return collect_entries_with_exclusions(
                rd,
                opts,
                rounder,
                progress,
                classify_entry_following_links_with_excludes,
            );
        }

        return collect_entries_with(rd, rounder, progress, classify_entry_following_links);
    }

    if opts.has_excluded_paths() {
        return collect_entries_with_exclusions(
            rd,
            opts,
            rounder,
            progress,
            classify_entry_without_following_links_with_excludes,
        );
    }

    collect_entries_with(
        rd,
        rounder,
        progress,
        classify_entry_without_following_links,
    )
}

fn collect_entries_with<F>(
    mut rd: fs::ReadDir,
    rounder: SizeRounder,
    progress: &ScanProgress,
    classify: F,
) -> CollectedEntries
where
    F: Fn(&fs::DirEntry, SizeRounder, &ScanProgress) -> EntryKind,
{
    let Some(first_entry) = next_readable_entry_from(&mut rd, progress) else {
        return CollectedEntries::Empty;
    };

    let mut files: Vec<Node> = Vec::new();
    let mut subdirs: Vec<(PathBuf, Box<str>)> = Vec::new();
    push_entry_kind_into(
        classify(&first_entry, rounder, progress),
        &mut files,
        &mut subdirs,
    );

    for entry in rd {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                record_scan_error_in(progress);
                continue;
            }
        };

        push_entry_kind_into(
            classify(&entry, rounder, progress),
            &mut files,
            &mut subdirs,
        );
    }

    CollectedEntries::from_parts(files, subdirs)
}

fn collect_entries_with_exclusions<F>(
    mut rd: fs::ReadDir,
    opts: &ScanOptions,
    rounder: SizeRounder,
    progress: &ScanProgress,
    classify: F,
) -> CollectedEntries
where
    F: Fn(&fs::DirEntry, &ScanOptions, SizeRounder, &ScanProgress) -> EntryKind,
{
    let Some(first_entry) = next_readable_entry_from(&mut rd, progress) else {
        return CollectedEntries::Empty;
    };

    let mut files: Vec<Node> = Vec::new();
    let mut subdirs: Vec<(PathBuf, Box<str>)> = Vec::new();
    push_entry_kind_into(
        classify(&first_entry, opts, rounder, progress),
        &mut files,
        &mut subdirs,
    );

    for entry in rd {
        let entry = match entry {
            Ok(entry) => entry,
            Err(_) => {
                record_scan_error_in(progress);
                continue;
            }
        };

        push_entry_kind_into(
            classify(&entry, opts, rounder, progress),
            &mut files,
            &mut subdirs,
        );
    }

    CollectedEntries::from_parts(files, subdirs)
}

fn next_readable_entry_from(rd: &mut fs::ReadDir, progress: &ScanProgress) -> Option<fs::DirEntry> {
    for entry in rd {
        match entry {
            Ok(entry) => return Some(entry),
            Err(_) => record_scan_error_in(progress),
        }
    }

    None
}

fn push_entry_kind_into(
    entry: EntryKind,
    files: &mut Vec<Node>,
    subdirs: &mut Vec<(PathBuf, Box<str>)>,
) {
    match entry {
        EntryKind::Dir(path, name) => subdirs.push((path, name)),
        EntryKind::File(node) => files.push(node),
        EntryKind::Skip => {}
    }
}

/// Sum sizes and counts for a directory from its already-built children.
/// Returns `(apparent, disk, file_count, dir_count)`.
pub(crate) fn aggregate_nodes_for(files: &[Node], child_dirs: &[Node]) -> (u64, u64, u64, u64) {
    let mut apparent = 0u64;
    let mut disk = 0u64;
    let mut file_count = files.len() as u64;
    let mut dir_count = 0u64;

    for f in files {
        apparent += f.apparent_size;
        disk += f.disk_size;
    }
    for d in child_dirs {
        apparent += d.apparent_size;
        disk += d.disk_size;
        file_count += d.file_count;
        dir_count += 1 + d.dir_count();
    }
    (apparent, disk, file_count, dir_count)
}

fn scan_dir(
    path: &Path,
    name: Box<str>,
    opts: &ScanOptions,
    rounder: SizeRounder,
    progress: &ScanProgress,
) -> Node {
    progress.add_dir();

    let rd = match fs::read_dir(path) {
        Ok(rd) => rd,
        Err(_) => {
            // Permission denied or transient error: count and skip.
            record_scan_error_in(progress);
            return Node::empty_dir_with_boxed_name(name);
        }
    };

    let CollectedEntries::Items { files, subdirs } =
        collect_entries_from(rd, opts, rounder, progress)
    else {
        return Node::empty_dir_with_boxed_name(name);
    };

    // Flush this directory's file/byte counts to the shared progress in one
    // batched update per directory, rather than one atomic add per file (which
    // contends badly across worker threads).
    publish_file_progress_for(&files, progress);

    let child_dirs = scan_child_dirs_from(subdirs, opts, rounder, progress);
    let (apparent, disk, file_count, dir_count) = aggregate_nodes_for(&files, &child_dirs);

    let mut children = files;
    children.reserve(child_dirs.len());
    children.extend(child_dirs);

    Node::dir_with_boxed_name(name, apparent, disk, file_count, dir_count, children)
}

#[cold]
fn record_scan_error_in(progress: &ScanProgress) {
    progress.add_error();
}

fn scan_child_dirs_from(
    subdirs: Vec<(PathBuf, Box<str>)>,
    opts: &ScanOptions,
    rounder: SizeRounder,
    progress: &ScanProgress,
) -> Vec<Node> {
    if subdirs.len() < constants::scan::PARALLEL_FANOUT {
        return scan_child_dirs_serially_from(subdirs, opts, rounder, progress);
    }

    scan_child_dirs_in_parallel_from(subdirs, opts, rounder, progress)
}

fn scan_child_dirs_serially_from(
    subdirs: Vec<(PathBuf, Box<str>)>,
    opts: &ScanOptions,
    rounder: SizeRounder,
    progress: &ScanProgress,
) -> Vec<Node> {
    subdirs
        .into_iter()
        .filter_map(|(path, name)| kept_child_dir_from(&path, name, opts, rounder, progress))
        .collect()
}

fn scan_child_dirs_in_parallel_from(
    subdirs: Vec<(PathBuf, Box<str>)>,
    opts: &ScanOptions,
    rounder: SizeRounder,
    progress: &ScanProgress,
) -> Vec<Node> {
    subdirs
        .into_par_iter()
        .filter_map(|(path, name)| kept_child_dir_from(&path, name, opts, rounder, progress))
        .collect()
}

fn kept_child_dir_from(
    path: &Path,
    name: Box<str>,
    opts: &ScanOptions,
    rounder: SizeRounder,
    progress: &ScanProgress,
) -> Option<Node> {
    let node = scan_dir(path, name, opts, rounder, progress);
    if should_prune_child_dir(&node, opts) {
        return None;
    }

    Some(node)
}

fn should_prune_child_dir(node: &Node, opts: &ScanOptions) -> bool {
    opts.prune_zero_size_dirs && node.is_dir() && directory_subtree_is_zero_size(node)
}

fn directory_subtree_is_zero_size(node: &Node) -> bool {
    node.file_count == 0 && node.apparent_size == 0 && node.disk_size == 0
}

fn path_matches_excluded_path(path: &Path, excluded: &Path) -> bool {
    if path == excluded || path.starts_with(excluded) {
        return true;
    }

    path_matches_excluded_path_by_text(path, excluded)
}

#[cfg(windows)]
fn path_matches_excluded_path_by_text(path: &Path, excluded: &Path) -> bool {
    let path_text = comparable_windows_path_for(path);
    let excluded_text = comparable_windows_path_for(excluded);

    path_text == excluded_text
        || path_text
            .strip_prefix(&excluded_text)
            .is_some_and(|tail| tail.starts_with('\\'))
}

#[cfg(not(windows))]
fn path_matches_excluded_path_by_text(_path: &Path, _excluded: &Path) -> bool {
    false
}

#[cfg(windows)]
fn comparable_windows_path_for(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}
