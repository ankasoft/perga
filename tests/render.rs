//! Snapshot tests of rendered frames.
//!
//! Rendering is a pure function of state, so every one of these drives the app
//! through `Action`s and then renders to `TestBackend`. No terminal is
//! involved.

mod common;

use common::{app, app_with, frame, SIZES};
use perga::action::Action;
use perga::ui::sidebar::SidebarMode;
use perga::ui::welcome::{LogoTier, MEDIUM_MIN_WIDTH, MIN_LOGO_HEIGHT};

#[test]
fn welcome_screen_at_every_size() {
    for (width, height) in SIZES {
        let mut app = app(width, height);
        insta::assert_snapshot!(
            format!("welcome_{width}x{height}"),
            frame(&mut app, width, height)
        );
    }
}

#[test]
fn sidebar_hidden() {
    let mut app = app(120, 40);
    app.update(Action::ToggleSidebar);
    insta::assert_snapshot!(frame(&mut app, 120, 40));
}

#[test]
fn sidebar_focused() {
    let mut app = common::vault_app(120, 40);
    app.update(Action::FocusNext);
    insta::assert_snapshot!(frame(&mut app, 120, 40));
}

#[test]
fn each_sidebar_mode() {
    for mode in SidebarMode::ALL {
        let mut app = common::vault_app(120, 40);
        app.update(Action::SetSidebarMode(mode));
        insta::assert_snapshot!(format!("sidebar_mode_{mode}"), frame(&mut app, 120, 40));
    }
}

/// The tree with the filter line open, and with non-Markdown files shown.
#[test]
fn files_sidebar_states() {
    let mut app = common::vault_app(120, 40);
    app.update(Action::TreeFilter);
    for c in "auth".chars() {
        app.update(Action::TreeFilterEdit(
            perga::ui::overlay::prompt::TextEdit::Insert(c),
        ));
    }
    insta::assert_snapshot!("files_filtered", frame(&mut app, 120, 40));

    let mut app = common::vault_app(120, 40);
    app.update(Action::TreeToggleAllFiles);
    insta::assert_snapshot!("files_all_files", frame(&mut app, 120, 40));
}

/// Hint mode, with a label over every link in view.
#[test]
fn link_hint_mode() {
    let mut app = common::app_with("README.md", 120, 40);
    frame(&mut app, 120, 40);
    app.update(Action::HintMode);
    insta::assert_snapshot!(frame(&mut app, 120, 40));
}

/// The outline mode, and the find bar with matches highlighted behind it.
#[test]
fn outline_and_find() {
    let mut app = common::app_with("anchors.md", 120, 40);
    frame(&mut app, 120, 40);
    app.update(Action::SetSidebarMode(
        perga::ui::sidebar::SidebarMode::Outline,
    ));
    insta::assert_snapshot!("sidebar_outline_populated", frame(&mut app, 120, 40));

    app.update(Action::OpenFindInDocument);
    for c in "section".chars() {
        app.update(Action::FindEdit(
            perga::ui::overlay::prompt::TextEdit::Insert(c),
        ));
    }
    insta::assert_snapshot!("find_bar", frame(&mut app, 120, 40));
}

/// The links mode, with outgoing links and backlinks.
#[test]
fn sidebar_links_populated() {
    let mut app = common::app_with("docs/guides/Token Rotation.md", 120, 40);
    app.index_now();
    frame(&mut app, 120, 40);
    app.update(Action::SetSidebarMode(
        perga::ui::sidebar::SidebarMode::Links,
    ));
    insta::assert_snapshot!(frame(&mut app, 120, 40));
}

/// The disambiguation overlay a wiki-link with two candidates produces.
#[test]
fn disambiguation_overlay() {
    let mut app = common::app_with("wiki.md", 120, 40);
    app.index_now();
    frame(&mut app, 120, 40);

    let index = app
        .tab()
        .doc
        .as_ref()
        .unwrap()
        .links
        .iter()
        .position(|link| link.target == "Ambiguous")
        .expect("the fixture has an ambiguous wiki-link");
    app.update(Action::FollowHintedLink(index));

    insta::assert_snapshot!(frame(&mut app, 120, 40));
}

/// The search mode with results, the search prompt, and the quick switcher.
#[test]
fn search_and_switcher() {
    let mut app = common::vault_app(120, 40);
    app.search_now("token");
    // Pinned, so the snapshot does not change with the machine it ran on.
    app.search.elapsed = Some(std::time::Duration::from_millis(7));
    frame(&mut app, 120, 40);
    insta::assert_snapshot!("sidebar_search_populated", frame(&mut app, 120, 40));

    app.update(Action::OpenProjectSearch);
    insta::assert_snapshot!("search_prompt", frame(&mut app, 120, 40));

    app.update(Action::Escape);
    app.update(Action::OpenQuickSwitcher);
    for c in "auth".chars() {
        app.update(Action::SwitcherEdit(
            perga::ui::overlay::prompt::TextEdit::Insert(c),
        ));
    }
    insta::assert_snapshot!("quick_switcher", frame(&mut app, 120, 40));
}

