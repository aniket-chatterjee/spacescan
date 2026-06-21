//! Small, dependency-free helpers shared across the crate.

/// Move `current` by `delta`, clamped to a valid index in `0..len`.
///
/// Returns `0` when `len` is `0` (an empty list has no valid index, so callers
/// that care about emptiness should guard that case themselves).
pub fn clamp_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    let max = (len - 1) as isize;
    (current as isize + delta).clamp(0, max) as usize
}

/// Map a mouse click's row to a list index.
///
/// `top` is the y of the list's container, `chrome` the number of non-data rows
/// above the first item (e.g. border + header = 2), `height` the container
/// height, `offset` the scroll offset, and `len` the item count. Returns `None`
/// when the click is outside the data rows.
pub fn row_at(
    top: u16,
    chrome: u16,
    height: u16,
    offset: usize,
    click_y: u16,
    len: usize,
) -> Option<usize> {
    let first = top.saturating_add(chrome);
    if click_y < first || click_y >= top.saturating_add(height) {
        return None;
    }
    let idx = offset + (click_y - first) as usize;
    (idx < len).then_some(idx)
}

/// True if `name` contains `filter`, case-insensitively. An empty filter matches
/// everything.
pub fn matches_filter(name: &str, filter: &str) -> bool {
    FilterMatcher::for_filter(filter).matches(name)
}

/// Reusable case-insensitive substring matcher for a fixed filter.
pub struct FilterMatcher {
    filter_lower: Option<String>,
}

impl FilterMatcher {
    pub fn for_filter(filter: &str) -> Self {
        if filter.is_empty() {
            return Self { filter_lower: None };
        }

        Self {
            filter_lower: Some(filter.to_lowercase()),
        }
    }

    pub fn matches(&self, name: &str) -> bool {
        let Some(filter) = &self.filter_lower else {
            return true;
        };

        name.to_lowercase().contains(filter)
    }
}
