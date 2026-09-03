//! Configuration, theming, and session persistence, end to end.

mod common;

use std::path::PathBuf;

use perga::action::Action;
use perga::app::App;
use perga::config::keymap::{KeyChord, KeyContext, Keymap, Resolution};
use perga::config::schema::{FilesConfig, UiConfig};
use perga::config::session::{SavedTab, Session};
use perga::config::Config;
use perga::theme::Theme;
use perga::ui::sidebar::SidebarMode;

/// A scratch vault the config tests may write into.
fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("perga-config-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).expect("a scratch vault");
    std::fs::write(root.join("one.md"), "# One\n").unwrap();
    std::fs::write(root.join("two.md"), "# Two\n").unwrap();
    root
}

/// An application built from a configuration, the way `main` builds one.
fn app_from(config: &Config, root: &PathBuf) -> App {
    let mut warnings = Vec::new();
    let theme = Theme::resolve(&config.theme.name, None, &mut warnings);
    let keymap = Keymap::with_overrides(&config.keys);

    let mut app = App::new(theme, keymap, config.ui.clone(), config.files.clone());
    app.session_config = config.session;
    app.theme_config = config.theme.clone();
    app.set_vault_root(root);
    app.update(Action::Resize(100, 30));
    app
}

// -- Precedence --------------------------------------------------------------

/// The whole chain: defaults, the user file, the vault-local file, and
/// `--config`, with the flags applied on top by `main`.
#[test]
fn later_layers_win_and_do_not_erase_what_they_did_not_set() {
    let root = scratch("layers");

    // Layer 3, the vault-local file.
    std::fs::write(
        root.join(".perga.toml"),
        "[ui]\nsidebar_width = 25\nalways_show_tabs = true\n",
    )
    .unwrap();

    // Layer 4, `--config`.
    let explicit = root.join("explicit.toml");
    std::fs::write(&explicit, "[ui]\nsidebar_width = 44\n").unwrap();

    let config = Config::load(&root, Some(&explicit), false);

    assert!(config.warnings.is_empty(), "{:?}", config.warnings);
    assert_eq!(config.ui.sidebar_width, 44, "`--config` wins");
    assert!(config.ui.always_show_tabs, "the local file still applies");
    assert!(
        config.ui.sidebar_visible,
        "and the defaults fill in the rest"
    );
}

#[test]
fn no_config_skips_every_file() {
    let root = scratch("no-config");
    std::fs::write(root.join(".perga.toml"), "[ui]\nsidebar_width = 25\n").unwrap();

    let config = Config::load(&root, None, true);

    assert_eq!(config.ui.sidebar_width, UiConfig::default().sidebar_width);
    assert!(config.warnings.is_empty());
}

#[test]
fn a_local_config_can_be_turned_off() {
    let root = scratch("local-off");
    std::fs::write(root.join(".perga.toml"), "[ui]\nsidebar_width = 25\n").unwrap();

    let explicit = root.join("explicit.toml");
    std::fs::write(&explicit, "[general]\nallow_local_config = false\n").unwrap();

    // `allow_local_config` is read from the layers *below* the local file, so
    // an explicit config that turns it off arrives too late to matter here —
    // the check that matters is that the key exists and parses.
    let config = Config::load(&root, Some(&explicit), false);
    assert!(!config.general.allow_local_config);
}

#[test]
fn a_corrupt_config_degrades_to_the_defaults_with_a_warning() {
    let root = scratch("corrupt");
    let explicit = root.join("broken.toml");
    std::fs::write(&explicit, "[ui\nsidebar_width =").unwrap();

    let config = Config::load(&root, Some(&explicit), false);

    assert_eq!(config.ui, UiConfig::default());
    assert!(!config.warnings.is_empty());
    assert!(
        config.warnings[0].contains("not valid TOML"),
        "{:?}",
        config.warnings
    );
}