/// Edit mode, and the confirmation that guards leaving it dirty.
#[test]
fn edit_mode_and_confirm() {
    let mut app = common::app_with("gfm.md", 120, 40);
    frame(&mut app, 120, 40);
    app.update(Action::EnterEditMode);
    insta::assert_snapshot!("edit_mode", frame(&mut app, 120, 40));

    app.update(Action::EditInput(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('x'),
        crossterm::event::KeyModifiers::NONE,
    )));
    app.update(Action::Escape);
    insta::assert_snapshot!("confirm_overlay", frame(&mut app, 120, 40));
}

#[test]
fn help_overlay() {
    let mut app = app(120, 40);
    app.update(Action::ToggleHelp);
    insta::assert_snapshot!(frame(&mut app, 120, 40));
}

#[test]
fn terminal_too_small() {
    let mut app = app(30, 8);
    insta::assert_snapshot!(frame(&mut app, 30, 8));
}

/// Each logo tier and the short-viewport fallback, at their boundary widths.
///
/// The tiers exist because a banner wider than the viewport corrupts the frame,
/// so these are the snapshots that would catch it.
#[test]
fn welcome_logo_tiers() {
    for (label, width, height) in [
        ("large", 100u16, 40u16),
        ("medium", 48, 40),
        ("minimal", 30, 40),
        ("short", 100, 10),
    ] {
        let mut app = app(width, height);
        // Hide the sidebar so the viewport, not the split, sets the tier.
        app.update(Action::ToggleSidebar);
        insta::assert_snapshot!(
            format!("welcome_tier_{label}"),
            frame(&mut app, width, height)
        );
    }
}

#[test]
fn no_frame_overflows_its_terminal() {
    // Section 8.1: never panic on tiny terminals, and never draw outside them.
    for width in [30u16, 40, 41, 55, 56, 79, 80, 120, 200] {
        for height in [8u16, 10, 12, 19, 20, 24, 60] {
            let mut app = app(width, height);
            app.update(Action::ToggleHelp);

            for line in frame(&mut app, width, height).lines() {
                assert!(
                    line.chars().count() <= width as usize,
                    "{width}x{height}: a line is {} columns wide",
                    line.chars().count()
                );
            }
        }
    }
}

#[test]
fn resizing_from_large_to_tiny_and_back_is_clean() {
    // The M1 definition of done: resize from 200x60 down to 30x8 without a
    // panic and without a corrupt frame.
    let mut app = app(200, 60);
    let before = frame(&mut app, 200, 60);

    for (width, height) in [(200u16, 60u16), (120, 40), (80, 24), (40, 10), (30, 8)] {
        app.update(Action::Resize(width, height));
        let rendered = frame(&mut app, width, height);
        assert!(!rendered.is_empty());
    }

    app.update(Action::Resize(200, 60));
    assert_eq!(
        frame(&mut app, 200, 60),
        before,
        "the frame did not come back"
    );
}

#[test]
fn logo_tier_boundaries_match_the_rendered_frame() {
    assert_eq!(LogoTier::for_size(MEDIUM_MIN_WIDTH, 40), LogoTier::Medium);
    assert_eq!(
        LogoTier::for_size(MEDIUM_MIN_WIDTH - 1, 40),
        LogoTier::Minimal
    );
    assert_eq!(LogoTier::for_size(100, MIN_LOGO_HEIGHT - 1), LogoTier::None);
}

// -- The fixture corpus ---------------------------------------------------

/// Every document in the corpus, at every snapshot size.
///
/// This is the test that catches a rendering regression anywhere in the
/// pipeline: a wrapping change, a style change, a block that stops rendering.
#[test]
fn fixture_corpus_at_every_size() {
    for name in [
        "README.md",
        "gfm.md",
        "wide.md",
        "unicode.md",
        "broken-links.md",
        "docs/api/auth.md",
    ] {
        for (width, height) in SIZES {
            let mut app = app_with(name, width, height);
            let label = name.replace(['/', '.'], "_");
            insta::assert_snapshot!(
                format!("corpus_{label}_{width}x{height}"),
                frame(&mut app, width, height)
            );
        }
    }
}

#[test]
fn degenerate_documents_render_without_panicking() {
    for name in [
        "empty.md",
        "frontmatter-only.md",
        "no-trailing-newline.md",
        "crlf.md",
        "invalid-utf8.md",
        "spaces and #hash/awkward ışık #1.md",
    ] {
        for (width, height) in SIZES {
            let mut app = app_with(name, width, height);
            let rendered = frame(&mut app, width, height);
            assert!(!rendered.is_empty(), "{name} at {width}x{height}");
        }
    }
}

