//! Formatting helpers: human-readable sizes, size parsing, bars, and slugs.

use crate::constants;

/// Format a byte count as a human-readable string using binary (1024) units.
pub fn human_size(bytes: u64) -> String {
    if bytes < constants::format::BINARY_UNIT as u64 {
        return format!("{bytes} B");
    }
    let mut size = bytes as f64;
    let mut unit = 0usize;
    while size >= constants::format::BINARY_UNIT && unit < constants::format::SIZE_UNITS.len() - 1 {
        size /= constants::format::BINARY_UNIT;
        unit += 1;
    }
    format!("{size:.1} {}", constants::format::SIZE_UNITS[unit])
}

/// Parse a human-readable size string (e.g. "100MB", "1.5g", "512k", "2048")
/// into a byte count. Units are binary (1024). A bare number means bytes.
pub fn parse_size(s: &str) -> Result<u64, String> {
    let t = s.trim().to_ascii_lowercase();
    if t.is_empty() {
        return Err("empty size".to_string());
    }
    let split = t.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(t.len());
    let (num, unit) = t.split_at(split);
    let value: f64 = num
        .trim()
        .parse()
        .map_err(|_| format!("invalid number in size: '{s}'"))?;
    if value < 0.0 {
        return Err(format!("size cannot be negative: '{s}'"));
    }
    let mult: f64 = match unit.trim() {
        "" | "b" => 1.0,
        "k" | "kb" | "kib" => constants::format::BINARY_UNIT,
        "m" | "mb" | "mib" => constants::format::BINARY_UNIT.powi(2),
        "g" | "gb" | "gib" => constants::format::BINARY_UNIT.powi(3),
        "t" | "tb" | "tib" => constants::format::BINARY_UNIT.powi(4),
        other => return Err(format!("unknown size unit: '{other}'")),
    };
    Ok((value * mult) as u64)
}

/// Number of filled cells for `frac` (clamped to `0.0..=1.0`) in a bar that is
/// `width` cells wide.
fn fill_count(frac: f64, width: usize) -> usize {
    (frac.clamp(0.0, 1.0) * width as f64).round() as usize
}

/// A Unicode block bar (`█` filled, `░` empty) `width` cells wide.
pub fn unicode_bar(frac: f64, width: usize) -> String {
    let filled = fill_count(frac, width);
    let mut s = String::with_capacity(width * 3);
    for _ in 0..filled {
        s.push('█');
    }
    for _ in filled..width {
        s.push('░');
    }
    s
}

/// An ASCII bar (`#` filled, `-` empty) `width` cells wide.
pub fn ascii_bar(frac: f64, width: usize) -> String {
    let filled = fill_count(frac, width);
    let mut s = String::with_capacity(width);
    for _ in 0..filled {
        s.push('#');
    }
    for _ in filled..width {
        s.push('-');
    }
    s
}

/// Turn an arbitrary name into a filesystem-safe slug (alphanumerics kept, every
/// other character collapsed to `_`, trimmed). Falls back to `"scan"` when empty.
pub fn sanitize(s: &str) -> String {
    let t: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '_' })
        .collect();
    let t = t.trim_matches('_').to_string();
    if t.is_empty() {
        constants::format::DEFAULT_SLUG.to_string()
    } else {
        t
    }
}
