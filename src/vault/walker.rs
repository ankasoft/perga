//! Background directory traversal built on `ignore`.
//!
//! The walk runs on its own thread and streams what it finds in batches, so
//! the first frame is painted from an empty tree and the tree fills in behind
//! it. Nothing here touches application state: the walker is handed a sink,
//! and the sink turns each event into an [`crate::action::Action`] like every
//! other background worker.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::SystemTime;

/// Directories that are never walked, whatever the configuration says.
///
/// A `.git` directory holds tens of thousands of object files and nothing a
/// reader wants to see. This list is deliberately not configurable.
pub const ALWAYS_EXCLUDED: [&str; 4] = [".git", ".hg", ".svn", ".jj"];

/// How many entries are collected before they are sent on.
///
/// One message per file would put 10,000 wake-ups through the event loop for a
/// vault that size; one message for the whole walk would leave the tree empty
/// until it finished. A batch is the compromise, and it is small enough that
/// the tree visibly fills in rather than appearing all at once.
const BATCH: usize = 256;

/// One entry found by the walk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The path relative to the vault root.
    pub path: PathBuf,
    /// Whether the entry is a directory.
    pub is_dir: bool,
    /// Last modification time, when the filesystem gave one.
    pub mtime: Option<SystemTime>,
    /// Size in bytes. Zero for directories.
    pub size: u64,
}

/// What the walker reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WalkEvent {
    /// A batch of entries, in whatever order the walk produced them.
    Entries(Vec<Entry>),
    /// The walk finished, having found this many entries in total.
    Finished(usize),
    /// The walk could not be completed. The payload is for the status bar.
    Failed(String),
}

/// How the vault is walked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkOptions {
    /// Honour `.gitignore`, `.ignore`, and the global gitignore.
    pub respect_gitignore: bool,
    /// Follow symlinks.
    pub follow_symlinks: bool,
}

impl Default for WalkOptions {
    fn default() -> Self {
        WalkOptions {
            respect_gitignore: true,
            follow_symlinks: false,
        }
    }
}

/// A running walk.
///
/// Dropping the handle cancels the walk: the thread checks the flag between
/// entries and stops at the next one, so switching vaults or quitting does not
/// leave a thread reading a filesystem nobody is waiting on.
#[derive(Debug)]
pub struct WalkHandle {
    cancelled: Arc<AtomicBool>,
}

impl WalkHandle {
    /// Stop the walk at the next entry.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl Drop for WalkHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Walk `root` on a background thread, reporting to `sink`.
///
/// The walk is single-threaded. `ignore` can walk in parallel, but the tree
/// sorts what it receives anyway, so the only thing parallelism buys is wall
/// clock on a vault large enough for the streaming to hide it regardless — at
/// the cost of a second channel and a harder cancellation story.
pub fn spawn(
    root: impl AsRef<Path>,
    options: WalkOptions,
    sink: impl Fn(WalkEvent) + Send + Sync + 'static,
) -> WalkHandle {
    let root = root.as_ref().to_path_buf();
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancelled);

    // Shared rather than moved, so that a thread that could not be spawned can
    // still report itself through the same sink.
    let sink = Arc::new(sink);
    let thread_sink = Arc::clone(&sink);

    let spawned = thread::Builder::new()
        .name("perga-walker".to_string())
        .spawn(move || walk(&root, options, &flag, thread_sink.as_ref()));

    if let Err(e) = spawned {
        // Not fatal: perga is still a usable reader without a tree, so this is
        // a status-bar message rather than an exit.
        sink(WalkEvent::Failed(format!("cannot walk the vault: {e}")));
    }

    WalkHandle { cancelled }
}

/// The body of the walk, factored out so the tests can run it synchronously.
pub fn walk(root: &Path, options: WalkOptions, cancelled: &AtomicBool, sink: &impl Fn(WalkEvent)) {
    let walker = ignore::WalkBuilder::new(root)
        // Dotted entries are the vault owner's notes, not noise. The tree
        // decides whether to *show* them; the walk always finds them.
        .hidden(false)
        // A notes vault is often not a git repository, and `.gitignore` still
        // means what it says there.
        .require_git(false)
        .git_ignore(options.respect_gitignore)
        .git_exclude(options.respect_gitignore)
        .git_global(options.respect_gitignore)
        .ignore(options.respect_gitignore)
        .parents(options.respect_gitignore)
        .follow_links(options.follow_symlinks)
        .filter_entry(|entry| !is_excluded(entry.file_name()))
        .build();

    let mut batch = Vec::with_capacity(BATCH);
    let mut total = 0usize;

    for result in walker {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }

        let entry = match result {
            Ok(entry) => entry,
            // A single unreadable directory is not a reason to abandon the
            // walk; the rest of the vault is still worth showing.
            Err(e) => {
                tracing::debug!("skipping an entry while walking: {e}");
                continue;
            }
        };

        // The root itself is the tree's implicit parent, not a row in it.
        let Ok(relative) = entry.path().strip_prefix(root) else {
            continue;
        };
        if relative.as_os_str().is_empty() {
            continue;
        }

        let metadata = entry.metadata().ok();
        let is_dir = entry.file_type().is_some_and(|t| t.is_dir());

        batch.push(Entry {
            path: relative.to_path_buf(),
            is_dir,
            mtime: metadata.as_ref().and_then(|m| m.modified().ok()),
            size: if is_dir {
                0
            } else {
                metadata.as_ref().map_or(0, |m| m.len())
            },
        });
        total += 1;

        if batch.len() >= BATCH {
            sink(WalkEvent::Entries(std::mem::take(&mut batch)));
            batch.reserve(BATCH);
        }
    }

    if cancelled.load(Ordering::Relaxed) {
        return;
    }

    if !batch.is_empty() {
        sink(WalkEvent::Entries(batch));
    }

    sink(WalkEvent::Finished(total));
}

