//! Configuration loading and the five-layer precedence chain.
//!
//! The layers, lowest first: built-in defaults, the user's `config.toml`, the
//! vault-local `.perga.toml`, a `--config` file, and individual CLI flags.
//! Each is parsed into a `toml::Table` and merged key by key, so a file that
//! sets one key inherits the rest rather than replacing a whole table.
//!
//! # Nothing here is ever a hard error
//!
//! An unknown key produces a warning and is dropped; an invalid *value*
//! produces a warning naming the key and falls back to the default for that key
//! alone. A configuration written for a newer perga still opens the vault, and
//! a typo costs one setting rather than the whole file.
//!
//! # A vault-local config is untrusted input
//!
//! `.perga.toml` arrives with any cloned repository. It may set presentation and
//! navigation keys and nothing else — never `editor.external_command`, never a
//! key remap. See [`LOCAL_ALLOW_LIST`] and Section 10.

pub mod keymap;
pub mod schema;
pub mod session;

use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use toml::{Table, Value};

use crate::config::schema::{
    EditorConfig, FilesConfig, GeneralConfig, SearchConfig, SessionConfig, ThemeConfig, UiConfig,
    WatchConfig, WikiLinkConfig,
};

/// The tables and keys a vault-local `.perga.toml` may set.
///
/// Presentation and navigation only. A `general.*` entry names one key; a bare
/// table name allows the whole table.
pub const LOCAL_ALLOW_LIST: &[&str] = &[
    "ui",
    "theme",
    "files",
    "wikilinks",
    "search",
    "session",
    "general.wrap",
    "general.tab_width",
];

/// Everything perga was configured with.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Config {
    pub general: GeneralConfig,
    pub ui: UiConfig,
    pub files: FilesConfig,
    pub theme: ThemeConfig,
    pub wikilinks: WikiLinkConfig,
    pub search: SearchConfig,
    pub editor: EditorConfig,
    pub watch: WatchConfig,
    pub session: SessionConfig,
    /// Raw `[keys]` remaps, resolved against the binding table by
    /// [`keymap::Keymap::with_overrides`].
    pub keys: Table,
    /// Everything that could not be understood, for the status bar and
    /// `--check-config`.
    pub warnings: Vec<String>,
}

impl Config {
    /// Built-in defaults and nothing else.
    pub fn defaults() -> Self {
        Config::default()
    }

    /// Assemble the configuration from every layer.
    ///
    /// `explicit` is `--config`; `vault_root` is where a local `.perga.toml`
    /// would be. Neither is read when `no_config` is set.
    pub fn load(vault_root: &Path, explicit: Option<&Path>, no_config: bool) -> Self {
        let mut warnings = Vec::new();
        let mut merged = Table::new();

        if !no_config {
            if let Some(path) = user_config_path() {
                if let Some(table) = read_table(&path, &mut warnings) {
                    merge(&mut merged, table);
                }
            }

            // Read before the local file so `allow_local_config` from the
            // user's own configuration decides whether it is read at all.
            let general: GeneralConfig = table_of(&merged, "general", &mut warnings);

            if general.allow_local_config {
                let local = vault_root.join(".perga.toml");
                if let Some(table) = read_table(&local, &mut warnings) {
                    let filtered = filter_local(table, &local, &mut warnings);
                    merge(&mut merged, filtered);
                }
            }

            if let Some(path) = explicit {
                match read_table(path, &mut warnings) {
                    Some(table) => merge(&mut merged, table),
                    // An explicit `--config` that cannot be read is the one
                    // case worth being loud about: the user named it.
                    None => warnings.push(format!("cannot read `{}`", path.display())),
                }
            }
        }

        Config::from_table(merged, warnings)
    }

    /// Build a configuration from an already-merged table.
    pub fn from_table(table: Table, mut warnings: Vec<String>) -> Self {
        let known = [
            "general",
            "ui",
            "files",
            "theme",
            "wikilinks",
            "search",
            "editor",
            "watch",
            "session",
            "keys",
        ];

        for key in table.keys() {
            if !known.contains(&key.as_str()) {
                warnings.push(format!("unknown section `[{key}]`"));
            }
        }

        Config {
            general: table_of(&table, "general", &mut warnings),
            ui: table_of(&table, "ui", &mut warnings),
            files: table_of(&table, "files", &mut warnings),
            theme: table_of(&table, "theme", &mut warnings),
            wikilinks: table_of(&table, "wikilinks", &mut warnings),
            search: table_of(&table, "search", &mut warnings),
            editor: table_of(&table, "editor", &mut warnings),
            watch: table_of(&table, "watch", &mut warnings),
            session: table_of(&table, "session", &mut warnings),
            keys: table
                .get("keys")
                .and_then(Value::as_table)
                .cloned()
                .unwrap_or_default(),
            warnings,
        }
    }
}

/// Where the user's own configuration lives.
pub fn user_config_path() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "perga")?;
    Some(dirs.config_dir().join("config.toml"))
}

