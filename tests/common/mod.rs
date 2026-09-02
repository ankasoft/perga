//! Helpers shared by the integration tests.

use perga::app::App;
use perga::config::keymap::Keymap;
use perga::config::schema::UiConfig;
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

/// Render the application once and return the frame as text.
///
/// Text rather than a styled dump: a snapshot is only useful if a human can see
/// what changed in the diff, and the styles are asserted separately.
pub fn frame(app: &App, width: u16, height: u16) -> String {
    let mut terminal =
        Terminal::new(TestBackend::new(width, height)).expect("the test backend never fails");

    terminal
        .draw(|f| perga::ui::render(app, f))
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
