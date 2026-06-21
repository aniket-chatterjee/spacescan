use std::fs;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

type TestResult = Result<(), Box<dyn std::error::Error>>;

fn spacescan() -> Command {
    Command::new(env!("CARGO_BIN_EXE_spacescan"))
}

fn temp_fixture() -> Result<std::path::PathBuf, Box<dyn std::error::Error>> {
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let root = std::env::temp_dir().join(format!("spacescan-cli-{stamp}"));
    fs::create_dir_all(root.join("target"))?;
    fs::write(root.join("target").join("cache.bin"), b"cache")?;
    fs::write(root.join("Cargo.toml"), b"[package]\nname='fixture'\n")?;
    Ok(root)
}

#[test]
fn help_lists_benchmark_json_flags() -> TestResult {
    let output = spacescan().arg("--help").output()?;
    let stdout = String::from_utf8(output.stdout)?;

    assert!(output.status.success());
    assert!(stdout.contains("--exclude"));
    assert!(stdout.contains("--prune-zero-size"));
    assert!(stdout.contains("--bench-json"));
    assert!(stdout.contains("--bench-warmup"));
    Ok(())
}

#[test]
fn no_tui_report_runs_on_fixture() -> TestResult {
    let root = temp_fixture()?;
    let output = spacescan()
        .arg(&root)
        .arg("--no-tui")
        .arg("--top")
        .arg("5")
        .output()?;
    let stdout = String::from_utf8(output.stdout)?;
    let _ = fs::remove_dir_all(root);

    assert!(output.status.success());
    assert!(stdout.contains("Scan root"));
    assert!(stdout.contains("Top 5 subdirectories"));
    Ok(())
}

#[test]
fn bench_json_writes_machine_readable_summary() -> TestResult {
    let root = temp_fixture()?;
    let json = root.join("bench.json");
    let output = spacescan()
        .arg(&root)
        .arg("--bench")
        .arg("2")
        .arg("--bench-warmup")
        .arg("0")
        .arg("--bench-json")
        .arg(&json)
        .output()?;
    let json_text = fs::read_to_string(&json)?;
    let _ = fs::remove_dir_all(root);

    assert!(output.status.success());
    assert!(json_text.contains("\"runs\": 2"));
    assert!(json_text.contains("\"engine\": \"walk\""));
    assert!(json_text.contains("\"worker_threads\""));
    assert!(json_text.contains("\"logical_cpus\""));
    assert!(json_text.contains("\"build_profile\""));
    assert!(json_text.contains("\"cache_state\""));
    assert!(json_text.contains("\"excluded_paths\""));
    assert!(json_text.contains("\"prune_zero_size_dirs\""));
    assert!(json_text.contains("\"memory_source\""));
    assert!(json_text.contains("\"memory_sample_ms\""));
    assert!(json_text.contains("\"peak_rss_bytes\""));
    assert!(json_text.contains("\"files\""));
    assert!(json_text.contains("\"tree_nodes\""));
    assert!(json_text.contains("\"dirs_per_second\""));
    assert!(json_text.contains("\"tree_nodes_per_second\""));
    assert!(json_text.contains("\"node_size_bytes\""));
    assert!(json_text.contains("\"estimated_tree_node_bytes\""));
    assert!(json_text.contains("\"name_bytes\""));
    assert!(json_text.contains("\"estimated_tree_storage_bytes\""));
    assert!(json_text.contains("\"empty_dirs\""));
    assert!(json_text.contains("\"zero_size_dirs\""));
    assert!(json_text.contains("\"max_depth\""));
    assert!(json_text.contains("\"max_children\""));
    assert!(json_text.contains("\"max_child_dirs\""));
    assert!(json_text.contains("\"median_seconds\""));
    Ok(())
}
