//! Unit tests for the `picker` navigation model (with a fake source).

use std::path::Path;

use spacescan::picker::{Entry, EntryKind, EntrySource, Picker};

struct Fake;

impl EntrySource for Fake {
    fn roots(&self) -> Vec<Entry> {
        vec![
            Entry::new("root", "/", EntryKind::Drive),
            Entry::new("home", "/home/me", EntryKind::Bookmark),
        ]
    }
    fn children(&self, dir: &Path) -> Vec<Entry> {
        vec![
            Entry::new("alpha", dir.join("alpha"), EntryKind::Dir),
            Entry::new("beta", dir.join("beta"), EntryKind::Dir),
        ]
    }
}

#[test]
fn opens_on_roots() {
    let p = Picker::new(&Fake);
    assert_eq!(p.entries().len(), 2);
    assert_eq!(p.location(), None);
    assert_eq!(p.selected(), 0);
}

#[test]
fn move_by_clamps() {
    let mut p = Picker::new(&Fake);
    p.move_by(-1);
    assert_eq!(p.selected(), 0);
    p.move_by(5);
    assert_eq!(p.selected(), 1);
}

#[test]
fn enter_drive_lists_children_with_up_row() {
    let mut p = Picker::new(&Fake);
    p.enter(&Fake); // into "/"
    assert_eq!(p.location(), Some(Path::new("/")));
    assert_eq!(p.entries().len(), 3); // "..", alpha, beta
    assert_eq!(p.entries()[0].kind, EntryKind::Up);
    assert_eq!(p.entries()[1].label, "alpha");
}

#[test]
fn up_from_top_returns_to_roots() {
    let mut p = Picker::new(&Fake);
    p.enter(&Fake); // into "/"
                    // selected is the ".." row; entering it goes up. "/" has no parent -> roots.
    p.enter(&Fake);
    assert_eq!(p.location(), None);
    assert_eq!(p.entries().len(), 2);
}

#[test]
fn nested_up_goes_to_parent() {
    let mut p = Picker::new(&Fake);
    p.move_by(1); // select "home" bookmark (/home/me)
    p.enter(&Fake); // into /home/me
    assert_eq!(p.location(), Some(Path::new("/home/me")));
    p.move_by(1); // select "alpha"
    p.enter(&Fake); // into /home/me/alpha
    assert_eq!(p.location(), Some(Path::new("/home/me/alpha")));
    // ".." is selected (index 0) after entering; go up to parent.
    p.enter(&Fake);
    assert_eq!(p.location(), Some(Path::new("/home/me")));
}

#[test]
fn target_resolves_selection() {
    let mut p = Picker::new(&Fake);
    p.move_by(1);
    // A bookmark row targets its own path.
    assert_eq!(p.target().as_deref(), Some(Path::new("/home/me")));
    p.enter(&Fake); // into /home/me, ".." selected
                    // The ".." row targets the current location.
    assert_eq!(p.target().as_deref(), Some(Path::new("/home/me")));
}
