//! Unit tests for the `stats` module (extension breakdown, largest files).

use std::path::Path;

use spacescan::metric::Metric;
use spacescan::node::Node;
use spacescan::stats::{ext_breakdown, ext_of, top_files_in};

#[test]
fn ext_of_extracts_lowercased_extensions() {
    assert_eq!(ext_of("file.txt"), "txt");
    assert_eq!(ext_of("FILE.TXT"), "txt");
    assert_eq!(ext_of("archive.tar.gz"), "gz");
    assert_eq!(ext_of("a.b"), "b");
}

#[test]
fn ext_of_treats_dotfiles_and_bare_names_as_none() {
    assert_eq!(ext_of(".gitignore"), "(none)");
    assert_eq!(ext_of("noext"), "(none)");
    assert_eq!(ext_of("trailing."), "(none)");
}

fn sample_tree() -> Node {
    Node::dir_with_children(
        "root".to_string(),
        350,
        448,
        3,
        0,
        vec![
            Node::file("a.txt".to_string(), 100, 128),
            Node::file("b.txt".to_string(), 200, 256),
            Node::file("c.log".to_string(), 50, 64),
        ],
    )
}

#[test]
fn ext_breakdown_groups_and_sorts_by_metric() {
    let root = sample_tree();
    let breakdown = ext_breakdown(&root, Metric::Apparent);

    // ".txt" (300 apparent across 2 files) outranks ".log" (50).
    assert_eq!(breakdown[0].ext, "txt");
    assert_eq!(breakdown[0].count, 2);
    assert_eq!(breakdown[0].size(Metric::Apparent), 300);
    assert_eq!(breakdown[1].ext, "log");
}

#[test]
fn top_files_in_returns_largest_first() {
    let root = sample_tree();
    let top = top_files_in(&root, Path::new("base"), 2, Metric::Apparent);

    assert_eq!(top.len(), 2);
    assert_eq!(top[0].0, Path::new("base").join("b.txt"));
    assert_eq!(top[0].1, 200);
    assert_eq!(top[1].1, 100);
}

#[test]
fn top_files_in_returns_nothing_for_zero() {
    let root = sample_tree();
    assert!(top_files_in(&root, Path::new("base"), 0, Metric::Apparent).is_empty());
}