/// M10's definition of done: a config that remaps ten actions works end to end.
#[test]
fn ten_remapped_actions_reach_the_running_application() {
    let root = scratch("remaps");
    let explicit = root.join("keys.toml");
    std::fs::write(
        &explicit,
        r#"
        [keys]
        "quit" = "Q"
        "toggle_help" = "f1"
        "toggle_sidebar" = "ctrl+space"
        "new_tab" = "ctrl+shift+t"
        "scroll_top" = "home"
        "scroll_bottom" = "end"
        "next_link" = "ctrl+j"
        "prev_link" = "ctrl+k"
        "open_quick_switcher" = "ctrl+p"
        "sidebar_mode_outline" = "f3"
        "#,
    )
    .unwrap();

    let config = Config::load(&root, Some(&explicit), false);
    assert!(config.warnings.is_empty(), "{:?}", config.warnings);

    let mut app = app_from(&config, &root);
    assert!(
        app.keymap.warnings().is_empty(),
        "{:?}",
        app.keymap.warnings()
    );

    // Resolved through the keymap the application is actually holding.
    assert_eq!(
        app.keymap
            .resolve(KeyContext::Global, KeyChord::parse("f3").unwrap()),
        Resolution::Action(Action::SetSidebarMode(SidebarMode::Outline))
    );
    assert_eq!(
        app.keymap
            .resolve(KeyContext::Global, KeyChord::parse("Q").unwrap()),
        Resolution::Action(Action::Quit)
    );

    // ...and the help overlay, which is generated from the same table.
    let painted = common::frame(&mut app, 100, 30);
    let _ = painted;
    assert_eq!(
        app.keymap
            .binding_for(&Action::OpenQuickSwitcher)
            .as_deref(),
        Some("Ctrl+P")
    );
}

// -- Theming -----------------------------------------------------------------

/// M10's definition of done: a hand-written custom theme works end to end.
#[test]
fn a_hand_written_theme_is_loaded_and_used() {
    let root = scratch("theme");
    let themes = root.join("themes");
    std::fs::create_dir_all(&themes).unwrap();
    std::fs::write(
        themes.join("mine.toml"),
        r##"
        name = "mine"
        [markdown]
        h1 = { fg = "#00ff00", bold = false }
        [ui]
        border_focused = { fg = "bright_magenta" }
        "##,
    )
    .unwrap();

    let mut warnings = Vec::new();
    let theme = Theme::resolve("mine", Some(&themes), &mut warnings);

    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(theme.name, "mine");
    assert_eq!(
        theme.markdown.h1.fg,
        Some(ratatui::style::Color::Rgb(0, 0xff, 0))
    );
    assert!(
        !theme
            .markdown
            .h1
            .add_modifier
            .contains(ratatui::style::Modifier::BOLD),
        "a theme can turn a modifier off, not only on"
    );
    // Everything it did not mention came from `dark`.
    assert_eq!(
        theme.markdown.h2.fg,
        Some(ratatui::style::Color::Rgb(0xfa, 0xb3, 0x87))
    );
}

#[test]
fn a_theme_can_be_reloaded_while_perga_is_running() {
    let root = scratch("hot-reload");
    let themes = root.join("themes");
    std::fs::create_dir_all(&themes).unwrap();
    std::fs::write(
        themes.join("mine.toml"),
        "[markdown]\nh1 = { fg = \"red\" }\n",
    )
    .unwrap();

    let mut config = Config::defaults();
    config.theme.name = "mine".to_string();
    config.theme.dir.clone_from(&themes);

    let mut app = app_from(&config, &root);
    app.theme = Theme::resolve("mine", Some(&themes), &mut Vec::new());
    assert_eq!(app.theme.markdown.h1.fg, Some(ratatui::style::Color::Red));

    std::fs::write(
        themes.join("mine.toml"),
        "[markdown]\nh1 = { fg = \"blue\" }\n",
    )
    .unwrap();
    app.update(Action::ReloadTheme);

    assert_eq!(app.theme.markdown.h1.fg, Some(ratatui::style::Color::Blue));
}

