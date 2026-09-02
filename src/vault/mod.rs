//! The vault: the root directory, its file tree, and the backlink index.

pub mod index;
pub mod tree;
pub mod walker;
pub mod watch;

use std::path::{Path, PathBuf};

use crate::config::schema::FilesConfig;
use crate::vault::index::{Index, IndexEvent, IndexHandle};
use crate::vault::tree::Tree;
use crate::vault::walker::{WalkHandle, WalkOptions};

/// The directory perga was opened on, and everything derived from it.
#[derive(Debug)]
pub struct Vault {
    /// The root every path in the tree is relative to.
    pub root: PathBuf,
    /// The file tree.
    pub tree: Tree,
    /// The wiki-link and backlink index.
    pub index: Index,
    /// Markdown files the walk found, with what the cache is checked against.
    ///
    /// Collected as the walk streams so that when it finishes the index knows
    /// exactly which files changed without a second pass over the filesystem.
    pub markdown: Vec<(PathBuf, Option<std::time::SystemTime>, u64)>,
    /// The running walk. Dropping it cancels the walk.
    walk: Option<WalkHandle>,
    /// The running index build. Dropping it cancels the build.
    build: Option<IndexHandle>,
}

impl Vault {
    /// A vault rooted at `root` with an empty tree.
    pub fn new(root: impl Into<PathBuf>, config: &FilesConfig) -> Self {
        Vault {
            root: root.into(),
            tree: Tree::new(config),
            index: Index::new(),
            markdown: Vec::new(),
            walk: None,
            build: None,
        }
    }

    /// Start walking the vault, cancelling any walk already running.
    pub fn start_walk(
        &mut self,
        options: WalkOptions,
        sink: impl Fn(walker::WalkEvent) + Send + Sync + 'static,
    ) {
        // Replacing the handle drops the old one, which cancels it. Doing this
        // first means a vault switched twice in quick succession never has two
        // walks feeding one tree.
        self.walk = None;
        self.walk = Some(walker::spawn(self.root.clone(), options, sink));
    }

    /// Stop any running walk.
    pub fn cancel_walk(&mut self) {
        self.walk = None;
    }

    /// Start building the index for the files the cache does not cover.
    pub fn start_index(
        &mut self,
        stale: Vec<PathBuf>,
        sink: impl Fn(IndexEvent) + Send + Sync + 'static,
    ) {
        self.build = None;
        self.build = Some(index::spawn(self.root.clone(), stale, sink));
    }

    /// Stop any running index build.
    pub fn cancel_index(&mut self) {
        self.build = None;
    }

    /// Resolve a tree path against the vault root.
    pub fn absolute(&self, relative: &Path) -> PathBuf {
        self.root.join(relative)
    }

    /// Express an absolute path relative to the vault root.
    ///
    /// `None` when the path is outside the vault, which is not an error: a
    /// document opened by an absolute path simply has no row in the tree.
    pub fn relative<'a>(&self, path: &'a Path) -> Option<&'a Path> {
        path.strip_prefix(&self.root).ok()
    }
}

/// Hand a path to the desktop's own opener.
///
/// Used for the files in the tree that perga does not render and, in Section
/// 9.5, for external links. The child is deliberately not waited on: `xdg-open`
/// returns as soon as it has handed the path over, and blocking the event loop
/// on someone else's application would freeze the UI.
pub fn open_external(path: &Path) -> std::io::Result<()> {
    use std::process::{Command, Stdio};

    #[cfg(target_os = "macos")]
    const OPENER: &str = "open";
    #[cfg(not(target_os = "macos"))]
    const OPENER: &str = "xdg-open";

    Command::new(OPENER)
        .arg(path)
        // The opener must not write over the alternate screen.
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_round_trip_through_the_root() {
        let vault = Vault::new("/vault", &FilesConfig::default());
        let absolute = vault.absolute(Path::new("docs/api/auth.md"));

        assert_eq!(absolute, PathBuf::from("/vault/docs/api/auth.md"));
        assert_eq!(
            vault.relative(&absolute),
            Some(Path::new("docs/api/auth.md"))
        );
    }

    #[test]
    fn a_path_outside_the_vault_has_no_relative_form() {
        let vault = Vault::new("/vault", &FilesConfig::default());
        assert_eq!(vault.relative(Path::new("/elsewhere/note.md")), None);
    }
}
