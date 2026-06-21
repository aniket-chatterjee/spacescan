//! Unit tests for the `util` helpers.

use spacescan::util::{clamp_index, matches_filter, row_at, FilterMatcher};

#[test]
fn clamp_index_steps_within_bounds() {
    assert_eq!(clamp_index(0, 5, 1), 1);
    assert_eq!(clamp_index(3, 5, -2), 1);
}

#[test]
fn clamp_index_saturates_at_the_edges() {
    assert_eq!(clamp_index(0, 5, -1), 0);
    assert_eq!(clamp_index(4, 5, 1), 4);
    assert_eq!(clamp_index(4, 5, 10), 4);
    assert_eq!(clamp_index(2, 5, -10), 0);
}

#[test]
fn clamp_index_handles_empty_lists() {
    assert_eq!(clamp_index(0, 0, 1), 0);
    assert_eq!(clamp_index(0, 0, -1), 0);
}

#[test]
fn row_at_maps_clicks_to_indices() {
    // Container at y=5, height=10, chrome=2 (border+header), no scroll, 6 items.
    // First data row is screen y=7.
    assert_eq!(row_at(5, 2, 10, 0, 7, 6), Some(0));
    assert_eq!(row_at(5, 2, 10, 0, 9, 6), Some(2));
    // Clicks on the border/header rows are not data.
    assert_eq!(row_at(5, 2, 10, 0, 5, 6), None);
    assert_eq!(row_at(5, 2, 10, 0, 6, 6), None);
    // Below the last item (only 6 items -> rows y=7..=12).
    assert_eq!(row_at(5, 2, 10, 0, 13, 6), None);
    // Outside the container height.
    assert_eq!(row_at(5, 2, 10, 0, 99, 6), None);
}

#[test]
fn row_at_accounts_for_scroll_offset() {
    // Scrolled down by 10: the first visible data row maps to index 10.
    assert_eq!(row_at(0, 2, 20, 10, 2, 100), Some(10));
    assert_eq!(row_at(0, 2, 20, 10, 5, 100), Some(13));
}

#[test]
fn matches_filter_is_case_insensitive_substring() {
    assert!(matches_filter("node_modules", "mod"));
    assert!(matches_filter("Cargo.toml", "CARGO"));
    assert!(matches_filter("anything", "")); // empty matches all
    assert!(!matches_filter("src", "target"));
}

#[test]
fn filter_matcher_reuses_normalized_filter() {
    let matcher = FilterMatcher::for_filter("CARGO");

    assert!(matcher.matches("Cargo.toml"));
    assert!(!matcher.matches("package.json"));
}

#[test]
fn empty_filter_matcher_matches_everything() {
    let matcher = FilterMatcher::for_filter("");

    assert!(matcher.matches("src"));
    assert!(matcher.matches(""));
}
