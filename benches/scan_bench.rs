use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use spacescan::constants;
use spacescan::export::write_csv_to;
use spacescan::metric::Metric;
use spacescan::reclaim::{find_hotspots, summarize};
use spacescan::report::{reclaim_lines_for, report_lines_for};
use spacescan::scan::{scan, ScanOptions, ScanProgress};
use spacescan::util::{matches_filter, FilterMatcher};

const WIDE_DIRS: usize = 32;
const WIDE_FILES_PER_DIR: usize = 16;
const FILE_ONLY_FILES: usize = 512;
const DIR_ONLY_DIRS: usize = 512;
const DIR_FANOUT_DIRS: usize = 4096;
const HUGE_DIR_FANOUT_DIRS: usize = 16_384;
const NESTED_FANOUT_DIRS: usize = 4096;
const NESTED_FANOUT_DEPTH: usize = 3;
const NESTED_FANOUT_MARKER_INTERVAL: usize = 16;
const FILTER_NAME_COUNT: usize = 4096;
const DEEP_LEVELS: usize = 64;
const MIXED_PROJECTS: usize = 12;
const MIXED_SOURCE_FILES: usize = 16;
const MIXED_CACHE_FILES: usize = 24;
const RECLAIM_PROJECTS: usize = 24;
const REPORT_TOP_ROWS: usize = 20;
const RECLAIM_MIN_SIZE: u64 = 1;
const FIXTURE_BYTES: &[u8] = b"0123456789";
const SCAN_FILES_ONLY_BENCH: &str = "scan_files_only_tree";
const SCAN_DIRS_ONLY_BENCH: &str = "scan_dirs_only_tree";
const SCAN_DIR_FANOUT_BENCH: &str = "scan_dir_fanout_tree";
const SCAN_HUGE_DIR_FANOUT_BENCH: &str = "scan_huge_dir_fanout_tree";
const SCAN_NESTED_FANOUT_BENCH: &str = "scan_nested_fanout_tree";
const SCAN_WIDE_BENCH: &str = "scan_wide_tree";
const SCAN_DEEP_BENCH: &str = "scan_deep_tree";
const SCAN_MIXED_BENCH: &str = "scan_mixed_tree";
const EXPORT_MIXED_BENCH: &str = "export_csv_mixed_tree";
const REPORT_MIXED_BENCH: &str = "report_mixed_tree";
const RECLAIM_HEAVY_BENCH: &str = "reclaim_heavy_tree";
const FILTER_UNCACHED_BENCH: &str = "filter_names_uncached";
const FILTER_CACHED_BENCH: &str = "filter_names_cached";
const SORT_NAMES_UNCACHED_BENCH: &str = "sort_names_uncached";
const SORT_NAMES_CACHED_BENCH: &str = "sort_names_cached";
const FILTER_NEEDLE: &str = "MODULE";

fn fixture_root_for(name: &str) -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    std::env::temp_dir().join(format!("spacescan-bench-{name}-{stamp}"))
}

fn build_wide_fixture_at(root: &Path) {
    create_dir_or_exit(root, "wide fixture root");
    for dir_index in 0..WIDE_DIRS {
        let dir = root.join(format!("dir-{dir_index:02}"));
        create_dir_or_exit(&dir, "wide fixture dir");
        for file_index in 0..WIDE_FILES_PER_DIR {
            write_file_or_exit(
                &dir.join(format!("file-{file_index:02}.bin")),
                FIXTURE_BYTES,
            );
        }
    }
}

fn build_files_only_fixture_at(root: &Path) {
    create_dir_or_exit(root, "files-only fixture root");
    for file_index in 0..FILE_ONLY_FILES {
        write_file_or_exit(
            &root.join(format!("file-{file_index:03}.bin")),
            FIXTURE_BYTES,
        );
    }
}

fn build_dirs_only_fixture_at(root: &Path) {
    create_dir_or_exit(root, "dirs-only fixture root");
    create_numbered_dirs_in(root, "dir", DIR_ONLY_DIRS, "dirs-only fixture dir");
}

fn build_dir_fanout_fixture_at(root: &Path) {
    create_dir_or_exit(root, "dir-fanout fixture root");
    create_numbered_dirs_in(root, "fanout", DIR_FANOUT_DIRS, "dir-fanout fixture dir");
}

fn build_huge_dir_fanout_fixture_at(root: &Path) {
    create_dir_or_exit(root, "huge dir-fanout fixture root");
    create_numbered_dirs_in(
        root,
        "huge-fanout",
        HUGE_DIR_FANOUT_DIRS,
        "huge dir-fanout fixture dir",
    );
}

fn build_nested_fanout_fixture_at(root: &Path) {
    create_dir_or_exit(root, "nested fanout fixture root");
    for dir_index in 0..NESTED_FANOUT_DIRS {
        let leaf = create_nested_child_chain_in(root, dir_index);
        if should_write_nested_marker_for(dir_index) {
            write_file_or_exit(&leaf.join("marker.bin"), FIXTURE_BYTES);
        }
    }
}

