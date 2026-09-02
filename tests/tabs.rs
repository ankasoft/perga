//! Tabs: creation, closing, switching, and the independence of what each one
//! owns.

mod common;

use std::path::PathBuf;

use common::{app_with, frame, vault};
use perga::action::Action;
use perga::app::{truncate_middle, App, Severity, MAX_TABS};

/// The path open in a given tab.
fn path_in(app: &App, tab: usize) -> Option<PathBuf> {
    app.tabs[tab].doc.as_ref().map(|doc| doc.path.clone())
}

/// Focus the link whose target is `target`.
fn focus_link(app: &mut App, target: &str) {
    let count = app.tab().doc.as_ref().expect("a document").links.len();

    for _ in 0..count {
        app.update(Action::NextLink);
        let focused = app
            .tab()
            .focused_link
            .and_then(|i| app.tab().doc.as_ref().map(|d| d.links[i].target.clone()));
        if focused.as_deref() == Some(target) {
            return;
        }
    }

    panic!("no link to `{target}` in this document");
}

#[test]
fn a_new_tab_opens_on_the_welcome_screen_and_takes_focus() {
    let mut app = app_with("README.md", 120, 40);

    app.update(Action::NewTab);
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.active_tab, 1);
    assert!(app.tab().doc.is_none());
    assert_eq!(app.tab().label(), "welcome");

    // The frame still paints, and the tab bar has appeared with it.
    assert!(frame(&mut app, 120, 40).contains("welcome"));
}

#[test]
fn tabs_switch_and_wrap_in_both_directions() {
    let mut app = app_with("README.md", 120, 40);
    app.update(Action::NewTab);
    app.update(Action::NewTab);
    assert_eq!(app.active_tab, 2);

    app.update(Action::NextTab);
    assert_eq!(app.active_tab, 0, "the last tab wraps to the first");

    app.update(Action::PrevTab);
    assert_eq!(app.active_tab, 2, "and the first wraps back to the last");

    app.update(Action::PrevTab);
    assert_eq!(app.active_tab, 1);
}

#[test]
fn closing_a_tab_lands_on_the_one_to_its_left() {
    let mut app = app_with("README.md", 120, 40);
    app.update(Action::NewTab);
    app.update(Action::NewTab);

    app.update(Action::CloseTab);
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.active_tab, 1);

    app.update(Action::PrevTab);
    app.update(Action::CloseTab);
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.active_tab, 0);
}

#[test]
fn closing_the_last_tab_quits() {
    let mut app = app_with("README.md", 120, 40);

    app.update(Action::CloseTab);
    assert!(app.should_quit);
    assert_eq!(app.exit_code, 0);
}

#[test]
fn the_twenty_first_tab_reuses_the_active_one() {
    let mut app = app_with("README.md", 120, 40);

    for _ in 1..MAX_TABS {
        app.update(Action::NewTab);
    }
    assert_eq!(app.tabs.len(), MAX_TABS);

    app.update(Action::NewTab);
    assert_eq!(app.tabs.len(), MAX_TABS, "the cap holds");

    let (message, severity) = app.status.message.clone().expect("a message");
    assert!(message.contains("20"), "{message}");
    assert_eq!(severity, Severity::Warning);
}

/// The cross-contamination test from Section 15.3.
#[test]
fn two_tabs_keep_their_own_history_scroll_and_document() {
    let mut app = app_with("README.md", 80, 14);
    frame(&mut app, 80, 14);

    // Tab 0: follow a link, so it has a history and a scrolled position.
    focus_link(&mut app, "docs/api/auth.md");
    app.update(Action::FollowLink);
    frame(&mut app, 80, 14);
    for _ in 0..3 {
        app.update(Action::ScrollLineDown);
    }
    frame(&mut app, 80, 14);
    let first_scroll = app.tab().scroll;
    assert!(first_scroll > 0);
    assert_eq!(app.tabs[0].history.back_len(), 1);

    // Tab 1: a different document, scrolled somewhere else, with no history.
    app.update(Action::NewTab);
    app.update(Action::OpenPath(PathBuf::from("gfm.md")));
    frame(&mut app, 80, 14);
    for _ in 0..8 {
        app.update(Action::ScrollLineDown);
    }
    frame(&mut app, 80, 14);
    let second_scroll = app.tab().scroll;

    assert_ne!(first_scroll, second_scroll);
    assert_eq!(app.tabs[1].history.back_len(), 0, "a new tab starts fresh");

    // Going back in the second tab must not touch the first.
    app.update(Action::HistoryBack);
    assert_eq!(path_in(&app, 1), Some(vault().join("gfm.md")));
    assert_eq!(path_in(&app, 0), Some(vault().join("docs/api/auth.md")));

    // ...and switching back finds the first exactly as it was left.
    app.update(Action::PrevTab);
    assert_eq!(app.active_tab, 0);
    assert_eq!(app.tab().scroll, first_scroll);
    assert_eq!(app.tabs[0].history.back_len(), 1);

    app.update(Action::HistoryBack);
    assert_eq!(path_in(&app, 0), Some(vault().join("README.md")));
    assert_eq!(
        path_in(&app, 1),
        Some(vault().join("gfm.md")),
        "the other tab did not move"
    );
}

