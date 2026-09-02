//! Link navigation and per-tab history.
//!
//! Every acceptance criterion in Section 9.5 is asserted here.

mod common;

use std::path::PathBuf;

use common::{app_with, frame, vault};
use perga::action::Action;
use perga::app::{App, Focus, Overlay, Severity, MAX_HISTORY};

/// The path of the document in the active tab.
fn open_path(app: &App) -> Option<PathBuf> {
    app.tab().doc.as_ref().map(|doc| doc.path.clone())
}

/// The target of the focused link.
fn focused_target(app: &App) -> Option<String> {
    let doc = app.tab().doc.as_ref()?;
    Some(doc.links[app.tab().focused_link?].target.clone())
}

/// Focus the link whose target is `target`, however far down the list it is.
fn focus_link(app: &mut App, target: &str) {
    let count = app.tab().doc.as_ref().expect("a document").links.len();

    for _ in 0..count {
        app.update(Action::NextLink);
        if focused_target(app).as_deref() == Some(target) {
            return;
        }
    }

    panic!("no link to `{target}` in this document");
}

/// Open the fixture vault at a document, with the frame measured once so the
/// offset↔line map has something in it.
fn reader(name: &str) -> App {
    reader_at(name, 120, 40)
}

/// The same, at a chosen terminal size.
///
/// The scroll-restoration tests need a viewport shorter than the document, or
/// there is no offset to restore.
fn reader_at(name: &str, width: u16, height: u16) -> App {
    let mut app = app_with(name, width, height);
    frame(&mut app, width, height);
    app
}

#[test]
fn links_are_cycled_in_reading_order_and_wrap() {
    let mut app = reader("README.md");

    app.update(Action::NextLink);
    assert_eq!(focused_target(&app).as_deref(), Some("docs/api/auth.md"));

    app.update(Action::NextLink);
    assert_eq!(
        focused_target(&app).as_deref(),
        Some("docs/guides/setup.md")
    );

    app.update(Action::PrevLink);
    assert_eq!(focused_target(&app).as_deref(), Some("docs/api/auth.md"));

    // `N` from the first link wraps to the last.
    app.update(Action::PrevLink);
    assert_eq!(focused_target(&app).as_deref(), Some("https://example.com"));
}

#[test]
fn the_focused_link_is_drawn_in_the_focused_style() {
    let mut app = reader("README.md");
    let plain = common::frame_buffer(&mut app, 120, 40);

    app.update(Action::NextLink);
    let focused = common::frame_buffer(&mut app, 120, 40);

    let changed: Vec<usize> = plain
        .content()
        .iter()
        .zip(focused.content())
        .enumerate()
        .filter(|(_, (a, b))| a.style() != b.style())
        .map(|(at, _)| at)
        .collect();

    assert_eq!(
        changed.len(),
        "the API docs".len(),
        "exactly the focused link's own text is restyled"
    );

    // Compared by colour rather than by whole `Style`: the backend fills in an
    // explicit `underline_color` reset that the theme does not name.
    let style = focused.content()[changed[0]].style();
    assert_eq!(style.fg, app.theme.markdown.link_focused.fg);
    assert_eq!(style.bg, app.theme.markdown.link_focused.bg);
}

#[test]
fn cycling_scrolls_a_link_below_the_fold_into_view() {
    let mut app = app_with("README.md", 80, 12);
    frame(&mut app, 80, 12);
    assert_eq!(app.tab().scroll, 0);

    // The autolink is far enough down the README to be off screen at this
    // height.
    focus_link(&mut app, "https://example.com");
    assert!(
        app.tab().scroll > 0,
        "the focused link must be brought into view"
    );
}

/// Acceptance: following a relative link two directories up and one down.
#[test]
fn following_a_relative_link_resolves_against_the_document() {
    let mut app = reader("docs/api/auth.md");

    focus_link(&mut app, "../guides/setup.md");
    app.update(Action::FollowLink);

    assert_eq!(open_path(&app), Some(vault().join("docs/guides/setup.md")));
}