/// The default directory user themes are read from.
pub fn user_theme_dir() -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "perga")?;
    Some(dirs.config_dir().join("themes"))
}

/// Read one TOML file, warning rather than failing on a syntax error.
fn read_table(path: &Path, warnings: &mut Vec<String>) -> Option<Table> {
    let text = std::fs::read_to_string(path).ok()?;

    match toml::from_str::<Table>(&text) {
        Ok(table) => Some(table),
        Err(e) => {
            warnings.push(format!("`{}` is not valid TOML: {e}", path.display()));
            None
        }
    }
}

/// Layer `overlay` onto `base`, one key at a time.
///
/// Tables merge; anything else replaces. This is what makes a file that sets
/// `ui.sidebar_width` inherit the rest of `[ui]` rather than replacing it.
fn merge(base: &mut Table, overlay: Table) {
    for (key, value) in overlay {
        match (base.get_mut(&key), value) {
            (Some(Value::Table(existing)), Value::Table(new)) => merge(existing, new),
            (_, value) => {
                base.insert(key, value);
            }
        }
    }
}

/// Drop everything a vault-local config is not allowed to set.
fn filter_local(table: Table, path: &Path, warnings: &mut Vec<String>) -> Table {
    let mut out = Table::new();

    for (section, value) in table {
        if LOCAL_ALLOW_LIST.contains(&section.as_str()) {
            out.insert(section, value);
            continue;
        }

        // A partially allowed table keeps only the keys named in the list.
        let Some(inner) = value.as_table() else {
            warnings.push(format!(
                "`{}` may not set `{section}`; ignored",
                path.display()
            ));
            continue;
        };

        let mut kept = Table::new();
        for (key, value) in inner {
            let qualified = format!("{section}.{key}");
            if LOCAL_ALLOW_LIST.contains(&qualified.as_str()) {
                kept.insert(key.clone(), value.clone());
            } else {
                warnings.push(format!(
                    "`{}` may not set `{qualified}`; ignored",
                    path.display()
                ));
            }
        }

        if !kept.is_empty() {
            out.insert(section, Value::Table(kept));
        }
    }

    out
}

/// Deserialize one table, isolating whatever cannot be understood.
///
/// A table that deserializes cleanly is returned as it is. One that does not is
/// rebuilt key by key: each key is tried on its own, the ones that work are
/// kept, and each one that does not produces a warning naming it. That is what
/// makes an invalid value cost one key rather than the whole table.
fn table_of<T>(root: &Table, name: &str, warnings: &mut Vec<String>) -> T
where
    T: Default + DeserializeOwned,
{
    let Some(table) = root.get(name).and_then(Value::as_table) else {
        return T::default();
    };

    if let Ok(parsed) = T::deserialize(Value::Table(table.clone())) {
        return parsed;
    }

    let mut accepted = Table::new();

    for (key, value) in table {
        let mut candidate = accepted.clone();
        candidate.insert(key.clone(), value.clone());

        match T::deserialize(Value::Table(candidate.clone())) {
            Ok(_) => accepted = candidate,
            Err(e) => warnings.push(format!("`{name}.{key}` ignored: {e}")),
        }
    }

    T::deserialize(Value::Table(accepted)).unwrap_or_default()
}

/// The reference configuration, shipped verbatim as `--generate-config`.
pub const DEFAULT_CONFIG: &str = include_str!("../../docs/default-config.toml");

#[cfg(test)]
mod tests {
    use super::*;

    use crate::ui::sidebar::SidebarMode;

    fn config(text: &str) -> Config {
        Config::from_table(toml::from_str(text).unwrap(), Vec::new())
    }

    #[test]
    fn an_empty_configuration_is_the_defaults() {
        let config = config("");
        assert_eq!(config, Config::defaults());
        assert!(config.warnings.is_empty());
    }

    #[test]
    fn a_partial_table_keeps_the_rest_of_its_defaults() {
        let config = config("[ui]\nsidebar_width = 44\n");

        assert_eq!(config.ui.sidebar_width, 44);
        assert!(config.ui.sidebar_visible);
        assert_eq!(config.ui.narrow_threshold, 80);
    }

    #[test]
    fn later_layers_win_key_by_key() {
        let mut merged = Table::new();
        merge(
            &mut merged,
            toml::from_str("[ui]\nsidebar_width = 20\nmouse = false\n").unwrap(),
        );
        merge(
            &mut merged,
            toml::from_str("[ui]\nsidebar_width = 44\n").unwrap(),
        );

        let config = Config::from_table(merged, Vec::new());
        assert_eq!(config.ui.sidebar_width, 44, "the later layer wins");
        assert!(!config.ui.mouse, "and does not erase what it did not set");
    }