fn create_nested_child_chain_in(root: &Path, dir_index: usize) -> PathBuf {
    let mut current = root.join(format!("nested-fanout-{dir_index:04}"));
    create_dir_or_exit(&current, "nested fanout child dir");
    for depth in 0..NESTED_FANOUT_DEPTH {
        current = current.join(format!("nested-{depth:02}"));
        create_dir_or_exit(&current, "nested fanout chain dir");
    }

    current
}

fn should_write_nested_marker_for(dir_index: usize) -> bool {
    dir_index % NESTED_FANOUT_MARKER_INTERVAL == 0
}

fn build_deep_fixture_at(root: &Path) {
    let mut current = root.to_path_buf();
    for depth in 0..DEEP_LEVELS {
        current = current.join(format!("level-{depth:02}"));
        create_dir_or_exit(&current, "deep fixture dir");
        write_file_or_exit(&current.join("data.bin"), FIXTURE_BYTES);
    }
}

fn build_mixed_fixture_at(root: &Path) {
    create_dir_or_exit(root, "mixed fixture root");
    for project_index in 0..MIXED_PROJECTS {
        let project = root.join(format!("project-{project_index:02}"));
        let src = project.join("src");
        let cache = project.join(".cache");
        let target = project.join("target");
        create_dir_or_exit(&src, "mixed source dir");
        create_dir_or_exit(&cache, "mixed cache dir");
        create_dir_or_exit(&target, "mixed target dir");
        write_file_or_exit(&project.join("Cargo.toml"), FIXTURE_BYTES);

        for file_index in 0..MIXED_SOURCE_FILES {
            write_file_or_exit(
                &src.join(format!("module-{file_index:02}.rs")),
                FIXTURE_BYTES,
            );
        }
        for file_index in 0..MIXED_CACHE_FILES {
            write_file_or_exit(
                &cache.join(format!("blob-{file_index:02}.bin")),
                FIXTURE_BYTES,
            );
            write_file_or_exit(
                &target.join(format!("object-{file_index:02}.o")),
                FIXTURE_BYTES,
            );
        }
    }
}

fn build_reclaim_fixture_at(root: &Path) {
    create_dir_or_exit(root, "reclaim fixture root");
    for project_index in 0..RECLAIM_PROJECTS {
        let project = root.join(format!("workspace-{project_index:02}"));
        create_dir_or_exit(&project, "reclaim project dir");
        write_file_or_exit(&project.join("package.json"), FIXTURE_BYTES);
        write_fixture_files_in(&project.join("node_modules"), "dep", MIXED_CACHE_FILES);
        write_fixture_files_in(&project.join(".pytest_cache"), "py", MIXED_CACHE_FILES);
        write_fixture_files_in(&project.join("logs"), "log", MIXED_SOURCE_FILES);
    }
}

fn write_fixture_files_in(path: &Path, prefix: &str, count: usize) {
    create_dir_or_exit(path, "fixture file dir");
    for file_index in 0..count {
        write_file_or_exit(
            &path.join(format!("{prefix}-{file_index:02}.bin")),
            FIXTURE_BYTES,
        );
    }
}

fn create_numbered_dirs_in(root: &Path, prefix: &str, count: usize, label: &str) {
    for dir_index in 0..count {
        create_dir_or_exit(&root.join(format!("{prefix}-{dir_index:04}")), label);
    }
}

fn filter_names() -> Vec<String> {
    (0..FILTER_NAME_COUNT)
        .map(|name_index| format!("project-{name_index:04}-module-cache.bin"))
        .collect()
}

fn create_dir_or_exit(path: &Path, label: &str) {
    if let Err(error) = fs::create_dir_all(path) {
        eprintln!("failed to create {label} at {}: {error}", path.display());
        std::process::exit(1);
    }
}

fn write_file_or_exit(path: &Path, bytes: &[u8]) {
    if let Err(error) = fs::write(path, bytes) {
        eprintln!("failed to write fixture file {}: {error}", path.display());
        std::process::exit(1);
    }
}

fn scan_fixture(root: &Path) {
    let opts = ScanOptions {
        cluster_size: constants::cli::DEFAULT_CLUSTER_SIZE,
        follow_links: false,
        excluded_paths: Vec::new(),
        prune_zero_size_dirs: false,
    };
    let progress = ScanProgress::disabled();
    let _ = scan(root, &opts, &progress);
}

fn scan_tree_for(root: &Path) -> spacescan::node::Node {
    let opts = ScanOptions {
        cluster_size: constants::cli::DEFAULT_CLUSTER_SIZE,
        follow_links: false,
        excluded_paths: Vec::new(),
        prune_zero_size_dirs: false,
    };
    let progress = ScanProgress::disabled();
    scan(root, &opts, &progress)
}

