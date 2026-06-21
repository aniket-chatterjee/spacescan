//! Reclaim analysis: find "clusters" of directories that are large and easy to
//! remove (build artifacts, caches, temp, downloads, standalone app folders).
//!
//! The key idea is the **cluster**: when a directory matches a known reclaimable
//! pattern we record the whole subtree as a single removable unit and stop
//! descending. This keeps the number of reported spots small and actionable
//! (one `node_modules` entry, not its 5,000 inner folders).

use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};

use crate::metric::Metric;
use crate::node::Node;

/// How safe a category is to delete.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Safety {
    /// Regenerated automatically by tooling (caches, build output, deps).
    Regenerable,
    /// Disposable junk (temp files, recycle bin, logs).
    Junk,
    /// Reclaimable but holds user data — review before deleting.
    Review,
}

impl Safety {
    pub fn label(self) -> &'static str {
        match self {
            Safety::Regenerable => "regenerable",
            Safety::Junk => "junk",
            Safety::Review => "review",
        }
    }
}

/// A color hint for the UI (kept UI-framework agnostic on purpose).
#[derive(Clone, Copy)]
pub enum Hue {
    Green,
    Cyan,
    Yellow,
    Magenta,
    Blue,
}

/// Static metadata describing a reclaimable category.
#[allow(dead_code)]
pub struct CatMeta {
    pub key: &'static str,
    pub label: &'static str,
    pub badge: &'static str,
    pub safety: Safety,
    pub hue: Hue,
}

/// A reclaimable category. Each variant maps 1:1 (by declaration order) to an
/// entry in [`CATEGORIES`], so `category as usize` is its metadata index.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Category {
    Build,
    Pyenv,
    Pkg,
    Cache,
    Temp,
    Logs,
    Downloads,
    Bundle,
}

impl Category {
    /// All categories in canonical (display) order.
    pub const ALL: [Category; 8] = [
        Category::Build,
        Category::Pyenv,
        Category::Pkg,
        Category::Cache,
        Category::Temp,
        Category::Logs,
        Category::Downloads,
        Category::Bundle,
    ];

    /// Position of this category in [`CATEGORIES`] / [`Category::ALL`].
    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }

    /// Static metadata (label, badge, safety, hue) for this category.
    #[inline]
    pub fn meta(self) -> &'static CatMeta {
        &CATEGORIES[self.index()]
    }
}

pub static CATEGORIES: [CatMeta; 8] = [
    CatMeta {
        key: "build",
        label: "Build / dependencies",
        badge: "BUILD",
        safety: Safety::Regenerable,
        hue: Hue::Green,
    },
    CatMeta {
        key: "pyenv",
        label: "Python envs & caches",
        badge: "PY",
        safety: Safety::Regenerable,
        hue: Hue::Green,
    },
    CatMeta {
        key: "pkgcache",
        label: "Package manager caches",
        badge: "PKG",
        safety: Safety::Regenerable,
        hue: Hue::Cyan,
    },
    CatMeta {
        key: "appcache",
        label: "Application caches",
        badge: "CACHE",
        safety: Safety::Regenerable,
        hue: Hue::Cyan,
    },
    CatMeta {
        key: "temp",
        label: "Temp & recycle bin",
        badge: "TEMP",
        safety: Safety::Junk,
        hue: Hue::Magenta,
    },
    CatMeta {
        key: "logs",
        label: "Log folders",
        badge: "LOGS",
        safety: Safety::Junk,
        hue: Hue::Magenta,
    },
    CatMeta {
        key: "downloads",
        label: "Downloads",
        badge: "DL",
        safety: Safety::Review,
        hue: Hue::Yellow,
    },
    CatMeta {
        key: "bundle",
        label: "Standalone app / data folder",
        badge: "APP?",
        safety: Safety::Review,
        hue: Hue::Blue,
    },
];