    #[test]
    fn an_unknown_key_is_a_warning_and_the_rest_of_the_table_survives() {
        let config = config("[ui]\nsidebar_width = 44\nfrom_the_future = true\n");

        assert_eq!(config.ui.sidebar_width, 44);
        assert_eq!(config.warnings.len(), 1);
        assert!(
            config.warnings[0].contains("ui.from_the_future"),
            "{:?}",
            config.warnings
        );
    }

    #[test]
    fn an_unknown_section_is_a_warning_and_nothing_else() {
        let config = config("[from_the_future]\nkey = 1\n");

        assert_eq!(
            config,
            Config {
                warnings: config.warnings.clone(),
                ..Config::defaults()
            }
        );
        assert!(config.warnings[0].contains("[from_the_future]"));
    }

    #[test]
    fn an_invalid_value_costs_one_key() {
        let config = config("[ui]\nsidebar_width = \"wide\"\nnarrow_threshold = 100\n");

        assert_eq!(
            config.ui.sidebar_width,
            UiConfig::default().sidebar_width,
            "the bad key fell back to its default"
        );
        assert_eq!(config.ui.narrow_threshold, 100, "and the good one survived");
        assert_eq!(config.warnings.len(), 1);
        assert!(config.warnings[0].contains("ui.sidebar_width"));
    }

    #[test]
    fn every_table_is_reachable_from_a_file() {
        let config = config(
            r#"
            [general]
            wrap = 72
            [ui]
            sidebar_default_mode = "outline"
            [files]
            sort = "mtime"
            [theme]
            name = "light"
            [wikilinks]
            resolution = "filename-first"
            [search]
            max_results = 10
            [editor]
            tab_size = 2
            [watch]
            debounce_ms = 500
            [session]
            restore = false
            [keys]
            quit = "ctrl+q"
            "#,
        );

        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        assert_eq!(config.general.wrap, 72);
        assert_eq!(config.ui.sidebar_default_mode, SidebarMode::Outline);
        assert_eq!(config.files.sort, crate::config::schema::SortKey::Mtime);
        assert_eq!(config.theme.name, "light");
        assert_eq!(config.search.max_results, 10);
        assert_eq!(config.editor.tab_size, 2);
        assert_eq!(config.watch.debounce_ms, 500);
        assert!(!config.session.restore);
        assert_eq!(config.keys.len(), 1);
    }

    // -- The vault-local file ------------------------------------------------

    fn local(text: &str) -> (Table, Vec<String>) {
        let mut warnings = Vec::new();
        let table = filter_local(
            toml::from_str(text).unwrap(),
            Path::new("/vault/.perga.toml"),
            &mut warnings,
        );
        (table, warnings)
    }

    #[test]
    fn a_local_config_may_set_presentation_keys() {
        let (table, warnings) = local("[ui]\nsidebar_width = 20\n[files]\nshow_all = true\n");

        assert!(warnings.is_empty());
        assert!(table.contains_key("ui"));
        assert!(table.contains_key("files"));
    }

    /// The one that matters: a cloned repository must not be able to make
    /// perga run a program.
    #[test]
    fn a_local_config_may_never_name_a_program_to_run() {
        let (table, warnings) = local("[editor]\nexternal_command = \"rm -rf /\"\n");

        assert!(table.is_empty(), "{table:?}");
        assert_eq!(warnings.len(), 1);
        assert!(
            warnings[0].contains("editor.external_command"),
            "{warnings:?}"
        );
    }

    #[test]
    fn a_local_config_may_not_remap_keys() {
        let (table, warnings) = local("[keys]\nquit = \"x\"\n");

        assert!(table.is_empty());
        assert!(warnings[0].contains("keys"), "{warnings:?}");
    }

    #[test]
    fn a_local_config_may_set_only_the_two_allowed_general_keys() {
        let (table, warnings) = local(
            "[general]\nwrap = 72\ntab_width = 2\nfollow_symlinks = true\nstart_path = \"/\"\n",
        );

        let general = table["general"].as_table().unwrap();
        assert_eq!(general.len(), 2);
        assert!(general.contains_key("wrap"));
        assert!(general.contains_key("tab_width"));
        assert_eq!(warnings.len(), 2);
    }

    #[test]
    fn a_local_config_that_sets_nothing_allowed_contributes_nothing() {
        let (table, _) = local("[watch]\nenabled = false\n");
        assert!(table.is_empty());
    }

    // -- The shipped reference -----------------------------------------------

    /// Section 10.1 says the reference block, the `--generate-config` output,
    /// and the actual defaults are the same thing. This is what keeps them so.
    #[test]
    fn the_shipped_default_config_parses_to_the_defaults() {
        let config = Config::from_table(toml::from_str(DEFAULT_CONFIG).unwrap(), Vec::new());

        assert!(
            config.warnings.is_empty(),
            "the shipped configuration must be valid: {:?}",
            config.warnings
        );
        assert_eq!(
            Config {
                warnings: Vec::new(),
                keys: Table::new(),
                ..config
            },
            Config::defaults(),
            "the shipped configuration must match the built-in defaults"
        );
    }
}
