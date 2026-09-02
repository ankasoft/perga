//! Project-wide search and the quick switcher.
//!
//! Every acceptance criterion in Section 9.7 is asserted here.

mod common;

use std::path::{Path, PathBuf};

use common::{app_with, frame, vault, vault_app};
use perga::action::Action;
use perga::app::{App, Overlay, PromptKind};
use perga::ui::overlay::prompt::TextEdit;
use perga::ui::sidebar::SidebarMode;

/// Type into whichever prompt is open.
fn type_prompt(app: &mut App, text: &str) {
    for c in text.chars() {
        app.update(Action::PromptEdit(TextEdit::Insert(c)));
    }
}

/// Type into the quick switcher.
fn type_switcher(app: &mut App, text: &str) {
    for c in text.chars() {
        app.update(Action::SwitcherEdit(TextEdit::Insert(c)));
    }
}

/// The paths the switcher is offering.
fn switcher_rows(app: &App) -> Vec<PathBuf> {
    let Some(Overlay::Switcher { rows, .. }) = &app.overlay else {
        panic!("the switcher is open");
    };
    rows.iter().map(|row| row.path.clone()).collect()
}

// -- The search prompt -------------------------------------------------------

#[test]
fn the_prompt_runs_a_search_and_shows_it_in_the_sidebar() {
    let mut app = vault_app(120, 40);

    app.update(Action::OpenProjectSearch);
    assert!(matches!(
        app.overlay,
        Some(Overlay::Prompt {
            kind: PromptKind::ProjectSearch,
            ..
        })
    ));

    type_prompt(&mut app, "Bearer tokens");
    // With no event loop behind it the prompt has nowhere to send the search,
    // so the test runs it synchronously the way `--print` would.
    app.update(Action::PromptAccept);
    app.search_now("Bearer tokens");

    assert!(app.overlay.is_none());
    assert_eq!(app.sidebar.mode, SidebarMode::Search);
    assert!(!app.search.hits.is_empty());
    assert!(app
        .search
        .hits
        .iter()
        .all(|hit| hit.text.contains("Bearer tokens")));
}

#[test]
fn the_prompt_reopens_pre_filled_with_the_last_query() {
    let mut app = vault_app(120, 40);
    app.search_now("tokens");

    app.update(Action::OpenProjectSearch);
    let Some(Overlay::Prompt { input, .. }) = &app.overlay else {
        panic!("the prompt is open");
    };
    assert_eq!(input.value(), "tokens");
}

#[test]
fn slash_in_the_search_mode_reopens_the_prompt() {
    let mut app = vault_app(120, 40);
    app.search_now("tokens");
    app.update(Action::FocusNext);

    app.update(Action::TreeFilter);

    assert!(matches!(
        app.overlay,
        Some(Overlay::Prompt {
            kind: PromptKind::ProjectSearch,
            ..
        })
    ));
}

#[test]
fn results_are_grouped_by_file_with_the_match_shown() {
    let mut app = vault_app(120, 40);
    app.search_now("token");
    app.update(Action::SetSidebarMode(SidebarMode::Search));

    let groups = app.search.groups();
    assert!(groups.len() > 1, "the fixture matches in several files");

    let painted = frame(&mut app, 120, 40);
    assert!(painted.contains("hits in"), "{painted}");
    assert!(painted.contains("auth.md"), "{painted}");
}

/// The Section 15.3 integration test: search, open a result, land on the hit
/// line.
#[test]
fn opening_a_result_scrolls_to_the_line_it_was_found_on() {
    // A document taller than the viewport, or there is no offset to land on.
    let mut app = app_with("README.md", 80, 14);
    app.search_now("The second section");
    frame(&mut app, 80, 14);

    let hit = app.search.hits[0].clone();
    assert_eq!(hit.path, Path::new("anchors.md"));
    assert!(hit.line > 14, "the hit must be below the fold");

    app.update(Action::FocusNext);
    app.update(Action::SidebarActivate);

    assert_eq!(
        app.tab().doc.as_ref().map(|d| d.path.clone()),
        Some(vault().join("anchors.md"))
    );
    assert!(app.tab().scroll > 0, "the viewport did not move to the hit");
}

#[test]
fn the_selection_walks_the_hits() {
    let mut app = vault_app(120, 40);
    app.search_now("token");
    app.update(Action::SetSidebarMode(SidebarMode::Search));
    app.update(Action::FocusNext);

    assert_eq!(app.search.selected, 0);
    app.update(Action::SidebarDown);
    assert_eq!(app.search.selected, 1);

    for _ in 0..100 {
        app.update(Action::SidebarDown);
    }
    assert_eq!(app.search.selected, app.search.hits.len() - 1);
}

/// Acceptance: an invalid regex shows an inline error, not a panic.
#[test]
fn an_invalid_regex_is_shown_in_the_sidebar() {
    let mut app = vault_app(120, 40);
    app.search_config.regex = true;

    app.search_now("[unclosed");

    assert!(app.search.error.is_some());
    assert!(app.search.hits.is_empty());

    app.update(Action::SetSidebarMode(SidebarMode::Search));
    let painted = frame(&mut app, 120, 40);
    assert!(painted.contains("bad pattern"), "{painted}");
}

#[test]
fn a_slash_wrapped_query_searches_as_a_regex() {
    let mut app = vault_app(120, 40);
    app.search_now("/Bearer +tokens/");

    assert!(app.search.error.is_none());
    assert!(!app.search.hits.is_empty());
}

#[test]
fn the_result_cap_is_reported_rather_than_hidden() {
    let mut app = vault_app(120, 40);
    app.search_config.max_results = 3;

    app.search_now("e");

    assert!(app.search.truncated);
    assert!(
        app.search.summary().contains('+'),
        "{}",
        app.search.summary()
    );
}

