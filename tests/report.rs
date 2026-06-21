use std::path::Path;

use spacescan::metric::Metric;
use spacescan::node::Node;
use spacescan::reclaim::{CatAgg, Category, Hotspot};
use spacescan::report::{reclaim_lines_for, report_lines_for};

fn sample_tree() -> Node {
    Node::dir_with_children(
        "root".to_string(),
        60,
        64,
        2,
        1,
        vec![
            Node::dir_with_children(
                "src".to_string(),
                40,
                40,
                1,
                0,
                vec![Node::file("main.rs".to_string(), 40, 40)],
            ),
            Node::file("README.md".to_string(), 20, 24),
        ],
    )
}

#[test]
fn report_lines_preserve_summary_sections() {
    let lines = report_lines_for(&sample_tree(), Path::new("C:/repo"), 3, Metric::Apparent);

    assert!(lines.iter().any(|line| line == "Scan root : C:/repo"));
    assert!(lines.iter().any(|line| line == "Apparent  : 60 B"));
    assert!(lines
        .iter()
        .any(|line| line == "Top 3 subdirectories (by apparent size):"));
    assert!(lines.iter().any(|line| line.contains("src/")));
}

#[test]
fn reclaim_lines_include_category_and_threshold_sections() {
    let hotspot = Hotspot {
        path: "C:/repo/target".into(),
        cat: Category::Build,
        apparent: 120,
        disk: 128,
        files: 4,
        dirs: 1,
    };
    let aggs = vec![CatAgg {
        cat: Category::Build,
        apparent: 120,
        disk: 128,
        count: 1,
    }];

    let lines = reclaim_lines_for(&[hotspot], &aggs, 5, Metric::OnDisk, 100);

    assert!(lines
        .iter()
        .any(|line| line.contains("Reclaimable clusters")));
    assert!(lines.iter().any(|line| line.contains("BUILD")));
    assert!(lines.iter().any(|line| line.contains("C:/repo/target")));
}
