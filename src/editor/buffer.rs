//! The edit buffer: dirty tracking, atomic saving, and recovery files.
//!
//! The text itself lives in a `tui-textarea` `TextArea`, which already provides
//! undo, selection, and word motions; nothing here reimplements them. What is
//! here is everything about the *file*: what was on disk when it was loaded,
//! how to put it back without ever truncating it, and what to do with unsaved
//! text when the process is told to exit.

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use tui_textarea::TextArea;

use crate::doc::document::{Document, LineEnding, BOM};

/// One tab's edit buffer.
#[derive(Debug)]
pub struct EditorState {
    /// The text being edited.
    pub textarea: TextArea<'static>,
    /// Hash of the text as it last was on disk.
    saved_hash: u64,
    /// The file's modification time when it was loaded or last saved.
    pub known_mtime: SystemTime,
    /// The line ending to write back.
    line_ending: LineEnding,
    /// Whether the file began with a byte-order mark.
    had_bom: bool,
}

impl EditorState {
    /// Open a document for editing.
    pub fn new(document: &Document) -> Self {
        // Split on `\n` after normalising: the textarea works in lines, and
        // the ending the file actually used is put back on save.
        let lines: Vec<String> = document
            .source
            .replace("\r\n", "\n")
            .split('\n')
            .map(str::to_string)
            .collect();

        let textarea = TextArea::new(lines);

        EditorState {
            saved_hash: hash_of(&textarea),
            textarea,
            known_mtime: document.mtime,
            line_ending: document.line_ending,
            had_bom: document.had_bom,
        }
    }

    /// Whether the buffer differs from what is on disk.
    ///
    /// Compared by content rather than by an edited flag, so undoing back to
    /// the saved text makes the tab clean again.
    pub fn is_dirty(&self) -> bool {
        hash_of(&self.textarea) != self.saved_hash
    }

    /// The buffer as it would be written, with the file's own line ending and
    /// any byte-order mark restored.
    pub fn contents(&self) -> String {
        let mut out = String::new();
        if self.had_bom {
            out.push(BOM);
        }
        out.push_str(&self.textarea.lines().join(self.line_ending.as_str()));
        out
    }

    /// The buffer as plain text, for the recovery file.
    pub fn text(&self) -> String {
        self.textarea.lines().join("\n")
    }

    /// Record that the buffer is now what is on disk.
    pub fn mark_saved(&mut self, mtime: SystemTime) {
        self.saved_hash = hash_of(&self.textarea);
        self.known_mtime = mtime;
    }

    /// The cursor's `(line, column)`, both zero-based.
    pub fn cursor(&self) -> (usize, usize) {
        self.textarea.cursor()
    }
}

/// Hash a textarea's contents, for the dirty comparison.
fn hash_of(textarea: &TextArea<'static>) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    for line in textarea.lines() {
        line.hash(&mut hasher);
    }
    hasher.finish()
}

/// Why a save did not happen.
#[derive(Debug)]
pub enum SaveError {
    /// The file changed on disk since it was loaded.
    ///
    /// Never resolved silently: the reader is asked whether to overwrite.
    Conflict {
        /// The modification time now on disk.
        found: SystemTime,
    },
    /// The write failed.
    Io(io::Error),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Conflict { .. } => f.write_str("the file changed on disk"),
            SaveError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// Make a save fail after the temporary file is written but before it is
/// renamed into place.
///
/// This is the crash the atomic write exists to survive, and there is no other
/// way to produce it from a test.
#[cfg(test)]
pub static FAIL_BEFORE_RENAME: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Write `contents` to `path`, atomically.
///
/// The text goes to a temporary file in the same directory and is renamed into
/// place, so an interruption leaves either the old file or the new one and
/// never a truncated one. `rename` is only atomic within a filesystem, which is
/// why the temporary file is a sibling rather than something in `/tmp`.
///
/// `expected` is the modification time the caller believes the file has. When
/// it does not match, nothing is written and the caller is told.
pub fn save(
    path: &Path,
    contents: &str,
    expected: Option<SystemTime>,
) -> Result<SystemTime, SaveError> {
    let existing = std::fs::metadata(path).ok();

    if let (Some(expected), Some(metadata)) = (expected, &existing) {
        if let Ok(found) = metadata.modified() {
            if differs(found, expected) {
                return Err(SaveError::Conflict { found });
            }
        }
    }

    let directory = path.parent().unwrap_or(Path::new("."));
    let temp = temp_path(path);

    std::fs::write(&temp, contents).map_err(SaveError::Io)?;

    // The temporary file was created by this process with default
    // permissions; the file being replaced keeps its own.
    #[cfg(unix)]
    if let Some(metadata) = &existing {
        use std::os::unix::fs::PermissionsExt;
        let mode = metadata.permissions().mode();
        let _ = std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(mode));
    }