fn criterion_benchmark(c: &mut Criterion) {
    let names = filter_names();
    c.bench_function(FILTER_UNCACHED_BENCH, |b| {
        b.iter(|| {
            let matches = names
                .iter()
                .filter(|name| matches_filter(black_box(name), black_box(FILTER_NEEDLE)))
                .count();
            black_box(matches);
        })
    });
    c.bench_function(FILTER_CACHED_BENCH, |b| {
        b.iter(|| {
            let matcher = FilterMatcher::for_filter(black_box(FILTER_NEEDLE));
            let matches = names
                .iter()
                .filter(|name| matcher.matches(black_box(name)))
                .count();
            black_box(matches);
        })
    });
    c.bench_function(SORT_NAMES_UNCACHED_BENCH, |b| {
        b.iter(|| {
            let mut indices: Vec<usize> = (0..names.len()).collect();
            indices.sort_by(|&left, &right| {
                names[left].to_lowercase().cmp(&names[right].to_lowercase())
            });
            black_box(indices);
        })
    });
    c.bench_function(SORT_NAMES_CACHED_BENCH, |b| {
        b.iter(|| {
            let mut indices: Vec<usize> = (0..names.len()).collect();
            indices.sort_by_cached_key(|&index| names[index].to_lowercase());
            black_box(indices);
        })
    });

    let files_only = fixture_root_for("files-only");
    build_files_only_fixture_at(&files_only);
    c.bench_function(SCAN_FILES_ONLY_BENCH, |b| {
        b.iter(|| scan_fixture(&files_only))
    });
    let _ = fs::remove_dir_all(&files_only);

    let dirs_only = fixture_root_for("dirs-only");
    build_dirs_only_fixture_at(&dirs_only);
    c.bench_function(SCAN_DIRS_ONLY_BENCH, |b| {
        b.iter(|| scan_fixture(&dirs_only))
    });
    let _ = fs::remove_dir_all(&dirs_only);

    let dir_fanout = fixture_root_for("dir-fanout");
    build_dir_fanout_fixture_at(&dir_fanout);
    c.bench_function(SCAN_DIR_FANOUT_BENCH, |b| {
        b.iter(|| scan_fixture(&dir_fanout))
    });
    let _ = fs::remove_dir_all(&dir_fanout);

    let huge_dir_fanout = fixture_root_for("huge-dir-fanout");
    build_huge_dir_fanout_fixture_at(&huge_dir_fanout);
    c.bench_function(SCAN_HUGE_DIR_FANOUT_BENCH, |b| {
        b.iter(|| scan_fixture(&huge_dir_fanout))
    });
    let _ = fs::remove_dir_all(&huge_dir_fanout);

    let nested_fanout = fixture_root_for("nested-fanout");
    build_nested_fanout_fixture_at(&nested_fanout);
    c.bench_function(SCAN_NESTED_FANOUT_BENCH, |b| {
        b.iter(|| scan_fixture(&nested_fanout))
    });
    let _ = fs::remove_dir_all(&nested_fanout);

    let wide = fixture_root_for("wide");
    build_wide_fixture_at(&wide);
    c.bench_function(SCAN_WIDE_BENCH, |b| b.iter(|| scan_fixture(&wide)));
    let _ = fs::remove_dir_all(&wide);

    let deep = fixture_root_for("deep");
    build_deep_fixture_at(&deep);
    c.bench_function(SCAN_DEEP_BENCH, |b| b.iter(|| scan_fixture(&deep)));
    let _ = fs::remove_dir_all(&deep);

    let mixed = fixture_root_for("mixed");
    build_mixed_fixture_at(&mixed);
    c.bench_function(SCAN_MIXED_BENCH, |b| b.iter(|| scan_fixture(&mixed)));
    let mixed_tree = scan_tree_for(&mixed);
    c.bench_function(EXPORT_MIXED_BENCH, |b| {
        b.iter(|| {
            let mut out = Vec::new();
            write_csv_to(black_box(&mixed_tree), black_box(&mixed), &mut out).unwrap();
            black_box(out);
        })
    });
    c.bench_function(REPORT_MIXED_BENCH, |b| {
        b.iter(|| {
            black_box(report_lines_for(
                black_box(&mixed_tree),
                black_box(&mixed),
                REPORT_TOP_ROWS,
                Metric::OnDisk,
            ));
        })
    });
    let _ = fs::remove_dir_all(&mixed);

    let reclaim = fixture_root_for("reclaim");
    build_reclaim_fixture_at(&reclaim);
    let reclaim_tree = scan_tree_for(&reclaim);
    c.bench_function(RECLAIM_HEAVY_BENCH, |b| {
        b.iter(|| {
            let hotspots = find_hotspots(
                black_box(&reclaim_tree),
                black_box(&reclaim),
                RECLAIM_MIN_SIZE,
            );
            let aggs = summarize(&hotspots);
            black_box(reclaim_lines_for(
                &hotspots,
                &aggs,
                REPORT_TOP_ROWS,
                Metric::OnDisk,
                RECLAIM_MIN_SIZE,
            ));
        })
    });
    let _ = fs::remove_dir_all(&reclaim);
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