/// Whether a directory name is one of the VCS directories that are never
/// walked.
fn is_excluded(name: &std::ffi::OsStr) -> bool {
    name.to_str()
        .is_some_and(|name| ALWAYS_EXCLUDED.contains(&name))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    /// The committed fixture vault.
    fn fixture_vault() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault")
    }

    /// Run a walk to completion and return everything it reported.
    fn collect(root: &Path, options: WalkOptions) -> (Vec<Entry>, Option<usize>) {
        let entries = Mutex::new(Vec::new());
        let finished = Mutex::new(None);

        walk(
            root,
            options,
            &AtomicBool::new(false),
            &|event| match event {
                WalkEvent::Entries(batch) => entries.lock().unwrap().extend(batch),
                WalkEvent::Finished(total) => *finished.lock().unwrap() = Some(total),
                WalkEvent::Failed(e) => panic!("the walk failed: {e}"),
            },
        );

        (
            entries.into_inner().unwrap(),
            finished.into_inner().unwrap(),
        )
    }

    #[test]
    fn the_walk_finds_the_fixture_vault() {
        let (entries, finished) = collect(&fixture_vault(), WalkOptions::default());

        assert_eq!(finished, Some(entries.len()));
        assert!(entries.iter().any(|e| e.path == Path::new("README.md")));
        assert!(entries
            .iter()
            .any(|e| e.path == Path::new("docs/api/auth.md")));
        assert!(entries
            .iter()
            .any(|e| e.path == Path::new("docs") && e.is_dir));
    }

    #[test]
    fn dotted_directories_are_walked() {
        let (entries, _) = collect(&fixture_vault(), WalkOptions::default());

        assert!(
            entries
                .iter()
                .any(|e| e.path == Path::new(".github/CONTRIBUTING.md")),
            "a dotted directory must be found; hiding it is the tree's choice"
        );
    }

    #[test]
    fn a_symlink_loop_does_not_hang_the_walk() {
        // `tests/fixtures/vault/loop` is a symlink to its own directory.
        let (entries, finished) = collect(&fixture_vault(), WalkOptions::default());
        assert!(finished.is_some());
        assert!(entries.len() < 1_000, "the walk followed the loop");
    }

    #[test]
    fn gitignored_paths_are_skipped_unless_asked_for() {
        // The fixture vault ignores `notes/`, which holds one Markdown file.
        let (respected, _) = collect(&fixture_vault(), WalkOptions::default());
        assert!(!respected
            .iter()
            .any(|e| e.path == Path::new("notes/ignored.md")));

        let (ignored, _) = collect(
            &fixture_vault(),
            WalkOptions {
                respect_gitignore: false,
                ..WalkOptions::default()
            },
        );
        assert!(ignored
            .iter()
            .any(|e| e.path == Path::new("notes/ignored.md")));
    }

    #[test]
    fn vcs_directories_are_never_walked() {
        let root = tempdir();
        std::fs::create_dir(root.join(".git")).unwrap();
        std::fs::write(root.join(".git/config"), "[core]").unwrap();
        std::fs::write(root.join("note.md"), "# note").unwrap();

        let (entries, _) = collect(&root, WalkOptions::default());

        assert!(entries.iter().any(|e| e.path == Path::new("note.md")));
        assert!(
            !entries.iter().any(|e| e.path.starts_with(".git")),
            "`.git` is excluded regardless of include_hidden"
        );
    }

    #[test]
    fn cancelling_stops_the_walk() {
        let root = tempdir();
        for i in 0..2_000 {
            std::fs::write(root.join(format!("note-{i}.md")), "x").unwrap();
        }

        let cancelled = AtomicBool::new(false);
        let seen = Mutex::new(0usize);
        let finished = Mutex::new(false);

        walk(&root, WalkOptions::default(), &cancelled, &|event| {
            match event {
                WalkEvent::Entries(batch) => {
                    *seen.lock().unwrap() += batch.len();
                    // Cancel as soon as anything arrives.
                    cancelled.store(true, Ordering::Relaxed);
                }
                WalkEvent::Finished(_) => *finished.lock().unwrap() = true,
                WalkEvent::Failed(e) => panic!("the walk failed: {e}"),
            }
        });

        assert!(
            !*finished.lock().unwrap(),
            "a cancelled walk does not finish"
        );
        assert!(*seen.lock().unwrap() < 2_000);
    }

    /// A scratch directory, removed by the OS rather than by the test: the
    /// tests here only ever add files to it.
    fn tempdir() -> PathBuf {
        static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("perga-walker-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }
}