    #[cfg(test)]
    if FAIL_BEFORE_RENAME.load(std::sync::atomic::Ordering::Relaxed) {
        let _ = std::fs::remove_file(&temp);
        return Err(SaveError::Io(io::Error::other("injected failure")));
    }

    std::fs::rename(&temp, path).map_err(|e| {
        // A failed rename leaves the temporary file behind, which would show
        // up in the tree as a stray note.
        let _ = std::fs::remove_file(&temp);
        SaveError::Io(e)
    })?;

    let _ = directory;

    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map_err(SaveError::Io)
}

/// Whether two modification times differ.
///
/// Compared with a one-second tolerance: some filesystems store whole seconds,
/// and a mismatch caused by the filesystem rather than by another writer would
/// make every save ask about a conflict that is not there.
fn differs(found: SystemTime, expected: SystemTime) -> bool {
    let delta = found
        .duration_since(expected)
        .or_else(|_| expected.duration_since(found))
        .unwrap_or_default();

    delta.as_secs() >= 1
}

/// The temporary file a save writes through.
fn temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "unnamed".to_string());

    path.with_file_name(format!(".{name}.perga-{}.tmp", std::process::id()))
}

/// Where unsaved text for a document is parked when perga is signalled.
///
/// `$XDG_STATE_HOME/perga/recovery/<vault-hash>/<path-hash>.md`. Hashes rather
/// than paths, for the same reason as the index cache: a path can hold anything
/// a filesystem allows.
pub fn recovery_path(vault_root: &Path, document: &Path) -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "perga")?;

    Some(
        dirs.state_dir()
            .unwrap_or_else(|| dirs.data_dir())
            .join("recovery")
            .join(format!("{:016x}", hash_path(vault_root)))
            .join(format!("{:016x}.md", hash_path(document))),
    )
}

/// Write a dirty buffer's text where it can be offered back.
pub fn write_recovery(vault_root: &Path, document: &Path, text: &str) -> io::Result<()> {
    let Some(path) = recovery_path(vault_root, document) else {
        return Ok(());
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, text)
}

/// Read back a recovery file, if there is one.
pub fn read_recovery(vault_root: &Path, document: &Path) -> Option<String> {
    std::fs::read_to_string(recovery_path(vault_root, document)?).ok()
}

/// Forget a recovery file, once it has been restored or refused.
pub fn clear_recovery(vault_root: &Path, document: &Path) {
    if let Some(path) = recovery_path(vault_root, document) {
        let _ = std::fs::remove_file(path);
    }
}

