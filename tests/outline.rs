//! The outline sidebar mode and find-in-document.

mod common;

use common::{app_with, frame, frame_buffer};
use perga::action::Action;
use perga::app::{App, Overlay};
use perga::doc::outline::slugify;
use perga::ui::overlay::prompt::TextEdit;
use perga::ui::sidebar::SidebarMode;

/// A reader looking at a document with headings, measured once.
fn reader(name: &str, width: u16, height: u16) -> App {
    let mut app = app_with(name, width, height);
    frame(&mut app, width, height);
    app
}

/// Type a query into the find bar.
fn type_query(app: &mut App, text: &str) {
    for c in text.chars() {
        app.update(Action::FindEdit(TextEdit::Insert(c)));
    }
}

// -- Outline ---------------------------------------------------------------

#[test]
fn the_outline_lists_every_heading_indented_by_level() {
    let mut app = reader("anchors.md", 120, 40);
    app.update(Action::SetSidebarMode(SidebarMode::Outline));

    let painted = frame(&mut app, 120, 40);
    assert!(painted.contains("4 headings"), "{painted}");
    assert!(painted.contains("Anchors"));
    assert!(
        painted.contains("  The first section"),
        "a level-2 heading is indented one step"
    );
}

#[test]
fn the_outline_selection_moves_and_stops_at_the_ends() {
    let mut app = reader("anchors.md", 120, 40);
    app.update(Action::SetSidebarMode(SidebarMode::Outline));
    assert_eq!(app.sidebar.outline_selected, 0);

    app.update(Action::SidebarDown);
    assert_eq!(app.sidebar.outline_selected, 1);

    for _ in 0..20 {
        app.update(Action::SidebarDown);
    }
    assert_eq!(app.sidebar.outline_selected, 3, "the last heading");

    for _ in 0..20 {
        app.update(Action::SidebarUp);
    }
    assert_eq!(app.sidebar.outline_selected, 0);
}

#[test]
fn activating_a_heading_scrolls_the_viewport_to_it() {
    let mut app = reader("anchors.md", 80, 14);
    app.update(Action::SetSidebarMode(SidebarMode::Outline));

    for _ in 0..2 {
        app.update(Action::SidebarDown);
    }
    app.update(Action::SidebarActivate);

    assert!(app.tab().scroll > 0, "the outline scrolled the document");
}

/// M6's definition of done: an anchor and the outline use one slug
/// implementation, so they can never disagree about where a heading is.
#[test]
fn an_anchor_and_the_outline_land_on_the_same_line() {
    let mut app = reader("anchors.md", 80, 14);

    // Through the outline.
    app.update(Action::SetSidebarMode(SidebarMode::Outline));
    for _ in 0..3 {
        app.update(Action::SidebarDown);
    }
    app.update(Action::SidebarActivate);
    let via_outline = app.tab().scroll;

    let heading = &app.tab().doc.as_ref().unwrap().outline[3];
    assert_eq!(heading.text, "Kurulum Kılavuzu");
    assert_eq!(heading.slug, slugify("Kurulum Kılavuzu"));
    let slug = heading.slug.clone();

    // ...and through the anchor link written against the same heading.
    let mut app = reader("anchors.md", 80, 14);
    let index = app
        .tab()
        .doc
        .as_ref()
        .unwrap()
        .links
        .iter()
        .position(|link| link.target == format!("#{slug}"))
        .expect("the fixture links to that heading by its slug");

    app.update(Action::FollowHintedLink(index));

    assert_eq!(app.tab().scroll, via_outline);
    assert!(via_outline > 0);
}

#[test]
fn the_current_heading_follows_the_scroll_position() {
    let mut app = reader("anchors.md", 80, 14);
    assert_eq!(app.current_heading(), 0);

    // Down past the first section's heading.
    for _ in 0..12 {
        app.update(Action::ScrollLineDown);
    }
    frame(&mut app, 80, 14);
    let after = app.current_heading();
    assert!(after > 0, "the outline highlight moved with the reader");

    app.update(Action::ScrollTop);
    frame(&mut app, 80, 14);
    assert_eq!(app.current_heading(), 0);
}

#[test]
fn a_document_with_no_headings_says_so() {
    let mut app = reader("empty.md", 120, 40);
    app.update(Action::SetSidebarMode(SidebarMode::Outline));

    let painted = frame(&mut app, 120, 40);
    assert!(painted.contains("no headings"), "{painted}");
}