const BUILD_NAMES: &[&str] = &[
    "node_modules",
    ".next",
    ".nuxt",
    "bower_components",
    ".svelte-kit",
    ".angular",
    ".turbo",
    ".parcel-cache",
];
const PYENV_NAMES: &[&str] = &[
    "__pycache__",
    ".pytest_cache",
    ".mypy_cache",
    ".ruff_cache",
    ".tox",
    ".venv",
    "venv",
    ".ipynb_checkpoints",
    ".eggs",
];
const PACKAGE_CACHE_NAMES: &[&str] = &[
    ".gradle",
    ".nuget",
    ".m2",
    ".npm",
    ".yarn",
    ".pnpm-store",
    ".ivy2",
];
const APP_CACHE_NAMES: &[&str] = &[
    "cache",
    "caches",
    "code cache",
    "gpucache",
    "cachestorage",
    "shadercache",
    "dxcache",
    "grshadercache",
    "crashpad",
    "blob_storage",
    ".cache",
    "cacheddata",
    "cache_data",
    "service worker",
];
const TEMP_NAMES: &[&str] = &["temp", "tmp", "$recycle.bin", "temporary internet files"];
const LOG_NAMES: &[&str] = &["logs"];
const DOWNLOAD_NAMES: &[&str] = &["downloads"];
const GENERIC_BUILD_NAMES: &[&str] = &["build", "dist", "out"];
const PROJECT_MANIFEST_NAMES: &[&str] = &["package.json", "cargo.toml", "cmakelists.txt"];
const SYSTEM_PATH_NAMES: &[&str] = &[
    "windows",
    "program files",
    "program files (x86)",
    "programdata",
];
const CONTAINER_NAMES: &[&str] = &[
    "users",
    "appdata",
    "local",
    "locallow",
    "roaming",
    "documents",
    "desktop",
    "onedrive",
    "public",
    "downloads",
    "music",
    "pictures",
    "videos",
];
const CARGO_TARGET_NAME: &str = "target";
const CARGO_MANIFEST_NAME: &str = "cargo.toml";
const BIN_DIR_NAME: &str = "bin";
const WINDOWS_EXE_SUFFIX: &str = ".exe";

/// A single removable cluster found in the tree.
pub struct Hotspot {
    pub path: PathBuf,
    pub cat: Category,
    pub apparent: u64,
    pub disk: u64,
    pub files: u64,
    pub dirs: u64,
}

impl Hotspot {
    #[inline]
    pub fn size(&self, metric: Metric) -> u64 {
        metric.pick(self.apparent, self.disk)
    }
}

/// Aggregated totals for one category.
pub struct CatAgg {
    pub cat: Category,
    pub apparent: u64,
    pub disk: u64,
    pub count: u64,
}

impl CatAgg {
    #[inline]
    pub fn size(&self, metric: Metric) -> u64 {
        metric.pick(self.apparent, self.disk)
    }
}

/// Classify a directory by name (with knowledge of its siblings for the few
/// context-sensitive cases). Returns the matching category, or `None`.
pub fn category_for(name: &str, siblings_lc: &HashSet<String>) -> Option<Category> {
    let n = name.to_ascii_lowercase();
    if BUILD_NAMES.contains(&n.as_str()) {
        return Some(Category::Build);
    }
    if PYENV_NAMES.contains(&n.as_str()) {
        return Some(Category::Pyenv);
    }
    if PACKAGE_CACHE_NAMES.contains(&n.as_str()) {
        return Some(Category::Pkg);
    }
    if APP_CACHE_NAMES.contains(&n.as_str()) {
        return Some(Category::Cache);
    }
    if TEMP_NAMES.contains(&n.as_str()) {
        return Some(Category::Temp);
    }
    if LOG_NAMES.contains(&n.as_str()) {
        return Some(Category::Logs);
    }
    if DOWNLOAD_NAMES.contains(&n.as_str()) {
        return Some(Category::Downloads);
    }

    // Context-sensitive: only flag generic build output dirs when a project
    // manifest sits next to them, to avoid false positives.
    if n == CARGO_TARGET_NAME && siblings_lc.contains(CARGO_MANIFEST_NAME) {
        return Some(Category::Build);
    }
    if GENERIC_BUILD_NAMES.contains(&n.as_str()) && has_project_manifest_in(siblings_lc) {
        return Some(Category::Build);
    }

    None
}

fn has_project_manifest_in(siblings_lc: &HashSet<String>) -> bool {
    PROJECT_MANIFEST_NAMES
        .iter()
        .any(|manifest| siblings_lc.contains(*manifest))
}

/// True if any path component is a protected system / install location. Used to
/// keep the "standalone app folder" heuristic away from real installs.
pub fn is_system_path(path: &Path) -> bool {
    for comp in path.components() {
        if let Component::Normal(os) = comp {
            let s = os.to_string_lossy().to_ascii_lowercase();
            if SYSTEM_PATH_NAMES.contains(&s.as_str()) {
                return true;
            }
        }
    }
    false
}