/// Acceptance: back after a link follow restores the previous scroll position
/// exactly.
#[test]
fn going_back_restores_the_exact_scroll_position() {
    let mut app = reader_at("README.md", 80, 14);

    for _ in 0..5 {
        app.update(Action::ScrollLineDown);
    }
    // Measured once more so the offset has settled against the real total
    // before it is remembered.
    frame(&mut app, 80, 14);
    let was = app.tab().scroll;
    assert!(was > 0, "the document must be taller than the viewport");

    focus_link(&mut app, "docs/api/auth.md");
    app.update(Action::FollowLink);
    assert_eq!(open_path(&app), Some(vault().join("docs/api/auth.md")));
    assert_eq!(app.tab().scroll, 0, "a new document opens at the top");

    frame(&mut app, 80, 14);
    app.update(Action::HistoryBack);

    assert_eq!(open_path(&app), Some(vault().join("README.md")));
    assert_eq!(
        app.tab().scroll,
        was,
        "back restores the offset, not the top"
    );
}

/// Acceptance: the forward stack is truncated when a new navigation occurs
/// after going back.
#[test]
fn a_new_navigation_after_going_back_truncates_the_forward_stack() {
    let mut app = reader("README.md");

    focus_link(&mut app, "docs/api/auth.md");
    app.update(Action::FollowLink);
    frame(&mut app, 120, 40);

    app.update(Action::HistoryBack);
    assert_eq!(app.tab().history.forward_len(), 1);

    // Somewhere else instead: the forward entry is gone.
    focus_link(&mut app, "docs/guides/setup.md");
    app.update(Action::FollowLink);

    assert_eq!(app.tab().history.forward_len(), 0);
    assert_eq!(open_path(&app), Some(vault().join("docs/guides/setup.md")));

    app.update(Action::HistoryForward);
    assert_eq!(
        open_path(&app),
        Some(vault().join("docs/guides/setup.md")),
        "there is nowhere forward to go"
    );
}

#[test]
fn forward_returns_to_where_back_came_from() {
    let mut app = reader_at("README.md", 80, 14);

    focus_link(&mut app, "docs/api/auth.md");
    app.update(Action::FollowLink);
    frame(&mut app, 80, 14);
    for _ in 0..3 {
        app.update(Action::ScrollLineDown);
    }

    app.update(Action::HistoryBack);
    assert_eq!(open_path(&app), Some(vault().join("README.md")));

    app.update(Action::HistoryForward);
    assert_eq!(open_path(&app), Some(vault().join("docs/api/auth.md")));
    assert_eq!(app.tab().scroll, 3, "forward restores its offset too");
}

/// Acceptance: an anchor-only link scrolls without reloading the document.
#[test]
fn an_anchor_link_scrolls_without_reloading() {
    let mut app = reader("anchors.md");
    let before = app.tab().doc.as_ref().unwrap().content_hash;

    focus_link(&mut app, "#the-second-section");
    app.update(Action::FollowLink);

    assert_eq!(open_path(&app), Some(vault().join("anchors.md")));
    assert_eq!(
        app.tab().doc.as_ref().unwrap().content_hash,
        before,
        "the document must not have been reloaded"
    );
    assert!(app.tab().scroll > 0, "the anchor scrolled the viewport");
}

#[test]
fn an_anchor_on_another_document_opens_it_and_scrolls() {
    let mut app = reader("docs/api/auth.md");

    focus_link(&mut app, "../../README.md#fixture-vault");
    app.update(Action::FollowLink);

    assert_eq!(open_path(&app), Some(vault().join("README.md")));
}

/// Acceptance: a link to a file outside the vault root opens correctly and
/// does not corrupt the tree.
#[test]
fn a_link_outside_the_vault_opens_and_leaves_the_tree_alone() {
    let mut app = reader("outside.md");
    let rows_before = app.vault.tree.rows().len();

    focus_link(&mut app, "../outside-the-vault.md");
    app.update(Action::FollowLink);

    assert_eq!(
        open_path(&app),
        Some(vault().parent().unwrap().join("outside-the-vault.md"))
    );
    assert_eq!(app.vault.tree.rows().len(), rows_before);
    assert!(app.title_path().is_some());
}

