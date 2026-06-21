//! The scanned filesystem tree node type.

use crate::constants;
use crate::metric::Metric;
use thin_vec::ThinVec;

/// A node in the scanned filesystem tree.
///
/// A node is either a file or a directory. Sizes and counts on a directory are
/// aggregated over the whole subtree rooted at that directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// File or directory name (the last path component only).
    /// Names are immutable after scan construction and stored as boxed strings
    /// so the finalized tree does not retain unused `String` capacity.
    pub name: Box<str>,
    /// Logical size in bytes (sum over the subtree for directories).
    pub apparent_size: u64,
    /// On-disk size in bytes (apparent size rounded up to the cluster size).
    pub disk_size: u64,
    /// Total number of files contained in the subtree.
    pub file_count: u64,
    /// Total number of sub-directories contained in the subtree.
    dir_count: u64,
    /// Children discovered during the scan. Views and exports apply their own
    /// ordering at the boundary where users observe the tree. The scan stores
    /// finalized children as a thin vector so millions of file leaves do not
    /// carry unused vector capacity or a fat slice pointer.
    pub children: ThinVec<Node>,
}

impl Node {
    pub fn file(name: String, apparent: u64, disk: u64) -> Self {
        Self::file_with_boxed_name(name.into_boxed_str(), apparent, disk)
    }

    pub(crate) fn file_with_boxed_name(name: Box<str>, apparent: u64, disk: u64) -> Self {
        Self {
            name,
            apparent_size: apparent,
            disk_size: disk,
            file_count: 0,
            dir_count: constants::node::FILE_DIR_COUNT_SENTINEL,
            children: ThinVec::new(),
        }
    }

    pub fn empty_dir(name: String) -> Self {
        Self::empty_dir_with_boxed_name(name.into_boxed_str())
    }

    pub(crate) fn empty_dir_with_boxed_name(name: Box<str>) -> Self {
        Self {
            name,
            apparent_size: 0,
            disk_size: 0,
            file_count: 0,
            dir_count: 0,
            children: ThinVec::new(),
        }
    }

    pub fn dir_with_children(
        name: String,
        apparent_size: u64,
        disk_size: u64,
        file_count: u64,
        dir_count: u64,
        children: Vec<Node>,
    ) -> Self {
        Self::dir_with_boxed_name(
            name.into_boxed_str(),
            apparent_size,
            disk_size,
            file_count,
            dir_count,
            children,
        )
    }

    pub(crate) fn dir_with_boxed_name(
        name: Box<str>,
        apparent_size: u64,
        disk_size: u64,
        file_count: u64,
        dir_count: u64,
        children: Vec<Node>,
    ) -> Self {
        Self {
            name,
            apparent_size,
            disk_size,
            file_count,
            dir_count,
            children: ThinVec::from(children),
        }
    }

    /// Returns the size to use for the active metric.
    #[inline]
    pub fn size(&self, metric: Metric) -> u64 {
        metric.pick(self.apparent_size, self.disk_size)
    }

    #[inline]
    pub fn is_dir(&self) -> bool {
        self.dir_count != constants::node::FILE_DIR_COUNT_SENTINEL
    }

    #[inline]
    pub fn dir_count(&self) -> u64 {
        if self.is_dir() {
            return self.dir_count;
        }

        0
    }

    pub(crate) fn subtract_totals_by(&mut self, apparent: u64, disk: u64, files: u64, dirs: u64) {
        self.apparent_size = self.apparent_size.saturating_sub(apparent);
        self.disk_size = self.disk_size.saturating_sub(disk);
        self.file_count = self.file_count.saturating_sub(files);
        if self.is_dir() {
            self.dir_count = self.dir_count.saturating_sub(dirs);
        }
    }
}
