//! Unit tests for the `metric` enum.

use spacescan::metric::Metric;

#[test]
fn pick_selects_the_matching_value() {
    assert_eq!(Metric::Apparent.pick(10, 20), 10);
    assert_eq!(Metric::OnDisk.pick(10, 20), 20);
}

#[test]
fn label_is_stable() {
    assert_eq!(Metric::Apparent.label(), "apparent");
    assert_eq!(Metric::OnDisk.label(), "on-disk");
}

#[test]
fn toggled_flips_the_metric() {
    assert_eq!(Metric::Apparent.toggled(), Metric::OnDisk);
    assert_eq!(Metric::OnDisk.toggled(), Metric::Apparent);
}

#[test]
fn from_on_disk_maps_the_cli_flag() {
    assert_eq!(Metric::from_on_disk(true), Metric::OnDisk);
    assert_eq!(Metric::from_on_disk(false), Metric::Apparent);
}
