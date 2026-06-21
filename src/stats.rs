//! Derived statistics over a scanned tree: extension breakdown and largest files.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::path::{Path, PathBuf};

use crate::constants;
use crate::metric::Metric;
use crate::node::Node;

/// Aggregated statistics for a single file extension.
pub struct ExtStat {
    pub ext: String,
    /// Logical (apparent) bytes summed over all files with this extension.
    pub apparent: u64,
    pub disk_size: u64,
    pub count: u64,
}

#[derive(Default)]
struct ExtTotals {
    apparent: u64,
    disk_size: u64,
    count: u64,
}

impl ExtStat {
    /// Size of this extension group under the active metric.
    #[inline]
    pub fn size(&self, metric: Metric) -> u64 {
        metric.pick(self.apparent, self.disk_size)
    }
}

/// Build a breakdown of the subtree by file extension, sorted by the active
/// metric (descending).
pub fn ext_breakdown(node: &Node, metric: Metric) -> Vec<ExtStat> {
    let mut map: HashMap<String, ExtTotals> = HashMap::new();
    collect_ext(node, &mut map);
    let mut v: Vec<ExtStat> = map
        .into_iter()
        .map(|(ext, totals)| ExtStat {
            ext,
            apparent: totals.apparent,
            disk_size: totals.disk_size,
            count: totals.count,
        })
        .collect();
    v.sort_by_key(|e| Reverse(e.size(metric)));
    v
}

fn collect_ext(node: &Node, map: &mut HashMap<String, ExtTotals>) {
    if node.is_dir() {
        for c in &node.children {
            collect_ext(c, map);
        }
    } else {
        let ext = ext_of(&node.name);
        let e = map.entry(ext).or_default();
        e.apparent += node.apparent_size;
        e.disk_size += node.disk_size;
        e.count += 1;
    }
}

pub fn ext_of(name: &str) -> String {
    match name.rfind('.') {
        // Require a non-empty stem and a non-empty extension (so dotfiles like
        // ".gitignore" are treated as having no extension).
        Some(i) if i > 0 && i + 1 < name.len() => name[i + 1..].to_ascii_lowercase(),
        _ => constants::format::NO_EXTENSION.to_string(),
    }
}

/// Return the `n` largest files in the subtree, with full paths, largest first.
pub fn top_files_in(node: &Node, base: &Path, n: usize, metric: Metric) -> Vec<(PathBuf, u64)> {
    if n == 0 {
        return Vec::new();
    }
    let mut heap: BinaryHeap<Reverse<(u64, PathBuf)>> = BinaryHeap::with_capacity(n + 1);
    let mut path = base.to_path_buf();
    collect_top(node, &mut path, n, metric, &mut heap);
    let mut v: Vec<(PathBuf, u64)> = heap.into_iter().map(|Reverse((s, p))| (p, s)).collect();
    v.sort_by_key(|entry| Reverse(entry.1));
    v
}

fn collect_top(
    node: &Node,
    path: &mut PathBuf,
    n: usize,
    metric: Metric,
    heap: &mut BinaryHeap<Reverse<(u64, PathBuf)>>,
) {
    if node.is_dir() {
        for c in &node.children {
            path.push(c.name.as_ref());
            collect_top(c, path, n, metric, heap);
            path.pop();
        }
    } else {
        let size = metric.pick(node.apparent_size, node.disk_size);
        push_top_candidate_for(size, path, n, heap);
    }
}

fn push_top_candidate_for(
    size: u64,
    path: &Path,
    n: usize,
    heap: &mut BinaryHeap<Reverse<(u64, PathBuf)>>,
) {
    if heap.len() < n {
        heap.push(Reverse((size, path.to_path_buf())));
        return;
    }

    let Some(Reverse((smallest_size, smallest_path))) = heap.peek() else {
        return;
    };
    if !candidate_beats_smallest(size, path, *smallest_size, smallest_path) {
        return;
    }

    heap.pop();
    heap.push(Reverse((size, path.to_path_buf())));
}

fn candidate_beats_smallest(
    size: u64,
    path: &Path,
    smallest_size: u64,
    smallest_path: &Path,
) -> bool {
    size > smallest_size || (size == smallest_size && path > smallest_path)
}
