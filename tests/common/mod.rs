//! Helpers shared by the integration tests.

use std::path::{Path, PathBuf};

use perga::app::App;
use perga::config::keymap::Keymap;
use perga::config::schema::UiConfig;
use perga::doc::document::Document;
use perga::theme::Theme;
use ratatui::backend::TestBackend;
use ratatui::Terminal;

/// The three terminal sizes every snapshot is taken at.
pub const SIZES: [(u16, u16); 3] = [(120, 40), (80, 24), (40, 10)];

/// An application with built-in defaults, sized for a terminal.
pub fn app(width: u16, height: u16) -> App {
    let mut app = App::new(Theme::dark(), Keymap::defaults(), UiConfig::default());
    app.update(perga::action::Action::Resize(width, height));
    app
}

/// An application with a fixture document open.
pub fn app_with(name: &str, width: u16, height: u16) -> App {
    let mut app = app(width, height);
    app.set_vault_root(vault());

    let path = vault().join(name);
    let document = Document::load(&path)
        .unwrap_or_else(|e| panic!("the fixture `{}` must be readable: {e}", path.display()));
    app.open(document);

    app
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

    out
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

    std::fs::write(&path, source).expect("the fixture is writable");
    path
}
