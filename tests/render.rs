//! Snapshot tests of rendered frames.
//!
//! Rendering is a pure function of state, so every one of these drives the app
//! through `Action`s and then renders to `TestBackend`. No terminal is
//! involved.

mod common;

use common::{app, frame, SIZES};
use perga::action::Action;
use perga::ui::sidebar::SidebarMode;
use perga::ui::welcome::{LogoTier, MEDIUM_MIN_WIDTH, MIN_LOGO_HEIGHT};

#[test]
fn welcome_screen_at_every_size() {
    for (width, height) in SIZES {
        let app = app(width, height);
        insta::assert_snapshot!(
            format!("welcome_{width}x{height}"),
            frame(&app, width, height)
        );
    }
}

#[test]
fn sidebar_hidden() {
    let mut app = app(120, 40);
    app.update(Action::ToggleSidebar);
    insta::assert_snapshot!(frame(&app, 120, 40));
}

#[test]
fn sidebar_focused() {
    let mut app = app(120, 40);
    app.update(Action::FocusNext);
    insta::assert_snapshot!(frame(&app, 120, 40));
}

#[test]
fn each_sidebar_mode() {
    for mode in SidebarMode::ALL {
        let mut app = app(120, 40);
        app.update(Action::SetSidebarMode(mode));
        insta::assert_snapshot!(format!("sidebar_mode_{mode}"), frame(&app, 120, 40));
    }
}

#[test]
fn help_overlay() {
    let mut app = app(120, 40);
    app.update(Action::ToggleHelp);
    insta::assert_snapshot!(frame(&app, 120, 40));
}

#[test]
fn terminal_too_small() {
    let app = app(30, 8);
    insta::assert_snapshot!(frame(&app, 30, 8));
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
        insta::assert_snapshot!(format!("welcome_tier_{label}"), frame(&app, width, height));
    }
}

#[test]
fn no_frame_overflows_its_terminal() {
    // Section 8.1: never panic on tiny terminals, and never draw outside them.
    for width in [30u16, 40, 41, 55, 56, 79, 80, 120, 200] {
        for height in [8u16, 10, 12, 19, 20, 24, 60] {
            let mut app = app(width, height);
            app.update(Action::ToggleHelp);

            for line in frame(&app, width, height).lines() {
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
    let before = frame(&app, 200, 60);

    for (width, height) in [(200u16, 60u16), (120, 40), (80, 24), (40, 10), (30, 8)] {
        app.update(Action::Resize(width, height));
        let rendered = frame(&app, width, height);
        assert!(!rendered.is_empty());
    }

    app.update(Action::Resize(200, 60));
    assert_eq!(frame(&app, 200, 60), before, "the frame did not come back");
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
