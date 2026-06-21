//! Unit tests for the `format` module (sizes, bars, slugs).

use spacescan::format::{ascii_bar, human_size, parse_size, sanitize, unicode_bar};

#[test]
fn human_size_uses_bytes_below_one_kib() {
    assert_eq!(human_size(0), "0 B");
    assert_eq!(human_size(512), "512 B");
    assert_eq!(human_size(1023), "1023 B");
}

#[test]
fn human_size_scales_to_binary_units() {
    assert_eq!(human_size(1024), "1.0 KB");
    assert_eq!(human_size(1536), "1.5 KB");
    assert_eq!(human_size(1024 * 1024), "1.0 MB");
    assert_eq!(human_size(1024 * 1024 * 1024), "1.0 GB");
}

#[test]
fn parse_size_handles_units_and_decimals() {
    assert_eq!(parse_size("2048"), Ok(2048));
    assert_eq!(parse_size("1k"), Ok(1024));
    assert_eq!(parse_size("1kb"), Ok(1024));
    assert_eq!(parse_size("1kib"), Ok(1024));
    assert_eq!(parse_size("512k"), Ok(512 * 1024));
    assert_eq!(parse_size("100mb"), Ok(100 * 1024 * 1024));
    assert_eq!(parse_size("1.5g"), Ok(1610612736));
}

#[test]
fn parse_size_is_case_and_whitespace_insensitive() {
    assert_eq!(parse_size("  1G "), Ok(1024 * 1024 * 1024));
}

#[test]
fn parse_size_rejects_bad_input() {
    assert!(parse_size("").is_err());
    assert!(parse_size("abc").is_err());
    assert!(parse_size("-5").is_err());
    assert!(parse_size("5x").is_err());
}

#[test]
fn bars_have_requested_width_and_clamp() {
    assert_eq!(unicode_bar(0.0, 4).chars().count(), 4);
    assert_eq!(unicode_bar(1.0, 4), "████");
    assert_eq!(unicode_bar(0.0, 4), "░░░░");
    assert_eq!(unicode_bar(0.5, 4), "██░░");
    // Out-of-range fractions are clamped.
    assert_eq!(unicode_bar(2.0, 4), "████");
    assert_eq!(unicode_bar(-1.0, 4), "░░░░");

    assert_eq!(ascii_bar(1.0, 4), "####");
    assert_eq!(ascii_bar(0.0, 4), "----");
    assert_eq!(ascii_bar(0.5, 4), "##--");
}

#[test]
fn sanitize_slugifies_and_falls_back() {
    assert_eq!(sanitize("hello"), "hello");
    assert_eq!(sanitize("a b/c"), "a_b_c");
    assert_eq!(sanitize("__ab__"), "ab");
    assert_eq!(sanitize(""), "scan");
    assert_eq!(sanitize("***"), "scan");
}