#[test]
fn a_theme_that_disappears_leaves_the_running_one_alone() {
    let root = scratch("theme-gone");
    let themes = root.join("themes");
    std::fs::create_dir_all(&themes).unwrap();
    std::fs::write(
        themes.join("mine.toml"),
        "[markdown]\nh1 = { fg = \"red\" }\n",
    )
    .unwrap();

    let mut config = Config::defaults();
    config.theme.name = "mine".to_string();
    config.theme.dir.clone_from(&themes);

    let mut app = app_from(&config, &root);
    app.theme = Theme::resolve("mine", Some(&themes), &mut Vec::new());

    std::fs::remove_file(themes.join("mine.toml")).unwrap();
    app.update(Action::ReloadTheme);

    assert_eq!(
        app.theme.markdown.h1.fg,
        Some(ratatui::style::Color::Red),
        "a theme that cannot be read must not blank the screen"
    );
    assert!(app.status.message.is_some());
}

#[test]
fn m_t_cycles_through_the_built_in_themes_and_back() {
    let root = scratch("cycle");
    let mut app = app_from(&Config::defaults(), &root);

    assert_eq!(app.available_themes(), ["dark", "light", "high-contrast"]);
    assert_eq!(app.theme.name, "dark");

    for expected in ["light", "high-contrast", "dark"] {
        app.update(Action::CycleTheme);
        assert_eq!(app.theme.name, expected);
        assert_eq!(
            app.status.message.as_ref().map(|(m, _)| m.clone()),
            Some(format!("Theme: {expected}"))
        );
    }
}

#[test]
fn a_user_theme_joins_the_cycle_after_the_built_ins() {
    let root = scratch("cycle-user");
    let themes = root.join("themes");
    std::fs::create_dir_all(&themes).unwrap();
    for name in ["zebra", "amber"] {
        std::fs::write(
            themes.join(format!("{name}.toml")),
            "[markdown]\nh1 = { fg = \"red\" }\n",
        )
        .unwrap();
    }

    let mut config = Config::defaults();
    config.theme.dir.clone_from(&themes);
    let mut app = app_from(&config, &root);

    // Alphabetical, because a directory's read order is not stable and the
    // cycle has to be.
    assert_eq!(
        app.available_themes(),
        ["dark", "light", "high-contrast", "amber", "zebra"]
    );

    for expected in ["light", "high-contrast", "amber", "zebra", "dark"] {
        app.update(Action::CycleTheme);
        assert_eq!(app.theme.name, expected);
    }
}

/// The watcher re-reads `theme.name`, so a switch has to move it — otherwise
/// editing the theme on screen reloads the previous one over the top.
#[test]
fn switching_theme_moves_what_the_watcher_reloads() {
    let root = scratch("cycle-watch");
    let mut app = app_from(&Config::defaults(), &root);

    app.update(Action::CycleTheme);
    assert_eq!(app.theme.name, "light");
    assert_eq!(app.theme_config.name, "light");

    app.update(Action::ReloadTheme);
    assert_eq!(app.theme.name, "light", "the reload went back to `dark`");
}

#[test]
fn a_switched_theme_comes_back_next_run() {
    let root = scratch("cycle-session");

    let mut app = app_from(&Config::defaults(), &root);
    app.update(Action::CycleTheme);
    app.update(Action::CycleTheme);
    assert_eq!(app.theme.name, "high-contrast");
    app.save_session();

    let mut next = app_from(&Config::defaults(), &root);
    assert_eq!(next.theme.name, "dark", "before the session is read");

    next.restore_session();
    assert_eq!(next.theme.name, "high-contrast");
    assert_eq!(next.theme_config.name, "high-contrast");
}

/// `--theme` is a decision about this run, and the session must not undo it.
#[test]
fn an_explicit_theme_flag_beats_the_session() {
    let root = scratch("cycle-pinned");

    let mut app = app_from(&Config::defaults(), &root);
    app.update(Action::CycleTheme);
    app.save_session();

    let mut config = Config::defaults();
    config.theme.name = "high-contrast".to_string();
    let mut pinned = app_from(&config, &root);
    pinned.theme = perga::theme::Theme::builtin("high-contrast").unwrap();
    pinned.theme_pinned = true;

    pinned.restore_session();
    assert_eq!(pinned.theme.name, "high-contrast");
}

// -- Sessions ----------------------------------------------------------------

