//! Helpers shared by the integration tests.
//!
//! Each integration test binary compiles this module separately and uses a
//! different part of it, so the unused ones are expected rather than dead.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use perga::app::App;
use perga::config::keymap::Keymap;
use perga::config::schema::{FilesConfig, UiConfig};
use perga::doc::document::Document;
use perga::theme::Theme;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

/// The three terminal sizes every snapshot is taken at.
pub const SIZES: [(u16, u16); 3] = [(120, 40), (80, 24), (40, 10)];

/// An application with built-in defaults, sized for a terminal.
pub fn app(width: u16, height: u16) -> App {
    let mut app = App::new(
        Theme::dark(),
        Keymap::defaults(),
        UiConfig::default(),
        FilesConfig::default(),
    );
    app.update(perga::action::Action::Resize(width, height));
    app
}

/// Walk a vault to completion and feed the result into the app.
///
/// The real walk is a background thread reporting into the event channel; this
/// does the same work synchronously so a test can assert against a finished
/// tree without sleeping on one.
pub fn walk(app: &mut App) {
    use perga::vault::walker::{self, WalkEvent, WalkOptions};

    let root = app.vault.root.clone();
    let actions = std::sync::Mutex::new(Vec::new());

    walker::walk(
        &root,
        WalkOptions::default(),
        &std::sync::atomic::AtomicBool::new(false),
        &|event| {
            actions.lock().unwrap().push(match event {
                WalkEvent::Entries(entries) => perga::action::Action::VaultEntries(entries),
                WalkEvent::Finished(total) => perga::action::Action::VaultWalkFinished(total),
                WalkEvent::Failed(reason) => panic!("the walk failed: {reason}"),
            });
        },
    );

    for action in actions.into_inner().unwrap() {
        app.update(action);
    }
}

/// An application rooted on the fixture vault, with its tree fully walked.
pub fn vault_app(width: u16, height: u16) -> App {
    let mut app = app(width, height);
    app.set_vault_root(vault());
    walk(&mut app);
    app
}

/// An application with the fixture vault walked and a document open.
pub fn app_with(name: &str, width: u16, height: u16) -> App {
    let mut app = app(width, height);
    app.set_vault_root(vault());
    walk(&mut app);

    let path = vault().join(name);
    let document = Document::load(&path)
        .unwrap_or_else(|e| panic!("the fixture `{}` must be readable: {e}", path.display()));
    app.open(document);

    app
}

/// Render one frame and hand back the buffer, for the assertions that are
/// about style rather than text.
pub fn frame_buffer(app: &mut App, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("the test backend never fails");

    terminal
        .draw(|f| perga::ui::draw(app, f))
        .expect("rendering never fails");

    terminal.backend().buffer().clone()
}

/// Hide the crate version in a rendered frame.
///
/// The welcome screen shows `perga <version>`, so without this every release
/// invalidates seven snapshots, and a snapshot that churns on every release
/// is one people learn to accept without reading, which is the whole value of
/// having it gone.
///
/// Replaced character for character, so the grid stays aligned and a version
/// that changes *length* still shows up as a layout change, which it is.
fn hide_version(text: &str) -> String {
    let version = env!("CARGO_PKG_VERSION");
    text.replace(version, &"#".repeat(version.chars().count()))
}

/// Render one frame and return it as text.
///
/// Text rather than a styled dump: a snapshot is only useful if a human can see
/// what changed in the diff, and styles are asserted separately.
///
/// Takes `&mut App` because measuring the document mutates the layout cache;
/// rendering itself stays a pure function of state.
pub fn frame(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("the test backend never fails");

    terminal
        .draw(|f| perga::ui::draw(app, f))
        .expect("rendering never fails");

    let buffer = terminal.backend().buffer();
    let mut out = String::new();

    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        // Trailing spaces make the snapshots noisy without saying anything.
        while out.ends_with(' ') {
            out.pop();
        }
        out.push('\n');
    }

    hide_version(&out)
}

/// The committed fixture vault.
pub fn vault() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault")
}

/// Generate the large document the performance tests and benchmarks use.
///
/// Written at test time rather than committed: a 50,000-line file bloats every
/// clone of the repository for the sake of one benchmark. The path is in
/// `.gitignore`.
pub fn large_document(lines: usize) -> PathBuf {
    use std::fmt::Write as _;

    let dir = vault().join("generated");
    std::fs::create_dir_all(&dir).expect("the fixture directory is writable");

    let path = dir.join(format!("large-{lines}.md"));
    if path.exists() {
        return path;
    }

    let mut source = String::with_capacity(lines * 48);
    source.push_str("# A large document\n\n");

    for i in 0..lines {
        match i % 25 {
            0 => {
                let _ = writeln!(source, "## Section {}\n", i / 25);
            }
            7 => {
                let _ = writeln!(
                    source,
                    "```rust\nfn function_{i}() -> usize {{ {i} }}\n```\n"
                );
            }
            13 => {
                let _ = writeln!(source, "- a list item, number {i}\n");
            }
            _ => {
                let _ = writeln!(
                    source,
                    "Paragraph {i}: prose long enough that it wraps on a narrow \
                     terminal and short enough to stay readable.\n"
                );
            }
        }
    }

    // Written to a unique temporary name and renamed into place: the tests that
    // share this fixture run in parallel, and a reader that finds a half-written
    // file measures a document that is missing its tail.
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let unique = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let temp = dir.join(format!("large-{lines}.{}.{unique}.tmp", std::process::id()));
    std::fs::write(&temp, source).expect("the fixture is writable");
    std::fs::rename(&temp, &path).expect("the fixture is writable");
    path
}