#[test]
fn ctrl_enter_opens_a_link_in_a_background_tab() {
    let mut app = app_with("README.md", 120, 40);
    frame(&mut app, 120, 40);

    focus_link(&mut app, "docs/api/auth.md");
    app.update(Action::FollowLinkInNewTab);

    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.active_tab, 0, "a background tab does not steal focus");
    assert_eq!(path_in(&app, 0), Some(vault().join("README.md")));
    assert_eq!(path_in(&app, 1), Some(vault().join("docs/api/auth.md")));
}

#[test]
fn an_external_link_in_a_new_tab_falls_back_to_following_it() {
    let mut app = app_with("broken-links.md", 120, 40);
    frame(&mut app, 120, 40);

    focus_link(&mut app, "does-not-exist.md");
    app.update(Action::FollowLinkInNewTab);

    assert_eq!(app.tabs.len(), 1, "a broken link is not worth a tab");
    assert_eq!(
        app.status.message.as_ref().map(|(m, _)| m.as_str()),
        Some("Cannot resolve: does-not-exist.md")
    );
}

#[test]
fn a_tab_label_is_the_frontmatter_title_then_the_file_stem() {
    let mut app = app_with("README.md", 120, 40);
    assert_eq!(app.tab().label(), "Fixture Vault");

    app.update(Action::OpenPath(PathBuf::from("gfm.md")));
    assert_eq!(app.tab().label(), "gfm");
}

#[test]
fn a_dirty_tab_wears_a_marker() {
    let mut app = app_with("README.md", 120, 40);
    assert_eq!(app.tab().display_label(), "Fixture Vault");

    app.tabs[0].dirty = true;
    assert_eq!(app.tab().display_label(), "● Fixture Vault");
}

#[test]
fn a_long_label_is_elided_in_the_middle() {
    assert_eq!(truncate_middle("short", 20), "short");
    assert_eq!(
        truncate_middle("exactly-twenty-chars", 20),
        "exactly-twenty-chars"
    );

    // The ends are what tell two dated notes apart, so the middle goes.
    let elided = truncate_middle("2024-01-conference-notes", 20);
    assert!(elided.starts_with("2024-01-c"), "{elided}");
    assert!(elided.ends_with("nce-notes"), "{elided}");
    assert!(elided.contains('…'));
}

#[test]
fn the_tab_bar_appears_with_the_second_tab_and_shows_both() {
    let mut app = app_with("README.md", 120, 40);
    assert!(app.frames().tabs.is_none(), "one tab needs no bar");

    app.update(Action::NewTab);
    app.update(Action::OpenPath(PathBuf::from("gfm.md")));

    assert!(app.frames().tabs.is_some());
    let painted = frame(&mut app, 120, 40);
    assert!(painted.contains("Fixture Vault"), "{painted}");
    assert!(painted.contains("gfm"));
}

#[test]
fn twenty_tabs_still_draw_a_bar_that_fits() {
    let mut app = app_with("README.md", 80, 24);
    for _ in 1..MAX_TABS {
        app.update(Action::NewTab);
    }

    let painted = frame(&mut app, 80, 24);
    for line in painted.lines() {
        assert!(
            line.chars().count() <= 80,
            "a tab bar row overflowed: {line:?}"
        );
    }
}
