//! Editing: the buffer, and the file operations around it.
//!
//! Creating and renaming are here rather than in `vault` because they are what
//! the editor is for. Both go through [`resolve_new_path`], which is the one
//! place that decides whether a path the reader typed is allowed, and it
//! refuses everything that would write outside the vault.

pub mod buffer;

use std::path::{Component, Path, PathBuf};

use crate::config::schema::FilesConfig;
use crate::doc::links;

/// Why a path the reader typed cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PathError {
    /// Nothing was typed.
    #[error("no path given")]
    Empty,
    /// The path climbs out of the vault.
    #[error("`{0}` is outside the vault")]
    OutsideVault(String),
    /// Something is already there.
    #[error("`{0}` already exists")]
    Exists(String),
    /// A rename was given a path rather than a name.
    #[error("a new name cannot contain a path separator")]
    NameHasSeparator,
}

/// Turn a path the reader typed into an absolute one, or refuse it.
///
/// `relative_to` is what a bare name is taken to be relative to: the active
/// document's directory, or the selected tree directory. A path is refused when
/// it escapes the vault after normalisation, which is the check that makes
/// `../../etc/passwd` a message rather than a write.
pub fn resolve_new_path(
    typed: &str,
    relative_to: &Path,
    vault_root: &Path,
    files: &FilesConfig,
) -> Result<PathBuf, PathError> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Err(PathError::Empty);
    }

    let with_extension = with_markdown_extension(typed, files);
    let candidate = Path::new(&with_extension);

    // A path the reader wrote from the vault root is taken as written; one
    // they wrote bare is relative to where they are.
    let joined = if candidate.is_absolute() || typed.starts_with('/') {
        vault_root.join(candidate.strip_prefix("/").unwrap_or(candidate))
    } else if has_directory(candidate) {
        vault_root.join(candidate)
    } else {
        relative_to.join(candidate)
    };

    let normalised = links::normalise(&joined);

    if !normalised.starts_with(vault_root) {
        return Err(PathError::OutsideVault(typed.to_string()));
    }

    if normalised.exists() {
        return Err(PathError::Exists(typed.to_string()));
    }

    Ok(normalised)
}

/// Turn a new name for an existing file into a path, or refuse it.
///
/// A rename takes a *name*, not a path: moving a note between directories is a
/// different operation, and accepting a separator here would make `r` silently
/// do it.
pub fn resolve_rename(
    typed: &str,
    existing: &Path,
    vault_root: &Path,
) -> Result<PathBuf, PathError> {
    let typed = typed.trim();
    if typed.is_empty() {
        return Err(PathError::Empty);
    }

    if typed.contains('/') || typed.contains('\\') {
        return Err(PathError::NameHasSeparator);
    }

    let directory = existing.parent().unwrap_or(vault_root);
    let candidate = links::normalise(&directory.join(typed));

    if !candidate.starts_with(vault_root) {
        return Err(PathError::OutsideVault(typed.to_string()));
    }

    if candidate.exists() && candidate != existing {
        return Err(PathError::Exists(typed.to_string()));
    }

    Ok(candidate)
}

/// Create a file and the directories above it.
///
/// The content is whatever the caller decided, which is either nothing or the
/// frontmatter stub from `editor.new_file_frontmatter`.
pub fn create(path: &Path, contents: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // `create_new` rather than `write`: between the existence check in
    // `resolve_new_path` and here, something else may have created the file,
    // and overwriting a note that appeared in that window would lose it.
    use std::io::Write as _;
    let mut file = std::fs::File::create_new(path)?;
    file.write_all(contents.as_bytes())
}

/// The frontmatter a new file is seeded with, when that is turned on.
pub fn frontmatter_stub(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("untitled");

    format!("---\ntitle: {stem}\n---\n\n")
}

/// Give a typed path a Markdown extension when it has none.
fn with_markdown_extension(typed: &str, files: &FilesConfig) -> String {
    if files.is_markdown(Path::new(typed)) {
        return typed.to_string();
    }

    let extension = files.extensions.first().map_or("md", String::as_str);
    format!("{typed}.{extension}")
}

