//! Non-interactive text report printed to stdout.

use std::path::Path;

use crate::constants;
use crate::format::{ascii_bar, human_size};
use crate::metric::Metric;
use crate::node::Node;
use crate::reclaim::{indices_ranked_by_size, safety_totals, total_reclaimable, CatAgg, Hotspot};
use crate::stats::{ext_breakdown, top_files_in};

/// Print a non-interactive summary report to stdout.
pub fn print_report(root: &Node, base: &Path, top: usize, metric: Metric) {
    for line in report_lines_for(root, base, top, metric) {
        println!("{line}");
    }
}

/// Build the non-interactive summary report as lines.
pub fn report_lines_for(root: &Node, base: &Path, top: usize, metric: Metric) -> Vec<String> {
    let metric_label = metric.label();
    let mut lines = vec![
        String::new(),
        format!("Scan root : {}", base.display()),
        format!("Apparent  : {}", human_size(root.apparent_size)),
        format!("On disk   : {}", human_size(root.disk_size)),
        format!("Files     : {}", root.file_count),
        format!("Dirs      : {}", root.dir_count()),
        String::new(),
        format!("Top {top} subdirectories (by {metric_label} size):"),
    ];

    lines.extend(top_directory_lines_for(root, top, metric));
    lines.push(String::new());
    lines.push(format!("Top {top} files (by {metric_label} size):"));
    lines.extend(top_file_lines_for(root, base, top, metric));
    lines.push(String::new());
    lines.push(format!("Top {top} file types (by {metric_label} size):"));
    lines.extend(file_type_lines_for(root, top, metric));
    lines.push(String::new());
    lines
}

fn top_directory_lines_for(root: &Node, top: usize, metric: Metric) -> Vec<String> {
    let dirs = sorted_directories_in(root, metric);
    if dirs.is_empty() {
        return vec![constants::report::NONE_ROW.to_string()];
    }
    dirs.iter()
        .take(top)
        .map(|dir| {
            format!(
                "  {:>11}  {:>10} files  {}/",
                human_size(dir.size(metric)),
                dir.file_count,
                dir.name
            )
        })
        .collect()
}

fn sorted_directories_in(root: &Node, metric: Metric) -> Vec<&Node> {
    let mut dirs: Vec<&Node> = root
        .children
        .iter()
        .filter(|child| child.is_dir())
        .collect();
    dirs.sort_by_key(|dir| std::cmp::Reverse(dir.size(metric)));
    dirs
}

fn top_file_lines_for(root: &Node, base: &Path, top: usize, metric: Metric) -> Vec<String> {
    let files = top_files_in(root, base, top, metric);
    if files.is_empty() {
        return vec![constants::report::NONE_ROW.to_string()];
    }
    files
        .iter()
        .map(|(path, size)| format!("  {:>11}  {}", human_size(*size), path.display()))
        .collect()
}

fn file_type_lines_for(root: &Node, top: usize, metric: Metric) -> Vec<String> {
    let ext = ext_breakdown(root, metric);
    if ext.is_empty() {
        return vec![constants::report::NONE_ROW.to_string()];
    }
    ext.iter()
        .take(top)
        .map(|entry| {
            format!(
                "  {:>11}  {:>10} files  .{}",
                human_size(entry.size(metric)),
                entry.count,
                entry.ext
            )
        })
        .collect()
}

/// Print the reclaimable-space section: which directory clusters are biggest
/// and easiest to remove.
pub fn print_reclaim(
    hotspots: &[Hotspot],
    aggs: &[CatAgg],
    top: usize,
    metric: Metric,
    min_size: u64,
) {
    for line in reclaim_lines_for(hotspots, aggs, top, metric, min_size) {
        println!("{line}");
    }
}

/// Build the reclaimable-space report section as lines.
pub fn reclaim_lines_for(
    hotspots: &[Hotspot],
    aggs: &[CatAgg],
    top: usize,
    metric: Metric,
    min_size: u64,
) -> Vec<String> {
    let metric_label = metric.label();
    let total = total_reclaimable(hotspots, metric);
    let (regen, junk, review) = safety_totals(hotspots, metric);
    let mut lines = vec![
        format!("Reclaimable clusters (easy-to-remove directories, by {metric_label} size):"),
        format!(
            "  Total potential: {}   [regenerable {}, junk {}, review {}]   across {} spots",
            human_size(total),
            human_size(regen),
            human_size(junk),
            human_size(review),
            hotspots.len()
        ),
        String::new(),
    ];

    if aggs.is_empty() {
        lines.push(constants::report::NO_RECLAIM_CLUSTERS.to_string());
        lines.push(String::new());
        return lines;
    }

    lines.extend(category_lines_for(aggs, metric));
    lines.push(String::new());
    lines.extend(reclaim_spot_lines_for(hotspots, top, metric, min_size));
    lines.push(String::new());
    lines
}

fn category_lines_for(aggs: &[CatAgg], metric: Metric) -> Vec<String> {
    let max_cat = aggs
        .iter()
        .map(|a| a.size(metric))
        .max()
        .unwrap_or(1)
        .max(1);
    let mut lines = vec!["  By category:".to_string()];
    lines.extend(
        aggs.iter()
            .map(|agg| category_line_for(agg, max_cat, metric)),
    );
    lines
}

fn category_line_for(agg: &CatAgg, max_cat: u64, metric: Metric) -> String {
    let size = agg.size(metric);
    let meta = agg.cat.meta();
    format!(
        "    {}  {:>11}  {:>4} spots  {:<6} {} ({})",
        ascii_bar(
            size as f64 / max_cat as f64,
            constants::report::ASCII_BAR_WIDTH
        ),
        human_size(size),
        agg.count,
        meta.badge,
        meta.label,
        meta.safety.label()
    )
}

fn reclaim_spot_lines_for(
    hotspots: &[Hotspot],
    top: usize,
    metric: Metric,
    min_size: u64,
) -> Vec<String> {
    let order = indices_ranked_by_size(hotspots, metric, min_size);
    let mut lines = vec![format!(
        "  Largest removable spots (>= {}):",
        human_size(min_size)
    )];
    if order.is_empty() {
        lines.push(constants::report::NONE_ABOVE_THRESHOLD_ROW.to_string());
        return lines;
    }
    lines.extend(order.iter().take(top).map(|&index| {
        let hotspot = &hotspots[index];
        let meta = hotspot.cat.meta();
        format!(
            "    {:>11}  {:<6} {}",
            human_size(hotspot.size(metric)),
            meta.badge,
            hotspot.path.display()
        )
    }));
    lines
}