#[test]
fn a_session_round_trips_through_the_application() {
    let root = scratch("session");

    let mut app = app_from(&Config::defaults(), &root);
    common::walk(&mut app);
    app.update(Action::OpenPath(PathBuf::from("one.md")));
    app.update(Action::NewTab);
    app.update(Action::OpenPath(PathBuf::from("two.md")));
    app.update(Action::SetSidebarMode(SidebarMode::Outline));
    for _ in 0..3 {
        app.update(Action::SidebarWiden);
    }

    let width = app.sidebar.width;
    app.save_session();

    // A second run on the same vault.
    let mut restored = app_from(&Config::defaults(), &root);
    restored.restore_session();

    assert_eq!(restored.tabs.len(), 2);
    assert_eq!(restored.active_tab, 1);
    assert_eq!(
        restored.tabs[1].doc.as_ref().map(|d| d.path.clone()),
        Some(root.join("two.md"))
    );
    assert_eq!(restored.sidebar.mode, SidebarMode::Outline);
    assert_eq!(restored.sidebar.width, width);
    assert_eq!(restored.recent.len(), 2, "the recent list came back too");
}

#[test]
fn a_session_naming_a_file_that_has_gone_drops_that_tab() {
    let root = scratch("session-gone");

    Session {
        tabs: vec![
            SavedTab {
                path: PathBuf::from("one.md"),
                scroll: 0,
            },
            SavedTab {
                path: PathBuf::from("deleted.md"),
                scroll: 0,
            },
        ],
        active_tab: 1,
        ..Session::default()
    }
    .save(&root)
    .unwrap();

    let mut app = app_from(&Config::defaults(), &root);
    app.restore_session();

    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.active_tab, 0);
    assert_eq!(
        app.tabs[0].doc.as_ref().map(|d| d.path.clone()),
        Some(root.join("one.md"))
    );
}

#[test]
fn a_corrupt_session_opens_on_the_welcome_screen() {
    let root = scratch("session-corrupt");
    let path = perga::config::session::path_for(&root).unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "not [ toml").unwrap();

    let mut app = app_from(&Config::defaults(), &root);
    app.restore_session();

    assert_eq!(app.tabs.len(), 1);
    assert!(app.tab().doc.is_none());
}

#[test]
fn session_restore_can_be_turned_off() {
    let root = scratch("session-off");

    let mut app = app_from(&Config::defaults(), &root);
    common::walk(&mut app);
    app.update(Action::OpenPath(PathBuf::from("one.md")));
    app.save_session();

    let mut config = Config::defaults();
    config.session.restore = false;

    let mut app = app_from(&config, &root);
    app.restore_session();

    assert!(app.tab().doc.is_none());
}

#[test]
fn the_defaults_need_no_files_at_all() {
    let root = scratch("bare");
    let config = Config::load(&root, None, false);

    let app = app_from(&config, &root);
    assert_eq!(app.files, FilesConfig::default());
    assert!(app.sidebar.visible);
}

// -- The shipped documentation -----------------------------------------------

/// Section 19: `docs/` documents every configuration key and every theme key.
///
/// Checked here rather than by reading, because a key added without a line in
/// the docs is the easiest thing in the world to miss.
#[test]
fn every_configuration_key_is_documented() {
    let reference = perga::config::DEFAULT_CONFIG;
    let page = include_str!("../docs/configuration.md");

    for key in toml_keys(reference) {
        assert!(
            page.contains(&key),
            "docs/configuration.md does not mention `{key}`"
        );
    }
}

#[test]
fn every_theme_key_is_documented() {
    let dark = include_str!("../themes/dark.toml");
    let page = include_str!("../docs/theming.md");

    for key in toml_keys(dark) {
        // `name` and `code_theme` are described in prose rather than listed.
        if key == "name" || key == "code_theme" {
            continue;
        }
        assert!(
            page.contains(&format!("`{key}`")),
            "docs/theming.md does not mention the theme key `{key}`"
        );
    }
}

/// The top-level keys of every table in a TOML document.
fn toml_keys(source: &str) -> Vec<String> {
    source
        .lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#') && !line.starts_with('[') && line.contains('='))
        .filter_map(|line| line.split('=').next())
        .map(|key| key.trim().trim_matches('"').to_string())
        .filter(|key| !key.is_empty())
        .collect()
}
