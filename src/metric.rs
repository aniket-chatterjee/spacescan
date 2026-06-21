//! The size metric the user is viewing by.
//!
//! Sizes are reported two ways: the logical (apparent) byte count, and the
//! on-disk size (apparent rounded up to the filesystem cluster size). Passing a
//! `Metric` instead of a bare `bool` keeps call sites self-explanatory:
//! `node.size(Metric::OnDisk)` reads better than `node.size(true)`.

/// Which size measure to report.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Metric {
    /// Logical file size (the number of bytes of content).
    Apparent,
    /// Size occupied on disk (apparent rounded up to the cluster size).
    OnDisk,
}

impl Metric {
    /// Pick the matching value from an `(apparent, on_disk)` pair.
    #[inline]
    pub fn pick(self, apparent: u64, on_disk: u64) -> u64 {
        match self {
            Metric::Apparent => apparent,
            Metric::OnDisk => on_disk,
        }
    }

    /// Short human label used in report and TUI headers.
    pub fn label(self) -> &'static str {
        match self {
            Metric::Apparent => "apparent",
            Metric::OnDisk => "on-disk",
        }
    }

    /// The other metric (used by the TUI's toggle key).
    pub fn toggled(self) -> Self {
        match self {
            Metric::Apparent => Metric::OnDisk,
            Metric::OnDisk => Metric::Apparent,
        }
    }

    /// Build a metric from the CLI `--disk` flag — the one boundary where a
    /// boolean is still the natural input.
    pub fn from_on_disk(on_disk: bool) -> Self {
        if on_disk {
            Metric::OnDisk
        } else {
            Metric::Apparent
        }
    }
}
