//! Wiki-links, the backlink index, and its cache.
//!
//! Every acceptance criterion in Section 9.6 is asserted here, apart from the
//! two that are timing rather than behaviour; those are measured by
//! `benches/vault.rs`, for the reason in Section 15.6.

mod common;

use std::path::{Path, PathBuf};

use common::{app_with, frame, vault, vault_app};
use perga::action::Action;
use perga::app::{App, Overlay};
use perga::doc::links::LinkKind;
use perga::ui::sidebar::SidebarMode;
use perga::vault::index::{Index, WikiResolution};

/// A reader with the fixture vault walked, indexed, and measured once.
fn reader(name: &str) -> App {
    let mut app = app_with(name, 120, 40);
    app.index_now();
    frame(&mut app, 120, 40);
    app
}

/// The path of the document in the active tab.
fn open_path(app: &App) -> Option<PathBuf> {
    app.tab().doc.as_ref().map(|doc| doc.path.clone())
}

/// Focus the wiki-link whose target is `target`.
fn focus_wiki(app: &mut App, target: &str) {
    let count = app.tab().doc.as_ref().expect("a document").links.len();

    for _ in 0..count {
        app.update(Action::NextLink);
        let focused = app.tab().focused_link.and_then(|i| {
            app.tab()
                .doc
                .as_ref()
                .map(|d| (d.links[i].target.clone(), d.links[i].kind))
        });
        if focused.as_ref().map(|(t, k)| (t.as_str(), *k)) == Some((target, LinkKind::Wiki)) {
            return;
        }
    }

    panic!("no wiki-link to `{target}` in this document");
}

#[test]
fn all_four_spellings_reach_the_page_they_name() {
    let mut app = reader("wiki.md");

    for target in [
        "Token Rotation",
        "Token Rotation|a display text",
        "docs/guides/Token Rotation",
        "token rotation",
    ] {
        // The display text is not part of the target; the extractor split it
        // off already.
        let target = target.split('|').next().unwrap();

        let mut app = reader("wiki.md");
        focus_wiki(&mut app, target);
        app.update(Action::FollowLink);

        assert_eq!(
            open_path(&app),
            Some(vault().join("docs/guides/Token Rotation.md")),
            "`[[{target}]]` did not resolve"
        );
    }

    // ...and a heading fragment opens the page and scrolls to the heading.
    focus_wiki(&mut app, "Setup#Setup");
    app.update(Action::FollowLink);
    assert_eq!(open_path(&app), Some(vault().join("docs/guides/setup.md")));
}

/// Acceptance: ambiguous targets produce a disambiguation overlay, not a
/// silent pick.
#[test]
fn an_ambiguous_page_asks_rather_than_guessing() {
    let mut app = reader("wiki.md");

    focus_wiki(&mut app, "Ambiguous");
    app.update(Action::FollowLink);

    let Some(Overlay::Disambiguate {
        page, candidates, ..
    }) = app.overlay.clone()
    else {
        panic!("a silent pick would send the reader to the wrong note");
    };
    assert_eq!(page, "Ambiguous");
    assert_eq!(candidates.len(), 2);
    assert_eq!(open_path(&app), Some(vault().join("wiki.md")));

    // The overlay names both, and choosing one opens it.
    let painted = frame(&mut app, 120, 40);
    assert!(painted.contains("matches 2 pages"), "{painted}");

    app.update(Action::ChooseCandidate(1));
    assert!(app.overlay.is_none());
    assert_eq!(
        open_path(&app),
        Some(vault().join("docs/guides/ambiguous.md"))
    );
}

/// Section 9.11: a broken wiki-link is the one place perga offers to create a
/// file, and it asks before it does.
#[test]
fn a_page_nobody_has_written_yet_offers_to_create_it() {
    let mut app = reader("wiki.md");
    let missing = vault().join("Not Yet Written.md");

    focus_wiki(&mut app, "Not Yet Written");
    app.update(Action::FollowLink);

    let Some(Overlay::Confirm { question, .. }) = &app.overlay else {
        panic!("a broken wiki-link offers to create the page");
    };
    assert!(question.contains("Not Yet Written.md"), "{question}");
    assert!(
        !missing.exists(),
        "nothing is created until it is confirmed"
    );

    // Declining leaves the vault alone.
    app.update(Action::Escape);
    assert!(!missing.exists());
}

#[test]
fn following_a_wiki_link_before_the_index_is_ready_waits_rather_than_guessing() {
    // Walked but not indexed: exactly the first half-second of a cold start.
    let mut app = app_with("wiki.md", 120, 40);
    frame(&mut app, 120, 40);
    assert!(!app.vault.index.ready);

    focus_wiki(&mut app, "Token Rotation");
    app.update(Action::FollowLink);

    assert_eq!(
        app.status.message.as_ref().map(|(m, _)| m.as_str()),
        Some("Still indexing…")
    );
    assert_eq!(open_path(&app), Some(vault().join("wiki.md")));
}

