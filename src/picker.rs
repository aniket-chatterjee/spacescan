//! Interactive chooser for the directory or drive to scan.
//!
//! The navigation logic ([`Picker`]) is pure and driven through an
//! [`EntrySource`], so it can be unit tested with a fake source. The real
//! [`FsSource`] lists drives (via `sysinfo`), a few bookmarks, and directories.

use std::path::{Path, PathBuf};

use crate::util::clamp_index;

/// What a picker row represents.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EntryKind {
    /// Go to the parent of the current directory (or back to the root list).
    Up,
    /// A drive / volume root.
    Drive,
    /// A bookmarked location (home, downloads, ...).
    Bookmark,
    /// A normal sub-directory.
    Dir,
}

/// One selectable row.
#[derive(Clone, Debug)]
pub struct Entry {
    pub label: String,
    pub path: PathBuf,
    pub kind: EntryKind,
}

impl Entry {
    pub fn new(label: impl Into<String>, path: impl Into<PathBuf>, kind: EntryKind) -> Self {
        Entry {
            label: label.into(),
            path: path.into(),
            kind,
        }
    }
}

/// Supplies the rows shown by the picker. Abstracted so navigation is testable
/// without touching the real filesystem.
pub trait EntrySource {
    /// The top-level list: drives + bookmarks.
    fn roots(&self) -> Vec<Entry>;
    /// Sub-directories of `dir` (the picker prepends an `Up` row itself).
    fn children(&self, dir: &Path) -> Vec<Entry>;
}

/// A directory/drive picker with a current location and a selection.
pub struct Picker {
    /// `None` while showing the root list; otherwise the directory being browsed.
    location: Option<PathBuf>,
    entries: Vec<Entry>,
    selected: usize,
}

impl Picker {
    /// Open the picker at the root list (drives + bookmarks).
    pub fn new(src: &dyn EntrySource) -> Self {
        Picker {
            location: None,
            entries: src.roots(),
            selected: 0,
        }
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    pub fn location(&self) -> Option<&Path> {
        self.location.as_deref()
    }

    pub fn move_by(&mut self, delta: isize) {
        self.selected = clamp_index(self.selected, self.entries.len(), delta);
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }

    /// Activate the selected row: descend into a directory/drive, or go up.
    pub fn enter(&mut self, src: &dyn EntrySource) {
        let Some(entry) = self.entries.get(self.selected) else {
            return;
        };
        match entry.kind {
            EntryKind::Up => self.up(src),
            EntryKind::Drive | EntryKind::Bookmark | EntryKind::Dir => {
                let path = entry.path.clone();
                self.show_dir(&path, src);
            }
        }
    }

    /// Go to the parent directory, or back to the root list at the top level.
    pub fn up(&mut self, src: &dyn EntrySource) {
        match self.location.as_deref().and_then(Path::parent) {
            Some(parent) => {
                let parent = parent.to_path_buf();
                self.show_dir(&parent, src);
            }
            None => {
                self.location = None;
                self.entries = src.roots();
                self.selected = 0;
            }
        }
    }

    fn show_dir(&mut self, dir: &Path, src: &dyn EntrySource) {
        let mut entries = vec![Entry::new("..", dir, EntryKind::Up)];
        entries.extend(src.children(dir));
        self.location = Some(dir.to_path_buf());
        self.entries = entries;
        self.selected = 0;
    }

    /// The directory that would be scanned if the user confirms now: the
    /// selected directory, or the current location for the `Up`/`..` row.
    pub fn target(&self) -> Option<PathBuf> {
        let entry = self.selected_entry()?;
        match entry.kind {
            EntryKind::Up => self.location.clone(),
            _ => Some(entry.path.clone()),
        }
    }
}

/// The real filesystem source: drives via `sysinfo`, bookmarks, and `read_dir`.
pub struct FsSource;

impl EntrySource for FsSource {
    fn roots(&self) -> Vec<Entry> {
        let mut out = Vec::new();
        let disks = sysinfo::Disks::new_with_refreshed_list();
        for disk in &disks {
            let mount = disk.mount_point();
            out.push(Entry::new(
                format!("{} (drive)", mount.display()),
                mount,
                EntryKind::Drive,
            ));
        }
        for (label, path) in bookmarks() {
            out.push(Entry::new(label, path, EntryKind::Bookmark));
        }
        out
    }

    fn children(&self, dir: &Path) -> Vec<Entry> {
        let mut entries: Vec<Entry> = match std::fs::read_dir(dir) {
            Ok(rd) => rd
                .flatten()
                .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                .map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    Entry::new(name, e.path(), EntryKind::Dir)
                })
                .collect(),
            Err(_) => Vec::new(),
        };
        entries.sort_by_key(|entry| entry.label.to_lowercase());
        entries
    }
}

/// Common starting points, in display order (only those that exist).
fn bookmarks() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let mut add = |label: &str, path: Option<PathBuf>| {
        if let Some(p) = path {
            if p.exists() {
                out.push((label.to_string(), p));
            }
        }
    };
    let home = home_dir();
    add("Home", home.clone());
    if let Some(h) = &home {
        add("Downloads", Some(h.join("Downloads")));
        add("Desktop", Some(h.join("Desktop")));
        add("Documents", Some(h.join("Documents")));
    }
    add("Current dir", std::env::current_dir().ok());
    out
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}
