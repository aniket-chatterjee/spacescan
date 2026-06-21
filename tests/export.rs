use std::path::Path;

use spacescan::export::{write_csv_to, write_reclaim_csv_to};
use spacescan::node::Node;
use spacescan::reclaim::{Category, Hotspot};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn sample_tree() -> Node {
    Node::dir_with_children(
        "root".to_string(),
        12,
        16,
        1,
        1,
        vec![Node::dir_with_children(
            "target".to_string(),
            12,
            16,
            1,
            0,
            vec![Node::file("cache.bin".to_string(), 12, 16)],
        )],
    )
}

fn unsorted_sample_tree() -> Node {
    Node::dir_with_children(
        "root".to_string(),
        30,
        30,
        2,
        2,
        vec![
            Node::dir_with_children(
                "small".to_string(),
                10,
                10,
                1,
                0,
                vec![Node::file("a.bin".to_string(), 10, 10)],
            ),
            Node::dir_with_children(
                "large".to_string(),
                20,
                20,
                1,
                0,
                vec![Node::file("b.bin".to_string(), 20, 20)],
            ),
        ],
    )
}

#[test]
fn tree_csv_writer_preserves_header_and_directory_rows() -> TestResult {
    let mut out = Vec::new();
    write_csv_to(&sample_tree(), Path::new("C:/repo"), &mut out)?;
    let csv = String::from_utf8(out)?;

    assert!(csv.starts_with("path,apparent_bytes,disk_bytes,files,dirs"));
    assert!(csv.contains("C:/repo,12,16,1,1"));
    assert!(csv.contains("C:/repo\\target,12,16,1,0") || csv.contains("C:/repo/target,12,16,1,0"));
    Ok(())
}

#[test]
fn tree_csv_writer_sorts_directories_at_export_boundary() -> TestResult {
    let mut out = Vec::new();
    write_csv_to(&unsorted_sample_tree(), Path::new("C:/repo"), &mut out)?;
    let csv = String::from_utf8(out)?;

    let large_index = csv.find("large").expect("large directory row");
    let small_index = csv.find("small").expect("small directory row");
    assert!(large_index < small_index);
    Ok(())
}

#[test]
fn reclaim_csv_writer_preserves_public_schema() -> TestResult {
    let hotspots = vec![Hotspot {
        path: "C:/repo/target".into(),
        cat: Category::Build,
        apparent: 12,
        disk: 16,
        files: 1,
        dirs: 0,
    }];
    let mut out = Vec::new();
    write_reclaim_csv_to(&hotspots, &[0], &mut out)?;
    let csv = String::from_utf8(out)?;

    assert!(csv.starts_with("category,safety,apparent_bytes,disk_bytes,files,dirs,path"));
    assert!(csv.contains("build,regenerable,12,16,1,0,C:/repo/target"));
    Ok(())
}