/// Whether a path names a directory as well as a file.
fn has_directory(path: &Path) -> bool {
    path.components()
        .any(|c| matches!(c, Component::Normal(_)) && c != path.components().next_back().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch vault of its own for each test.
    ///
    /// Per call rather than per process: these run in parallel, and one test
    /// clearing the directory another is halfway through using is a race.
    fn setup() -> PathBuf {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        let root = std::env::temp_dir().join(format!("perga-editor-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("docs")).expect("a scratch vault");
        std::fs::write(root.join("docs/existing.md"), "# Existing\n").unwrap();
        root
    }

    fn resolve_in(root: &Path, typed: &str, from: &str) -> Result<PathBuf, PathError> {
        resolve_new_path(typed, &root.join(from), root, &FilesConfig::default())
    }

    /// Acceptance: `notes/ideas` becomes `notes/ideas.md`.
    #[test]
    fn a_path_without_an_extension_gets_one() {
        let root = setup();
        assert_eq!(
            resolve_in(&root, "notes/ideas", "").unwrap(),
            root.join("notes/ideas.md")
        );
    }

    #[test]
    fn an_extension_already_given_is_kept() {
        let root = setup();
        assert_eq!(
            resolve_in(&root, "notes/x.md", "").unwrap(),
            root.join("notes/x.md")
        );
        assert_eq!(
            resolve_in(&root, "notes/x.markdown", "").unwrap(),
            root.join("notes/x.markdown")
        );
    }

    #[test]
    fn a_bare_name_lands_beside_the_document_it_was_typed_from() {
        let root = setup();
        assert_eq!(
            resolve_in(&root, "idea", "docs").unwrap(),
            root.join("docs/idea.md"),
            "a bare name is relative to where the reader is"
        );
    }

    #[test]
    fn a_path_with_a_directory_is_read_from_the_vault_root() {
        let root = setup();
        assert_eq!(
            resolve_in(&root, "notes/idea", "docs").unwrap(),
            root.join("notes/idea.md")
        );
    }

    /// Acceptance: renaming to a path outside the vault is refused.
    #[test]
    fn a_path_that_climbs_out_of_the_vault_is_refused() {
        let root = setup();

        assert_eq!(
            resolve_in(&root, "../../etc/passwd", "docs"),
            Err(PathError::OutsideVault("../../etc/passwd".to_string()))
        );

        // A leading slash is read from the vault root, so this one lands
        // inside it and is allowed; it is not an escape.
        assert_eq!(
            resolve_in(&root, "/etc/passwd", "docs").unwrap(),
            root.join("etc/passwd.md")
        );
    }

    #[test]
    fn an_existing_path_is_refused_rather_than_overwritten() {
        let root = setup();
        assert_eq!(
            resolve_in(&root, "docs/existing.md", ""),
            Err(PathError::Exists("docs/existing.md".to_string()))
        );
    }

    #[test]
    fn an_empty_path_is_refused() {
        let root = setup();
        assert_eq!(resolve_in(&root, "   ", ""), Err(PathError::Empty));
    }

    #[test]
    fn a_rename_takes_a_name_and_not_a_path() {
        let root = setup();
        let existing = root.join("docs/existing.md");

        assert_eq!(
            resolve_rename("renamed.md", &existing, &root).unwrap(),
            root.join("docs/renamed.md")
        );
        assert_eq!(
            resolve_rename("../renamed.md", &existing, &root),
            Err(PathError::NameHasSeparator)
        );
        assert_eq!(
            resolve_rename("sub/renamed.md", &existing, &root),
            Err(PathError::NameHasSeparator)
        );
    }

    #[test]
    fn a_rename_onto_an_existing_file_is_refused() {
        let root = setup();
        std::fs::write(root.join("docs/other.md"), "").unwrap();

        assert_eq!(
            resolve_rename("other.md", &root.join("docs/existing.md"), &root),
            Err(PathError::Exists("other.md".to_string()))
        );
    }

    #[test]
    fn renaming_a_file_to_its_own_name_is_allowed() {
        let root = setup();
        let existing = root.join("docs/existing.md");
        assert!(resolve_rename("existing.md", &existing, &root).is_ok());
    }

    #[test]
    fn creating_makes_the_directories_above_it() {
        let root = setup();
        let path = root.join("deep/nested/note.md");

        create(&path, "# Note\n").expect("the file is created");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# Note\n");
    }

    #[test]
    fn creating_over_an_existing_file_fails_rather_than_replacing_it() {
        let root = setup();
        let path = root.join("docs/existing.md");

        assert!(create(&path, "replacement").is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# Existing\n");
    }

    #[test]
    fn the_frontmatter_stub_titles_the_file_after_its_stem() {
        assert_eq!(
            frontmatter_stub(Path::new("/vault/notes/an idea.md")),
            "---\ntitle: an idea\n---\n\n"
        );
    }
}