#[test]
fn a_non_utf8_document_says_so_and_refuses_editing() {
    let app = app_with("invalid-utf8.md", 120, 40);
    let doc = app.tab().doc.as_ref().expect("a document is open");

    assert!(!doc.is_editable());
    assert_eq!(
        app.status.message.as_ref().map(|(t, _)| t.as_str()),
        Some("Read-only: file is not valid UTF-8")
    );
}

#[test]
fn the_frontmatter_title_becomes_the_tab_label() {
    let app = app_with("README.md", 120, 40);
    assert_eq!(app.tab().label(), "Fixture Vault");
}

// -- Scrolling ------------------------------------------------------------

#[test]
fn scrolling_moves_the_window_and_stops_at_the_ends() {
    let mut app = app_with("gfm.md", 100, 30);
    frame(&mut app, 100, 30);
    assert_eq!(app.tab().scroll, 0);

    // Scrolling up at the top is a no-op rather than an error.
    app.update(Action::ScrollLineUp);
    assert_eq!(app.tab().scroll, 0);

    for _ in 0..5 {
        app.update(Action::ScrollLineDown);
    }
    assert_eq!(app.tab().scroll, 5);

    app.update(Action::ScrollTop);
    assert_eq!(app.tab().scroll, 0);

    app.update(Action::ScrollBottom);
    let bottom = app.tab().scroll;
    assert!(bottom > 0, "the document is taller than the viewport");

    // ...and scrolling down at the bottom stays there.
    app.update(Action::ScrollLineDown);
    assert_eq!(app.tab().scroll, bottom);
}

#[test]
fn a_page_scroll_moves_by_the_viewport_height() {
    // A document taller than several pages, so the move is not cut short by
    // the clamp that keeps a screenful in view at the bottom.
    let path = common::large_document(50_000);
    let mut app = app(100, 30);
    app.open(perga::doc::document::Document::load(&path).unwrap());
    frame(&mut app, 100, 30);

    let page = usize::from(app.viewport_inner().height);
    app.update(Action::ScrollPageDown);
    assert_eq!(app.tab().scroll, page);

    app.update(Action::ScrollHalfPageUp);
    assert_eq!(app.tab().scroll, page - page / 2);
}

#[test]
fn heading_motions_land_on_headings() {
    let mut app = app_with("gfm.md", 100, 30);
    frame(&mut app, 100, 30);

    app.update(Action::NextHeading);
    let first = app.tab().scroll;
    assert!(first > 0);

    app.update(Action::NextHeading);
    assert!(app.tab().scroll > first);

    app.update(Action::PrevHeading);
    assert_eq!(app.tab().scroll, first);
}

#[test]
fn a_wide_code_line_is_clipped_and_reachable_by_scrolling_right() {
    let mut app = app_with("wide.md", 80, 30);
    let before = frame(&mut app, 80, 30);
    assert!(before.contains('…'), "the wide line should be clipped");

    for _ in 0..20 {
        app.update(Action::ScrollRight);
    }
    assert_eq!(app.tab().hscroll, 20);

    let after = frame(&mut app, 80, 30);
    assert_ne!(before, after, "scrolling right changed nothing");

    // ...and it does not scroll past the left edge.
    for _ in 0..50 {
        app.update(Action::ScrollLeft);
    }
    assert_eq!(app.tab().hscroll, 0);
}

#[test]
fn scrolling_never_draws_outside_the_viewport() {
    let mut app = app_with("wide.md", 80, 24);

    for _ in 0..30 {
        for line in frame(&mut app, 80, 24).lines() {
            assert!(line.chars().count() <= 80, "{line:?}");
        }
        app.update(Action::ScrollLineDown);
        app.update(Action::ScrollRight);
    }
}

// -- Large documents ------------------------------------------------------

#[test]
fn a_large_document_paints_before_it_is_fully_measured() {
    // Section 9.2: only visible blocks render. The assertion is on ordering,
    // not on wall-clock time, which is unreliable on a shared runner.
    let path = common::large_document(50_000);
    let mut app = app(120, 40);
    app.set_vault_root(common::vault());
    app.open(perga::doc::document::Document::load(&path).unwrap());

    let rendered = frame(&mut app, 120, 40);
    assert!(rendered.contains("A large document"));

    // The total is still unknown, so the title bar shows no position yet.
    assert_eq!(app.scroll_position(), None);
}

#[test]
fn a_large_document_can_be_measured_to_the_end() {
    let path = common::large_document(50_000);
    let mut app = app(120, 40);
    app.open(perga::doc::document::Document::load(&path).unwrap());

    frame(&mut app, 120, 40);
    // Measurement is chunked, so reaching the end takes several passes.
    for _ in 0..500 {
        app.update(Action::ScrollBottom);
        if app.scroll_position().is_some() {
            break;
        }
    }

    let (current, total) = app.scroll_position().expect("the document is measured");
    assert!(total > 50_000, "{total}");
    assert!(current <= total);
}
