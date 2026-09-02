//! Filesystem watching with debouncing, built on `notify`.
//!
//! Debouncing happens on the watcher's own thread, not in the event loop. The
//! main loop is a single blocking `recv` and has no timer; giving it one to
//! coalesce filesystem bursts would undo the 0% idle CPU the whole design is
//! arranged around. See Section 7.1.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};

/// What the watcher reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WatchEvent {
    /// These paths changed, relative to the vault root.
    Changed(Vec<PathBuf>),
    /// These paths are gone.
    Removed(Vec<PathBuf>),
    /// Watching had to be given up. The payload is for the status bar.
    ///
    /// A large vault can exceed the platform's inotify limit, which is a
    /// degradation to manual reload rather than a failure to start.
    Stopped(String),
}

/// A running watch.
///
/// Dropping the handle stops it: the watcher is dropped with it, and the
/// debounce thread sees the channel close and returns.
#[derive(Debug)]
pub struct WatchHandle {
    cancelled: Arc<AtomicBool>,
    /// Kept alive: dropping the watcher unregisters every path it holds.
    _watcher: Option<RecommendedWatcher>,
}

impl WatchHandle {
    /// Stop watching.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl Drop for WatchHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Watch `root` recursively, reporting debounced batches to `sink`.
pub fn spawn(
    root: PathBuf,
    debounce: Duration,
    sink: impl Fn(WatchEvent) + Send + Sync + 'static,
) -> WatchHandle {
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancelled);
    let (tx, rx) = crossbeam_channel::unbounded();

    let watcher = notify::recommended_watcher(move |event| {
        // A send error means the debounce thread has gone, which happens only
        // during teardown.
        let _ = tx.send(event);
    });

    let mut watcher = match watcher {
        Ok(watcher) => watcher,
        Err(e) => {
            sink(WatchEvent::Stopped(stopped_message(&e)));
            return WatchHandle {
                cancelled,
                _watcher: None,
            };
        }
    };

    if let Err(e) = watcher.watch(&root, RecursiveMode::Recursive) {
        sink(WatchEvent::Stopped(stopped_message(&e)));
        return WatchHandle {
            cancelled,
            _watcher: None,
        };
    }

    let spawned = std::thread::Builder::new()
        .name("perga-watch".to_string())
        .spawn(move || debounce_loop(&root, debounce, &rx, &flag, &sink));

    if spawned.is_err() {
        return WatchHandle {
            cancelled,
            _watcher: None,
        };
    }

    WatchHandle {
        cancelled,
        _watcher: Some(watcher),
    }
}

/// Coalesce a burst of events into one batch.
///
/// Editors that save by write-and-rename produce three or four events for one
/// save; reloading the document once per event would make the viewport flicker
/// and would race the dirty flag.
fn debounce_loop(
    root: &Path,
    debounce: Duration,
    rx: &crossbeam_channel::Receiver<notify::Result<notify::Event>>,
    cancelled: &AtomicBool,
    sink: &impl Fn(WatchEvent),
) {
    let mut pending = Batch::default();

    loop {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }

        // Blocking until something happens, then draining whatever else
        // arrives inside the debounce window.
        let first = match rx.recv() {
            Ok(event) => event,
            Err(_) => return,
        };

        pending.add(root, first);
        let deadline = Instant::now() + debounce;

        while let Some(remaining) = deadline.checked_duration_since(Instant::now()) {
            match rx.recv_timeout(remaining) {
                Ok(event) => pending.add(root, event),
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => break,
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return,
            }
        }

        if cancelled.load(Ordering::Relaxed) {
            return;
        }

        for event in pending.drain() {
            sink(event);
        }
    }
}

/// The paths a debounce window collected.
#[derive(Debug, Default)]
struct Batch {
    changed: Vec<PathBuf>,
    removed: Vec<PathBuf>,
    stopped: Option<String>,
}

impl Batch {
    /// Fold one `notify` event into the batch.
    fn add(&mut self, root: &Path, event: notify::Result<notify::Event>) {
        let event = match event {
            Ok(event) => event,
            Err(e) => {
                self.stopped = Some(stopped_message(&e));
                return;
            }
        };

        let removed = matches!(event.kind, notify::EventKind::Remove(_));

        for path in event.paths {
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            if relative.as_os_str().is_empty() {
                continue;
            }

            let list = if removed {
                &mut self.removed
            } else {
                &mut self.changed
            };

            // A burst that touches one path several times is one change.
            if !list.contains(&relative.to_path_buf()) {
                list.push(relative.to_path_buf());
            }
        }
    }

