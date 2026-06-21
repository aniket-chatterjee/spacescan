//! JSON and CSV exporters for the scanned tree and the reclaim list.

use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::constants;
use crate::metric::Metric;
use crate::node::Node;
use crate::reclaim::{indices_ranked_by_size, Hotspot};

#[derive(Serialize)]
struct ExportNode {
    path: String,
    name: String,
    is_dir: bool,
    apparent_bytes: u64,
    disk_bytes: u64,
    files: u64,
    dirs: u64,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    children: Vec<ExportNode>,
}

fn export_node_from(node: &Node, path: &mut PathBuf, dirs_only: bool) -> ExportNode {
    let children = export_children_from(node, path, dirs_only);
    ExportNode {
        path: path.to_string_lossy().into_owned(),
        name: node.name.to_string(),
        is_dir: node.is_dir(),
        apparent_bytes: node.apparent_size,
        disk_bytes: node.disk_size,
        files: node.file_count,
        dirs: node.dir_count(),
        children,
    }
}

fn export_children_from(node: &Node, path: &mut PathBuf, dirs_only: bool) -> Vec<ExportNode> {
    let mut children = Vec::new();
    for child in sorted_children_for(node) {
        if dirs_only && !child.is_dir() {
            continue;
        }
        path.push(child.name.as_ref());
        children.push(export_node_from(child, path, dirs_only));
        path.pop();
    }
    children
}

/// Write the (optionally directory-only) tree to `out` as pretty JSON.
pub fn write_json(node: &Node, base: &Path, out: &Path, dirs_only: bool) -> io::Result<()> {
    let mut path = base.to_path_buf();
    let root = export_node_from(node, &mut path, dirs_only);
    let w = BufWriter::new(File::create(out)?);
    serde_json::to_writer_pretty(w, &root).map_err(io::Error::other)?;
    Ok(())
}

/// Write one CSV row per directory in the subtree.
pub fn write_csv(node: &Node, base: &Path, out: &Path) -> io::Result<()> {
    let mut w = BufWriter::new(File::create(out)?);
    write_csv_to(node, base, &mut w)?;
    Ok(())
}

pub fn write_csv_to<W: Write>(node: &Node, base: &Path, writer: &mut W) -> io::Result<()> {
    writeln!(writer, "{}", constants::export::TREE_CSV_HEADER)?;
    let mut path = base.to_path_buf();
    walk_csv(node, &mut path, writer)
}

fn walk_csv<W: Write>(node: &Node, path: &mut PathBuf, w: &mut W) -> io::Result<()> {
    if !node.is_dir() {
        return Ok(());
    }
    writeln!(
        w,
        "{},{},{},{},{}",
        csv_field(&path.to_string_lossy()),
        node.apparent_size,
        node.disk_size,
        node.file_count,
        node.dir_count()
    )?;
    for c in sorted_children_for(node) {
        if c.is_dir() {
            path.push(c.name.as_ref());
            walk_csv(c, path, w)?;
            path.pop();
        }
    }
    Ok(())
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn sorted_children_for(node: &Node) -> Vec<&Node> {
    let mut children: Vec<&Node> = node.children.iter().collect();
    children.sort_unstable_by(|left, right| {
        right
            .disk_size
            .cmp(&left.disk_size)
            .then_with(|| left.name.cmp(&right.name))
    });
    children
}

/// Write one CSV row per removable cluster, largest first.
pub fn write_reclaim_csv(hotspots: &[Hotspot], metric: Metric, out: &Path) -> io::Result<()> {
    let idx = indices_ranked_by_size(hotspots, metric, 0);
    let mut w = BufWriter::new(File::create(out)?);
    write_reclaim_csv_to(hotspots, &idx, &mut w)
}

pub fn write_reclaim_csv_to<W: Write>(
    hotspots: &[Hotspot],
    ranked_indices: &[usize],
    writer: &mut W,
) -> io::Result<()> {
    writeln!(writer, "{}", constants::export::RECLAIM_CSV_HEADER)?;
    for &i in ranked_indices {
        let h = &hotspots[i];
        let m = h.cat.meta();
        writeln!(
            writer,
            "{},{},{},{},{},{},{}",
            m.key,
            m.safety.label(),
            h.apparent,
            h.disk,
            h.files,
            h.dirs,
            csv_field(&h.path.to_string_lossy())
        )?;
    }
    Ok(())
}