#[test]
fn the_outline_selection_is_independent_of_the_tree_selection() {
    let mut app = reader("anchors.md", 120, 40);
    let tree_before = app.vault.tree.selected().map(|n| n.path.clone());

    app.update(Action::SetSidebarMode(SidebarMode::Outline));
    for _ in 0..3 {
        app.update(Action::SidebarDown);
    }

    assert_eq!(app.sidebar.outline_selected, 3);
    assert_eq!(
        app.vault.tree.selected().map(|n| n.path.clone()),
        tree_before,
        "moving down the outline moved the tree cursor"
    );
}

// -- Find in document -------------------------------------------------------

#[test]
fn find_counts_matches_as_the_query_is_typed() {
    let mut app = reader("anchors.md", 80, 14);

    app.update(Action::OpenFindInDocument);
    assert_eq!(app.overlay, Some(Overlay::Find));

    type_query(&mut app, "section");
    let find = app.tab().find.as_ref().expect("a find state");
    assert!(find.count() >= 3, "{}", find.count());
    assert_eq!(find.current, Some(0));

    // The count is on screen.
    let painted = frame(&mut app, 80, 14);
    assert!(painted.contains("/section"), "{painted}");
}

#[test]
fn find_is_case_insensitive_until_the_query_is_not() {
    let mut app = reader("anchors.md", 80, 14);

    // The fixture writes the phrase once as a heading and once in a link.
    app.update(Action::OpenFindInDocument);
    type_query(&mut app, "the second section");
    let insensitive = app.tab().find.as_ref().unwrap().count();

    app.update(Action::FindEdit(TextEdit::Clear));
    type_query(&mut app, "The second section");
    let sensitive = app.tab().find.as_ref().unwrap().count();

    assert!(insensitive > sensitive, "{insensitive} vs {sensitive}");
}

#[test]
fn cycling_matches_wraps_and_scrolls() {
    let mut app = reader("anchors.md", 80, 14);

    app.update(Action::OpenFindInDocument);
    type_query(&mut app, "second section");

    let count = app.tab().find.as_ref().unwrap().count();
    assert!(count >= 2);

    app.update(Action::FindNext);
    assert_eq!(app.tab().find.as_ref().unwrap().current, Some(1));
    assert!(app.tab().scroll > 0, "the match was scrolled into view");

    for _ in 0..count {
        app.update(Action::FindNext);
    }
    assert_eq!(app.tab().find.as_ref().unwrap().current, Some(1));

    app.update(Action::FindPrev);
    assert_eq!(app.tab().find.as_ref().unwrap().current, Some(0));
}

#[test]
fn matches_are_highlighted_where_they_are_drawn() {
    let mut app = reader("anchors.md", 80, 14);
    let plain = frame_buffer(&mut app, 80, 14);

    app.update(Action::OpenFindInDocument);
    type_query(&mut app, "anchor");
    let highlighted = frame_buffer(&mut app, 80, 14);

    let changed = plain
        .content()
        .iter()
        .zip(highlighted.content())
        .filter(|(a, b)| a.style() != b.style())
        .count();

    assert!(changed > 0, "nothing was highlighted");
}

#[test]
fn escape_closes_the_bar_and_clears_the_highlighting() {
    let mut app = reader("anchors.md", 80, 14);

    app.update(Action::OpenFindInDocument);
    type_query(&mut app, "section");
    app.update(Action::CloseFind);

    assert!(app.overlay.is_none());
    assert!(app.tab().find.is_none());
}

#[test]
fn find_state_belongs_to_the_tab_that_searched() {
    let mut app = reader("anchors.md", 80, 14);

    app.update(Action::OpenFindInDocument);
    type_query(&mut app, "section");
    assert!(app.tabs[0].find.is_some());

    app.update(Action::NewTab);
    assert!(app.tabs[1].find.is_none(), "a new tab is not searching");
    assert!(app.overlay.is_none(), "the bar did not follow the reader");

    app.update(Action::PrevTab);
    assert!(app.tabs[0].find.is_some(), "the first tab kept its query");
}

#[test]
fn a_query_that_matches_nothing_says_so_rather_than_moving() {
    let mut app = reader("anchors.md", 80, 14);

    app.update(Action::OpenFindInDocument);
    type_query(&mut app, "zzzznotpresent");

    let find = app.tab().find.as_ref().unwrap();
    assert_eq!(find.count(), 0);
    assert_eq!(find.position(), "no matches");
    assert_eq!(app.tab().scroll, 0);
}