    /// Take the events this batch turned into.
    fn drain(&mut self) -> Vec<WatchEvent> {
        let mut out = Vec::new();

        if !self.changed.is_empty() {
            out.push(WatchEvent::Changed(std::mem::take(&mut self.changed)));
        }
        if !self.removed.is_empty() {
            out.push(WatchEvent::Removed(std::mem::take(&mut self.removed)));
        }
        if let Some(reason) = self.stopped.take() {
            out.push(WatchEvent::Stopped(reason));
        }

        out
    }
}

/// A message explaining why watching stopped.
///
/// The inotify limit is the one a user can actually do something about, so it
/// is named rather than being folded into a generic failure.
fn stopped_message(error: &notify::Error) -> String {
    let text = error.to_string();

    if text.contains("limit") || text.contains("No space left") {
        return "Too many files to watch; reload manually with `r`".to_string();
    }

    format!("Not watching for changes: {text}")
}

/// Writes perga made itself, so the watcher does not report them back.
///
/// Without this, every save races the dirty flag it has just cleared: the
/// write lands, the watcher reports it, and the document reloads on top of the
/// buffer that produced it.
#[derive(Debug, Default)]
pub struct OwnWrites {
    seen: HashMap<PathBuf, SystemTime>,
}

impl OwnWrites {
    /// Record that perga wrote `path`, leaving it at `mtime`.
    pub fn record(&mut self, path: PathBuf, mtime: SystemTime) {
        self.seen.insert(path, mtime);
    }

    /// Whether a reported change is one perga made.
    ///
    /// The record is consumed: a second change to the same path with the same
    /// modification time is somebody else writing the identical bytes, and
    /// reloading then is harmless.
    pub fn claim(&mut self, path: &Path, mtime: Option<SystemTime>) -> bool {
        let Some(recorded) = self.seen.get(path).copied() else {
            return false;
        };

        match mtime {
            Some(mtime) if mtime == recorded => {
                self.seen.remove(path);
                true
            }
            // The file changed again after perga wrote it, so the record is
            // stale and what is on disk is somebody else's.
            Some(_) => {
                self.seen.remove(path);
                false
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("perga-watch-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    }

    #[test]
    fn a_burst_of_writes_arrives_as_one_batch() {
        let root = scratch("burst");
        let seen: Arc<Mutex<Vec<WatchEvent>>> = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&seen);

        let _handle = spawn(root.clone(), Duration::from_millis(80), move |event| {
            sink.lock().unwrap().push(event);
        });

        // The watcher needs a moment to register before it sees anything.
        std::thread::sleep(Duration::from_millis(100));

        for i in 0..5 {
            std::fs::write(root.join("note.md"), format!("version {i}\n")).unwrap();
        }

        std::thread::sleep(Duration::from_millis(400));

        let events = seen.lock().unwrap();
        let changed: Vec<&WatchEvent> = events
            .iter()
            .filter(|e| matches!(e, WatchEvent::Changed(_)))
            .collect();

        assert!(!changed.is_empty(), "the watcher saw nothing");
        assert!(
            changed.len() <= 2,
            "five writes produced {} batches",
            changed.len()
        );

        let WatchEvent::Changed(paths) = changed[0] else {
            unreachable!()
        };
        assert_eq!(paths, &[PathBuf::from("note.md")]);
    }

    #[test]
    fn a_write_perga_made_is_claimed_once() {
        let mut own = OwnWrites::default();
        let path = PathBuf::from("note.md");
        let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(10);

        own.record(path.clone(), mtime);

        assert!(own.claim(&path, Some(mtime)));
        assert!(
            !own.claim(&path, Some(mtime)),
            "the record is consumed by the event it explains"
        );
    }

    #[test]
    fn a_later_change_to_the_same_path_is_not_claimed() {
        let mut own = OwnWrites::default();
        let path = PathBuf::from("note.md");
        let ours = SystemTime::UNIX_EPOCH + Duration::from_secs(10);
        let theirs = SystemTime::UNIX_EPOCH + Duration::from_secs(20);

        own.record(path.clone(), ours);

        assert!(!own.claim(&path, Some(theirs)));
        assert!(!own.claim(&path, Some(ours)), "the stale record is gone");
    }

    #[test]
    fn a_path_perga_never_wrote_is_never_claimed() {
        let mut own = OwnWrites::default();
        assert!(!own.claim(Path::new("other.md"), Some(SystemTime::UNIX_EPOCH)));
    }

    #[test]
    fn an_inotify_limit_is_named_rather_than_shown_raw() {
        let error = notify::Error::new(notify::ErrorKind::MaxFilesWatch);
        assert_eq!(
            stopped_message(&error),
            "Too many files to watch; reload manually with `r`"
        );

        // Anything else says what went wrong rather than guessing.
        let other = notify::Error::generic("the disk fell over");
        assert!(stopped_message(&other).contains("the disk fell over"));
    }
}
