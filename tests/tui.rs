use spacescan::tui::browser_title_for;

#[test]
fn browser_title_shows_filter_state() {
    assert_eq!(browser_title_for(3, "", false), " Contents (3) ");
    assert_eq!(
        browser_title_for(2, "cache", false),
        " Contents - filter: cache (2) "
    );
    assert_eq!(
        browser_title_for(1, "cache", true),
        " Contents - filter: cache_ (1) "
    );
}
