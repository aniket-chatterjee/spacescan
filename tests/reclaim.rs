//! Unit tests for the `reclaim` module (classification, heuristics, totals).

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use spacescan::metric::Metric;
use spacescan::node::Node;
use spacescan::reclaim::{
    category_for, is_container_name, is_system_path, looks_like_app, safety_totals, summarize,
    Category, Hotspot,
};

fn siblings(names: &[&str]) -> HashSet<String> {
    names.iter().map(|s| s.to_string()).collect()
}

#[test]
fn category_for_matches_known_names_case_insensitively() {
    let empty = HashSet::new();
    assert_eq!(category_for("node_modules", &empty), Some(Category::Build));
    assert_eq!(category_for("NODE_MODULES", &empty), Some(Category::Build));
    assert_eq!(category_for("__pycache__", &empty), Some(Category::Pyenv));
    assert_eq!(category_for(".nuget", &empty), Some(Category::Pkg));
    assert_eq!(category_for("cache", &empty), Some(Category::Cache));
    assert_eq!(category_for("temp", &empty), Some(Category::Temp));
    assert_eq!(category_for("logs", &empty), Some(Category::Logs));
    assert_eq!(category_for("downloads", &empty), Some(Category::Downloads));
    assert_eq!(category_for("some_random_dir", &empty), None);
}

#[test]
fn category_for_needs_a_manifest_for_generic_build_dirs() {
    let empty = HashSet::new();
    assert_eq!(category_for("target", &empty), None);
    assert_eq!(
        category_for("target", &siblings(&["cargo.toml"])),
        Some(Category::Build)
    );
    assert_eq!(category_for("build", &empty), None);
    assert_eq!(
        category_for("build", &siblings(&["package.json"])),
        Some(Category::Build)
    );
}

#[test]
fn is_system_path_flags_protected_locations() {
    assert!(is_system_path(Path::new("X/Windows/Y")));
    assert!(is_system_path(Path::new("X/ProgramData/Y")));
    assert!(!is_system_path(Path::new("home/user/project")));
}

#[test]
fn is_container_name_flags_user_profile_folders() {
    assert!(is_container_name("Users"));
    assert!(is_container_name("appdata"));
    assert!(is_container_name("Downloads"));
    assert!(!is_container_name("my_project"));
}

#[test]
fn looks_like_app_detects_direct_or_bin_executables() {
    let direct = Node::dir_with_children(
        "app".to_string(),
        10,
        10,
        1,
        0,
        vec![Node::file("app.exe".to_string(), 10, 10)],
    );
    assert!(looks_like_app(&direct));

    // Case-insensitive extension match.
    let upper = Node::dir_with_children(
        "app2".to_string(),
        1,
        1,
        1,
        0,
        vec![Node::file("RUN.EXE".to_string(), 1, 1)],
    );
    assert!(looks_like_app(&upper));

    // Executable inside a bin/ subfolder (SDK layout).
    let bin = Node::dir_with_children(
        "bin".to_string(),
        1,
        1,
        1,
        0,
        vec![Node::file("tool.exe".to_string(), 1, 1)],
    );
    let sdk = Node::dir_with_children("sdk".to_string(), 1, 1, 1, 1, vec![bin]);
    assert!(looks_like_app(&sdk));
}

#[test]
fn looks_like_app_ignores_folders_without_executables() {
    let docs = Node::dir_with_children(
        "docs".to_string(),
        1,
        1,
        1,
        0,
        vec![Node::file("readme.txt".to_string(), 1, 1)],
    );
    assert!(!looks_like_app(&docs));
}

fn hotspot(cat: Category, apparent: u64, disk: u64) -> Hotspot {
    Hotspot {
        path: PathBuf::from("p"),
        cat,
        apparent,
        disk,
        files: 1,
        dirs: 0,
    }
}

#[test]
fn safety_totals_groups_by_safety_level() {
    let hs = vec![
        hotspot(Category::Build, 100, 100), // regenerable
        hotspot(Category::Temp, 30, 30),    // junk
        hotspot(Category::Downloads, 7, 7), // review
    ];
    let (regen, junk, review) = safety_totals(&hs, Metric::Apparent);
    assert_eq!((regen, junk, review), (100, 30, 7));
}

#[test]
fn summarize_aggregates_and_orders_by_disk() {
    let hs = vec![
        hotspot(Category::Build, 100, 100),
        hotspot(Category::Build, 50, 50),
        hotspot(Category::Temp, 400, 400),
    ];
    let aggs = summarize(&hs);
    assert_eq!(aggs.len(), 2);
    assert_eq!(aggs[0].cat, Category::Temp);
    assert_eq!(aggs[0].count, 1);
    assert_eq!(aggs[1].cat, Category::Build);
    assert_eq!(aggs[1].count, 2);
    assert_eq!(aggs[1].disk, 150);
}

#[test]
fn category_index_round_trips_through_all() {
    for (i, &c) in Category::ALL.iter().enumerate() {
        assert_eq!(c.index(), i);
        assert_eq!(Category::ALL[c.index()], c);
    }
}

#[test]
fn category_meta_keys_are_stable() {
    assert_eq!(Category::Build.meta().key, "build");
    assert_eq!(Category::Pkg.meta().key, "pkgcache");
    assert_eq!(Category::Cache.meta().key, "appcache");
    assert_eq!(Category::Bundle.meta().key, "bundle");
}