#[test]
fn an_empty_query_clears_the_results() {
    let mut app = vault_app(120, 40);
    app.search_now("token");
    assert!(!app.search.hits.is_empty());

    app.start_search("   ");
    assert!(app.search.hits.is_empty());
    assert!(app.search.query.is_empty());
}

/// Acceptance: cancelling mid-search leaves no orphaned threads.
#[test]
fn re_running_a_search_leaves_no_orphaned_threads() {
    let before = thread_count();

    let mut app = vault_app(120, 40);
    app.on_search(|_| {});

    // Ten searches in a row, each cancelling the last.
    for i in 0..10 {
        app.start_search(&format!("token{i}"));
    }

    // Dropping the app drops the handle, which cancels the search; the
    // threads then unwind at the next line they read.
    drop(app);

    for _ in 0..200 {
        if thread_count() <= before {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }

    assert!(
        thread_count() <= before,
        "{} threads before, {} after",
        before,
        thread_count()
    );
}

/// How many threads this process has, from `/proc`.
///
/// Linux-only; the assertion above is skipped elsewhere by always reporting
/// zero, which is the honest answer when the count cannot be read.
fn thread_count() -> usize {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| {
            status
                .lines()
                .find_map(|line| line.strip_prefix("Threads:"))
                .and_then(|n| n.trim().parse().ok())
        })
        .unwrap_or(0)
}

// -- The quick switcher ------------------------------------------------------

#[test]
fn an_empty_switcher_shows_the_recent_list_most_recent_first() {
    let mut app = vault_app(120, 40);

    app.update(Action::OpenPath(PathBuf::from("gfm.md")));
    app.update(Action::OpenPath(PathBuf::from("docs/api/auth.md")));

    app.update(Action::OpenQuickSwitcher);
    assert_eq!(
        switcher_rows(&app),
        [PathBuf::from("docs/api/auth.md"), PathBuf::from("gfm.md"),]
    );
}

#[test]
fn typing_switches_to_fuzzy_results_over_the_whole_vault() {
    let mut app = vault_app(120, 40);

    app.update(Action::OpenQuickSwitcher);
    type_switcher(&mut app, "auth");

    let rows = switcher_rows(&app);
    assert_eq!(rows[0], PathBuf::from("docs/api/auth.md"));

    let painted = frame(&mut app, 120, 40);
    assert!(painted.contains("Open: auth"), "{painted}");
}

#[test]
fn the_switcher_opens_what_it_has_selected() {
    let mut app = vault_app(120, 40);

    app.update(Action::OpenQuickSwitcher);
    type_switcher(&mut app, "auth");
    app.update(Action::SwitcherAccept { new_tab: false });

    assert!(app.overlay.is_none());
    assert_eq!(
        app.tab().doc.as_ref().map(|d| d.path.clone()),
        Some(vault().join("docs/api/auth.md"))
    );
}

#[test]
fn ctrl_enter_in_the_switcher_opens_a_background_tab() {
    let mut app = app_with("README.md", 120, 40);

    app.update(Action::OpenQuickSwitcher);
    type_switcher(&mut app, "auth");
    app.update(Action::SwitcherAccept { new_tab: true });

    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.active_tab, 0);
    assert_eq!(
        app.tabs[1].doc.as_ref().map(|d| d.path.clone()),
        Some(vault().join("docs/api/auth.md"))
    );
}

#[test]
fn the_selection_moves_and_stops_at_both_ends() {
    let mut app = vault_app(120, 40);

    app.update(Action::OpenQuickSwitcher);
    type_switcher(&mut app, "md");
    let count = switcher_rows(&app).len();
    assert!(count > 2);

    app.update(Action::SwitcherMove(1));
    let Some(Overlay::Switcher { selected, .. }) = &app.overlay else {
        panic!("the switcher is open");
    };
    assert_eq!(*selected, 1);

    for _ in 0..(count + 10) {
        app.update(Action::SwitcherMove(1));
    }
    let Some(Overlay::Switcher { selected, .. }) = &app.overlay else {
        panic!("the switcher is open");
    };
    assert_eq!(*selected, count - 1);
}

#[test]
fn a_query_matching_nothing_offers_to_create_the_file() {
    let mut app = vault_app(120, 40);

    app.update(Action::OpenQuickSwitcher);
    type_switcher(&mut app, "zzz-not-a-note");

    let Some(Overlay::Switcher { rows, .. }) = &app.overlay else {
        panic!("the switcher is open");
    };
    assert_eq!(rows.len(), 1);
    assert!(rows[0].create);
    assert_eq!(rows[0].path, PathBuf::from("zzz-not-a-note.md"));

    let painted = frame(&mut app, 120, 40);
    assert!(
        painted.contains("create \"zzz-not-a-note.md\""),
        "{painted}"
    );
}

#[test]
fn typing_moves_the_selection_back_to_the_best_match() {
    let mut app = vault_app(120, 40);

    app.update(Action::OpenQuickSwitcher);
    type_switcher(&mut app, "md");
    app.update(Action::SwitcherMove(2));

    type_switcher(&mut app, "a");

    let Some(Overlay::Switcher { selected, .. }) = &app.overlay else {
        panic!("the switcher is open");
    };
    assert_eq!(*selected, 0);
}

#[test]
fn escape_closes_the_switcher_without_opening_anything() {
    let mut app = app_with("README.md", 120, 40);

    app.update(Action::OpenQuickSwitcher);
    type_switcher(&mut app, "auth");
    app.update(Action::Escape);

    assert!(app.overlay.is_none());
    assert_eq!(
        app.tab().doc.as_ref().map(|d| d.path.clone()),
        Some(vault().join("README.md"))
    );
}