/// Heuristic: does this directory look like a self-contained application bundle?
/// We require an `.exe` *directly* in the folder (the usual layout for a
/// portable app) or directly inside a `bin/` subfolder (SDK layout). This is
/// deliberately strict so that large container folders (a user profile, AppData,
/// Downloads, …) that merely have an exe somewhere deep inside are NOT flagged.
pub fn looks_like_app(dir: &Node) -> bool {
    let has_direct_exe = |d: &Node| {
        d.children
            .iter()
            .any(|c| !c.is_dir() && c.name.to_ascii_lowercase().ends_with(WINDOWS_EXE_SUFFIX))
    };
    if has_direct_exe(dir) {
        return true;
    }
    for c in &dir.children {
        if c.is_dir() && c.name.eq_ignore_ascii_case(BIN_DIR_NAME) && has_direct_exe(c) {
            return true;
        }
    }
    false
}

/// Container folders that should never be treated as a removable "app bundle",
/// even if they happen to contain an executable.
pub fn is_container_name(name: &str) -> bool {
    CONTAINER_NAMES.contains(&name.to_ascii_lowercase().as_str())
}

fn push(out: &mut Vec<Hotspot>, path: &Path, cat: Category, node: &Node) {
    out.push(Hotspot {
        path: path.to_path_buf(),
        cat,
        apparent: node.apparent_size,
        disk: node.disk_size,
        files: node.file_count,
        dirs: node.dir_count(),
    });
}

/// Find all removable clusters under `root` (scanned at `base`).
///
/// `min_size` only gates the heuristic "app bundle" detection (to avoid noise);
/// known categories are always recorded so totals stay complete.
pub fn find_hotspots(root: &Node, base: &Path, min_size: u64) -> Vec<Hotspot> {
    let mut out = Vec::new();
    let mut path = base.to_path_buf();
    walk(root, &mut path, min_size, &mut out);
    out
}

fn walk(dir: &Node, path: &mut PathBuf, min_size: u64, out: &mut Vec<Hotspot>) {
    let siblings: HashSet<String> = dir
        .children
        .iter()
        .map(|c| c.name.to_ascii_lowercase())
        .collect();

    for child in &dir.children {
        if !child.is_dir() {
            continue;
        }
        path.push(child.name.as_ref());
        if should_descend_into(child, path, min_size, out, &siblings) {
            walk(child, path, min_size, out);
        }
        path.pop();
    }
}

fn should_descend_into(
    child: &Node,
    path: &Path,
    min_size: u64,
    out: &mut Vec<Hotspot>,
    siblings: &HashSet<String>,
) -> bool {
    if let Some(cat) = category_for(&child.name, siblings) {
        push(out, path, cat, child);
        return false;
    }

    let big = child.disk_size >= min_size || child.apparent_size >= min_size;
    if big && !is_container_name(&child.name) && !is_system_path(path) && looks_like_app(child) {
        push(out, path, Category::Bundle, child);
        return false;
    }

    true
}

/// Aggregate hotspots by category, sorted by on-disk size descending.
pub fn summarize(hotspots: &[Hotspot]) -> Vec<CatAgg> {
    let mut aggs: Vec<CatAgg> = Category::ALL
        .iter()
        .map(|&cat| CatAgg {
            cat,
            apparent: 0,
            disk: 0,
            count: 0,
        })
        .collect();
    for h in hotspots {
        let a = &mut aggs[h.cat.index()];
        a.apparent += h.apparent;
        a.disk += h.disk;
        a.count += 1;
    }
    aggs.retain(|a| a.count > 0);
    aggs.sort_by_key(|a| std::cmp::Reverse(a.disk));
    aggs
}

/// Total reclaimable bytes across all hotspots, under the active metric.
pub fn total_reclaimable(hotspots: &[Hotspot], metric: Metric) -> u64 {
    hotspots.iter().map(|h| h.size(metric)).sum()
}

/// Indices of `hotspots` at least `min_size` big, ordered largest first (ties
/// keep their original order). Pass `min_size = 0` to include every hotspot.
pub fn indices_ranked_by_size(hotspots: &[Hotspot], metric: Metric, min_size: u64) -> Vec<usize> {
    let mut idx: Vec<usize> = (0..hotspots.len())
        .filter(|&i| hotspots[i].size(metric) >= min_size)
        .collect();
    idx.sort_by(|&a, &b| hotspots[b].size(metric).cmp(&hotspots[a].size(metric)));
    idx
}

/// Total reclaimable bytes broken down by safety level (regenerable, junk, review).
pub fn safety_totals(hotspots: &[Hotspot], metric: Metric) -> (u64, u64, u64) {
    let (mut regen, mut junk, mut review) = (0u64, 0u64, 0u64);
    for h in hotspots {
        let s = h.size(metric);
        match h.cat.meta().safety {
            Safety::Regenerable => regen += s,
            Safety::Junk => junk += s,
            Safety::Review => review += s,
        }
    }
    (regen, junk, review)
}