/// Hash a path for a state directory name.
fn hash_path(path: &Path) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::atomic::Ordering;

    /// A scratch directory for the tests that touch the filesystem.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("perga-buffer-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    fn document(source: &str) -> Document {
        Document::from_source(
            PathBuf::from("note.md"),
            source.to_string(),
            SystemTime::UNIX_EPOCH,
            false,
            None,
        )
    }

    #[test]
    fn a_fresh_buffer_is_clean() {
        let editor = EditorState::new(&document("# Note\n\nProse.\n"));
        assert!(!editor.is_dirty());
    }

    #[test]
    fn typing_makes_it_dirty_and_undoing_makes_it_clean_again() {
        let mut editor = EditorState::new(&document("# Note\n"));

        editor.textarea.insert_char('x');
        assert!(editor.is_dirty());

        editor.textarea.undo();
        assert!(
            !editor.is_dirty(),
            "undoing back to the saved text is clean"
        );
    }

    #[test]
    fn crlf_endings_survive_a_round_trip() {
        let source = "# Note\r\n\r\nProse.\r\n";
        let document = Document::from_source(
            PathBuf::from("note.md"),
            source.to_string(),
            SystemTime::UNIX_EPOCH,
            false,
            None,
        );

        let editor = EditorState::new(&document);
        assert_eq!(editor.contents(), source);
    }

    #[test]
    fn a_byte_order_mark_is_restored() {
        let document = Document::from_source(
            PathBuf::from("note.md"),
            "# Note\n".to_string(),
            SystemTime::UNIX_EPOCH,
            true,
            None,
        );

        assert!(EditorState::new(&document).contents().starts_with(BOM));
    }

    #[test]
    fn saving_writes_the_file_and_reports_its_new_mtime() {
        let dir = scratch("write");
        let path = dir.join("note.md");
        std::fs::write(&path, "old\n").unwrap();
        let before = std::fs::metadata(&path).unwrap().modified().unwrap();

        let mtime = save(&path, "new\n", Some(before)).expect("the save succeeds");

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "new\n");
        assert!(mtime >= before);
    }

    #[test]
    fn saving_a_file_that_does_not_exist_yet_creates_it() {
        let dir = scratch("create");
        let path = dir.join("new.md");

        save(&path, "# New\n", None).expect("the save succeeds");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# New\n");
    }

    /// Acceptance: a crash during save never leaves a truncated file.
    #[test]
    fn a_failure_before_the_rename_leaves_the_original_intact() {
        let dir = scratch("fault");
        let path = dir.join("note.md");
        std::fs::write(&path, "the original\n").unwrap();

        FAIL_BEFORE_RENAME.store(true, Ordering::Relaxed);
        let outcome = save(&path, "the replacement\n", None);
        FAIL_BEFORE_RENAME.store(false, Ordering::Relaxed);

        assert!(outcome.is_err());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "the original\n",
            "the file was truncated by a failed save"
        );

        // ...and no temporary file is left behind to show up in the tree.
        let strays: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains("perga-"))
            .collect();
        assert!(strays.is_empty(), "{strays:?}");
    }

    /// Acceptance: file permissions are preserved across a save.
    #[cfg(unix)]
    #[test]
    fn permissions_survive_a_save() {
        use std::os::unix::fs::PermissionsExt;

        let dir = scratch("modes");
        let path = dir.join("note.md");
        std::fs::write(&path, "old\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();

        save(&path, "new\n", None).expect("the save succeeds");

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o640, "{mode:o}");
    }

    #[test]
    fn a_file_changed_on_disk_is_reported_rather_than_overwritten() {
        let dir = scratch("conflict");
        let path = dir.join("note.md");
        std::fs::write(&path, "theirs\n").unwrap();

        // A modification time an hour in the past: whatever is on disk now is
        // newer than what the caller believes it loaded.
        let stale = SystemTime::now() - std::time::Duration::from_secs(3_600);

        let outcome = save(&path, "mine\n", Some(stale));

        assert!(matches!(outcome, Err(SaveError::Conflict { .. })));
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "theirs\n",
            "a conflict must never overwrite"
        );
    }

    #[test]
    fn a_matching_mtime_is_not_a_conflict() {
        let dir = scratch("nomconflict");
        let path = dir.join("note.md");
        std::fs::write(&path, "old\n").unwrap();
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();

        assert!(save(&path, "new\n", Some(mtime)).is_ok());
    }

    #[test]
    fn recovery_text_round_trips() {
        let vault = Path::new("/some/vault");
        let document = Path::new("/some/vault/note.md");

        write_recovery(vault, document, "unsaved text").expect("the state dir is writable");
        assert_eq!(
            read_recovery(vault, document).as_deref(),
            Some("unsaved text")
        );

        clear_recovery(vault, document);
        assert_eq!(read_recovery(vault, document), None);
    }
}
