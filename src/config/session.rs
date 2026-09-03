//! Session persistence: one file per vault root.
//!
//! One file per vault, not one globally: two vaults open in two terminals would
//! otherwise overwrite each other's tabs. The file carries a format version and
//! is discarded on a mismatch: a session is a convenience, and reconstructing
//! one badly is worse than opening on the welcome screen.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ui::sidebar::SidebarMode;

/// The session file's format version.
pub const VERSION: u32 = 1;

/// One open tab, as the session remembers it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedTab {
    /// The document, relative to the vault root.
    pub path: PathBuf,
    /// Where the reader was in it.
    #[serde(default)]
    pub scroll: usize,
}

/// Everything perga remembers about a vault between runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Session {
    /// The format this was written in.
    pub version: u32,
    /// The tabs that were open.
    pub tabs: Vec<SavedTab>,
    /// Which of them was active.
    pub active_tab: usize,
    /// Whether the sidebar was showing.
    pub sidebar_visible: bool,
    /// How wide it was.
    pub sidebar_width: u16,
    /// Which mode it was in.
    pub sidebar_mode: SidebarMode,
    /// The theme, when it was overridden at runtime.
    pub theme: Option<String>,
    /// Documents opened, most recent first.
    pub recent: Vec<PathBuf>,
}

impl Default for Session {
    fn default() -> Self {
        Session {
            version: VERSION,
            tabs: Vec::new(),
            active_tab: 0,
            sidebar_visible: true,
            sidebar_width: 32,
            sidebar_mode: SidebarMode::Files,
            theme: None,
            recent: Vec::new(),
        }
    }
}

impl Session {
    /// Read the session for a vault, or `None` when there is nothing usable.
    ///
    /// Every failure is `None`: a missing file, an unreadable one, one that is
    /// not valid TOML, and one written by a different version. A corrupt
    /// session costs the reader their tabs, never their run.
    pub fn load(vault_root: &Path) -> Option<Self> {
        let path = path_for(vault_root)?;
        let text = std::fs::read_to_string(&path).ok()?;

        let session: Session = match toml::from_str(&text) {
            Ok(session) => session,
            Err(e) => {
                tracing::warn!("ignoring the session at {}: {e}", path.display());
                return None;
            }
        };

        if session.version != VERSION {
            tracing::info!(
                "ignoring the session at {}: written by format {}",
                path.display(),
                session.version
            );
            return None;
        }

        Some(session)
    }

    /// Write the session for a vault.
    pub fn save(&self, vault_root: &Path) -> std::io::Result<()> {
        let Some(path) = path_for(vault_root) else {
            return Ok(());
        };

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let text = toml::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        std::fs::write(path, text)
    }
}

/// Where a vault's session file lives.
///
/// `$XDG_STATE_HOME/perga/sessions/<vault-hash>.toml`, falling back to the data
/// directory on platforms with no state directory.
pub fn path_for(vault_root: &Path) -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "perga")?;
    let canonical = vault_root
        .canonicalize()
        .unwrap_or_else(|_| vault_root.to_path_buf());

    Some(
        dirs.state_dir()
            .unwrap_or_else(|| dirs.data_dir())
            .join("sessions")
            .join(format!("{:016x}.toml", hash_of(&canonical))),
    )
}

/// Hash a vault path for its session file's name.
fn hash_of(path: &Path) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("perga-session-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    fn session() -> Session {
        Session {
            tabs: vec![
                SavedTab {
                    path: PathBuf::from("README.md"),
                    scroll: 0,
                },
                SavedTab {
                    path: PathBuf::from("docs/api/auth.md"),
                    scroll: 42,
                },
            ],
            active_tab: 1,
            sidebar_visible: false,
            sidebar_width: 40,
            sidebar_mode: SidebarMode::Outline,
            theme: Some("light".to_string()),
            recent: vec![PathBuf::from("docs/api/auth.md")],
            ..Session::default()
        }
    }

    #[test]
    fn a_session_round_trips() {
        let vault = scratch("roundtrip");

        session().save(&vault).expect("the state dir is writable");
        assert_eq!(Session::load(&vault), Some(session()));
    }

    #[test]
    fn two_vaults_do_not_share_a_session() {
        let first = scratch("first");
        let second = scratch("second");

        assert_ne!(path_for(&first), path_for(&second));

        session().save(&first).unwrap();
        assert!(Session::load(&second).is_none() || Session::load(&second) != Some(session()));
    }

    #[test]
    fn a_session_from_another_format_is_ignored() {
        let vault = scratch("version");
        let mut session = session();
        session.version = VERSION + 1;
        session.save(&vault).unwrap();

        assert_eq!(Session::load(&vault), None);
    }

    #[test]
    fn a_corrupt_session_is_ignored_rather_than_fatal() {
        let vault = scratch("corrupt");
        let path = path_for(&vault).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "this is not [ toml").unwrap();

        assert_eq!(Session::load(&vault), None);
    }

    #[test]
    fn a_vault_with_no_session_loads_nothing() {
        assert_eq!(Session::load(&scratch("empty")), None);
    }

    #[test]
    fn a_partial_session_file_keeps_the_other_defaults() {
        let vault = scratch("partial");
        let path = path_for(&vault).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "version = 1\nsidebar_width = 20\n").unwrap();

        let loaded = Session::load(&vault).expect("a partial file still loads");
        assert_eq!(loaded.sidebar_width, 20);
        assert!(loaded.sidebar_visible, "the rest came from the defaults");
        assert!(loaded.tabs.is_empty());
    }
}
