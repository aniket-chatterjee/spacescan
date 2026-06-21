//! Centralized visual theme for the interactive TUI.
//!
//! One small palette and a set of reusable styles keep every pane, list, and
//! overlay coherent. The guiding rule is restraint: bright colors are reserved
//! for data meaning (sizes, usage heat, reclaim safety, category hues) while all
//! structural chrome — borders, labels, hints, separators — stays muted so the
//! data is what stands out.

use std::borrow::Cow;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::Block;

use crate::metric::Metric;
use crate::reclaim::{Hue, Safety};

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

/// Interactive / primary elements: paths, directory names, key hints.
pub const ACCENT: Color = Color::Cyan;
/// Structural chrome and secondary text: borders, labels, separators.
pub const MUTED: Color = Color::DarkGray;
/// Apparent (logical) size.
pub const APPARENT: Color = Color::Green;
/// On-disk size.
pub const ON_DISK: Color = Color::Yellow;
/// Destructive actions, warnings, and the hottest usage band.
pub const DANGER: Color = Color::Red;
/// Subtle background fill for the selected row.
pub const SELECTION_BG: Color = Color::Indexed(237);

/// The single caret that marks a selection across every list and table.
pub const SELECT_SYMBOL: &str = "› ";
/// Thin separator placed between inline metric and hint groups.
pub const SEP: &str = "  ·  ";

// ---------------------------------------------------------------------------
// Reusable styles
// ---------------------------------------------------------------------------

/// Border style shared by every panel.
pub fn border() -> Style {
    Style::new().fg(MUTED)
}

/// Plain bold panel title.
pub fn title() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

/// Accented bold title used by modal overlays.
pub fn title_accent() -> Style {
    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// Muted label / secondary text.
pub fn label() -> Style {
    Style::new().fg(MUTED)
}

/// Emphasized path or primary identifier.
pub fn path() -> Style {
    Style::new().fg(ACCENT).add_modifier(Modifier::BOLD)
}

/// Selected-row highlight. Background only, so each cell keeps its own color.
pub fn selection() -> Style {
    Style::new().bg(SELECTION_BG).add_modifier(Modifier::BOLD)
}

/// Table header-row style.
pub fn header() -> Style {
    Style::new().fg(MUTED).add_modifier(Modifier::BOLD)
}

// ---------------------------------------------------------------------------
// Panels
// ---------------------------------------------------------------------------

/// A standard bordered panel with a muted border and a plain bold title.
pub fn panel<'a>(title_text: impl Into<Cow<'a, str>>) -> Block<'a> {
    Block::bordered()
        .border_style(border())
        .title(Span::styled(title_text, title()))
}

/// A modal overlay panel: accent border and accent title to lift it above the
/// dimmed pane behind it.
pub fn overlay<'a>(title_text: impl Into<Cow<'a, str>>) -> Block<'a> {
    Block::bordered()
        .border_style(Style::new().fg(ACCENT))
        .title(Span::styled(title_text, title_accent()))
}

// ---------------------------------------------------------------------------
// Data colors
// ---------------------------------------------------------------------------

/// Color for the active size metric, so size values read consistently.
pub fn metric_color(m: Metric) -> Color {
    match m {
        Metric::Apparent => APPARENT,
        Metric::OnDisk => ON_DISK,
    }
}

/// Heat ramp for a usage fraction: calm green, caution yellow, hot red.
pub fn heat(frac: f64) -> Color {
    if frac >= crate::constants::tui::HEAT_HIGH {
        DANGER
    } else if frac >= crate::constants::tui::HEAT_MID {
        Color::Yellow
    } else {
        Color::Green
    }
}

/// Reclaim category hue mapped to a terminal color.
pub fn hue(h: Hue) -> Color {
    match h {
        Hue::Green => Color::Green,
        Hue::Cyan => Color::Cyan,
        Hue::Yellow => Color::Yellow,
        Hue::Magenta => Color::Magenta,
        Hue::Blue => Color::Blue,
    }
}

/// Reclaim safety level mapped to a terminal color.
pub fn safety(s: Safety) -> Color {
    match s {
        Safety::Regenerable => Color::Green,
        Safety::Junk => Color::Magenta,
        Safety::Review => Color::Yellow,
    }
}

// ---------------------------------------------------------------------------
// Composite widgets
// ---------------------------------------------------------------------------

/// A two-tone usage meter: a filled run in `fill` followed by a dimmed track,
/// so even short bars stay legible. Returns the (filled, track) spans.
pub fn meter(frac: f64, width: usize, fill: Color) -> Vec<Span<'static>> {
    let filled = (frac.clamp(0.0, 1.0) * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    vec![
        Span::styled("█".repeat(filled), Style::new().fg(fill)),
        Span::styled("░".repeat(empty), Style::new().fg(MUTED)),
    ]
}

/// Build a calm hint line from `(key, label)` pairs: keys accented, labels
/// muted, groups separated by a thin dot.
pub fn hints(pairs: &[(&str, &str)]) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    for (i, (key, desc)) in pairs.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(SEP, label()));
        }
        spans.push(Span::styled((*key).to_string(), Style::new().fg(ACCENT)));
        spans.push(Span::raw(" "));
        spans.push(Span::styled((*desc).to_string(), label()));
    }
    spans
}