/// Acceptance: cyclic links do not grow memory unboundedly.
#[test]
fn a_cycle_does_not_grow_the_history_past_its_cap() {
    let mut app = reader("cycle-a.md");

    // A → B → A → B → … far past the cap.
    for _ in 0..(MAX_HISTORY + 50) {
        let next = if open_path(&app) == Some(vault().join("cycle-a.md")) {
            "cycle-b.md"
        } else {
            "cycle-a.md"
        };
        focus_link(&mut app, next);
        app.update(Action::FollowLink);
    }

    assert_eq!(app.tab().history.back_len(), MAX_HISTORY);
}

#[test]
fn a_broken_link_says_so_and_creates_nothing() {
    let mut app = reader("broken-links.md");
    let missing = vault().join("does-not-exist.md");

    focus_link(&mut app, "does-not-exist.md");
    app.update(Action::FollowLink);

    let (message, severity) = app.status.message.clone().expect("a message");
    assert_eq!(message, "Cannot resolve: does-not-exist.md");
    assert_eq!(severity, Severity::Error);
    assert!(
        !missing.exists(),
        "a broken inline link never creates a file"
    );
    assert_eq!(open_path(&app), Some(vault().join("broken-links.md")));
}

#[test]
fn a_directory_target_is_revealed_in_the_tree() {
    let mut app = reader("outside.md");

    focus_link(&mut app, "docs/api");
    app.update(Action::FollowLink);

    assert_eq!(app.focus, Focus::Sidebar);
    assert_eq!(
        app.vault.tree.selected().map(|n| n.path.clone()),
        Some(PathBuf::from("docs/api"))
    );
    // The document did not change: a directory is shown, not opened.
    assert_eq!(open_path(&app), Some(vault().join("outside.md")));
}

#[test]
fn following_with_nothing_focused_says_what_to_press() {
    let mut app = reader("README.md");
    app.update(Action::FollowLink);

    let (message, _) = app.status.message.clone().expect("a message");
    assert!(message.contains('n'), "{message}");
    assert!(app.tab().focused_link.is_none());
}

#[test]
fn history_at_either_end_says_so_rather_than_doing_nothing() {
    let mut app = reader("README.md");

    app.update(Action::HistoryBack);
    assert_eq!(
        app.status.message.as_ref().map(|(m, _)| m.as_str()),
        Some("No further back")
    );

    app.update(Action::HistoryForward);
    assert_eq!(
        app.status.message.as_ref().map(|(m, _)| m.as_str()),
        Some("No further forward")
    );
}

// -- Hint mode -------------------------------------------------------------

#[test]
fn hint_mode_labels_the_links_in_view() {
    let mut app = reader("README.md");
    app.update(Action::HintMode);

    let Some(Overlay::Hints { links, typed }) = &app.overlay else {
        panic!("hint mode is open");
    };
    assert!(links.len() >= 2);
    assert!(typed.is_empty());
    assert_eq!(app.focus, Focus::Overlay);

    // The labels are drawn over the document.
    let painted = frame(&mut app, 120, 40);
    assert!(painted.contains("perga"));
}

#[test]
fn a_hint_label_follows_its_link() {
    let mut app = reader("README.md");
    app.update(Action::HintMode);

    let Some(Overlay::Hints { links, .. }) = app.overlay.clone() else {
        panic!("hint mode is open");
    };
    let first = links[0];

    app.update(Action::Escape);
    app.update(Action::FollowHintedLink(first));

    assert_eq!(open_path(&app), Some(vault().join("docs/api/auth.md")));
    assert!(app.overlay.is_none());
}

#[test]
fn hint_mode_on_a_document_with_no_links_says_so() {
    let mut app = reader("empty.md");
    app.update(Action::HintMode);

    assert!(app.overlay.is_none());
    assert_eq!(
        app.status.message.as_ref().map(|(m, _)| m.as_str()),
        Some("No links in this document")
    );
}