#[test]
fn wiki_links_can_be_turned_off_entirely() {
    let mut app = reader("wiki.md");
    app.wikilinks.enabled = false;

    focus_wiki(&mut app, "Token Rotation");
    app.update(Action::FollowLink);

    assert_eq!(
        app.status.message.as_ref().map(|(m, _)| m.as_str()),
        Some("Wiki-links are disabled")
    );
}

// -- Backlinks -------------------------------------------------------------

#[test]
fn the_links_mode_shows_outgoing_links_and_backlinks() {
    let mut app = reader("docs/guides/Token Rotation.md");
    app.update(Action::SetSidebarMode(SidebarMode::Links));

    let painted = frame(&mut app, 120, 40);
    assert!(painted.contains("outgoing"), "{painted}");
    assert!(painted.contains("backlinks"), "{painted}");
    // `wiki.md` and `docs/api/auth.md` both link here.
    assert!(painted.contains("wiki.md"), "{painted}");
}

#[test]
fn a_broken_link_is_marked_in_the_links_mode() {
    let mut app = reader("broken-links.md");
    app.update(Action::SetSidebarMode(SidebarMode::Links));

    let painted = frame(&mut app, 120, 40);
    assert!(painted.contains('✗'), "{painted}");
}

#[test]
fn the_links_mode_reports_progress_while_the_index_builds() {
    let mut app = app_with("wiki.md", 120, 40);
    app.vault.index.total = Some(42);
    app.update(Action::SetSidebarMode(SidebarMode::Links));

    let painted = frame(&mut app, 120, 40);
    assert!(painted.contains("indexing…"), "{painted}");
    assert!(painted.contains("/42 files"), "{painted}");
}

/// Acceptance: deleting a file removes it from the index and marks inbound
/// links broken.
#[test]
fn a_deleted_page_leaves_its_inbound_links_broken() {
    let mut app = reader("wiki.md");

    let source = Path::new("wiki.md");
    assert!(matches!(
        app.vault
            .index
            .resolve_wiki("Token Rotation", source, &app.wikilinks),
        WikiResolution::Found { .. }
    ));

    app.vault
        .index
        .remove(Path::new("docs/guides/Token Rotation.md"));

    assert!(matches!(
        app.vault
            .index
            .resolve_wiki("Token Rotation", source, &app.wikilinks),
        WikiResolution::Missing { .. }
    ));

    focus_wiki(&mut app, "Token Rotation");
    app.update(Action::FollowLink);
    assert_eq!(open_path(&app), Some(vault().join("wiki.md")));
}

/// Acceptance: editing a file updates its backlinks.
#[test]
fn reindexing_one_file_updates_the_backlinks_it_produces() {
    let mut app = reader("wiki.md");
    let target = Path::new("docs/guides/Token Rotation.md");

    let before = app.vault.index.backlinks(target, &app.wikilinks).len();
    assert!(before > 0);

    // The document stops linking there.
    app.vault.index.insert(
        PathBuf::from("wiki.md"),
        perga::vault::index::entry_for("# Wiki-links\n\nNothing here now.\n", None, 0),
    );

    let after = app.vault.index.backlinks(target, &app.wikilinks);
    assert!(after.len() < before);
    assert!(
        after.iter().all(|b| b.source != Path::new("wiki.md")),
        "the document that stopped linking is still listed"
    );
}

// -- The cache -------------------------------------------------------------

#[test]
fn a_warm_start_reparses_only_what_changed() {
    let mut app = vault_app(120, 40);
    app.index_now();

    let cache = app.vault.index.to_cache();
    assert!(cache.files.len() > 5);

    // A second run, restored from that cache, needs nothing reparsed.
    let restored = Index::from_cache(cache);
    let stale: Vec<&PathBuf> = app
        .vault
        .markdown
        .iter()
        .filter(|(path, mtime, size)| !restored.is_current(path, *mtime, *size))
        .map(|(path, _, _)| path)
        .collect();

    assert!(
        stale.is_empty(),
        "a warm start would reparse {stale:?} for no reason"
    );
}

#[test]
fn a_cache_written_for_a_changed_file_is_not_trusted() {
    let mut app = vault_app(120, 40);
    app.index_now();

    let restored = Index::from_cache(app.vault.index.to_cache());
    let (path, mtime, size) = app.vault.markdown[0].clone();

    assert!(restored.is_current(&path, mtime, size));
    assert!(!restored.is_current(&path, mtime, size + 1), "size changed");
    assert!(
        !restored.is_current(
            &path,
            mtime.map(|t| t + std::time::Duration::from_secs(60)),
            size
        ),
        "mtime changed"
    );
}
