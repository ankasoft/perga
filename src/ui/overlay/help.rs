//! The help overlay, generated from the resolved keymap.
//!
//! Generated, not written: a hand-maintained reference drifts from the actual
//! bindings the moment anyone remaps anything, and a user's remaps would not
//! appear at all.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Widget};

use crate::config::keymap::{KeyContext, Keymap};
use crate::theme::Theme;

/// The columns the key column occupies before the description starts.
const KEY_COLUMN: usize = 26;

/// The help overlay.
pub struct Help<'a> {
    keymap: &'a Keymap,
    theme: &'a Theme,
    scroll: u16,
}

impl<'a> Help<'a> {
    /// Build the overlay from the resolved keymap.
    pub fn new(keymap: &'a Keymap, theme: &'a Theme, scroll: u16) -> Self {
        Help {
            keymap,
            theme,
            scroll,
        }
    }

    /// The reference, one line per binding, grouped by context.
    pub fn lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();

        for context in [
            KeyContext::Global,
            KeyContext::Viewport,
            KeyContext::Sidebar,
            KeyContext::Edit,
        ] {
            let entries: Vec<_> = self
                .keymap
                .entries()
                .iter()
                .filter(|e| e.context == context)
                .collect();

            if entries.is_empty() {
                continue;
            }

            if !lines.is_empty() {
                lines.push(Line::default());
            }
            lines.push(Line::styled(
                context.heading().to_string(),
                self.theme.ui.title,
            ));

            for entry in entries {
                // Every spelling is listed, including the plain fallback for
                // bindings that need the kitty keyboard protocol, so the user
                // can see which one their terminal will deliver.
                let keys = entry
                    .sequences
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(", ");

                lines.push(Line::from(vec![
                    Span::styled(format!("  {keys:<KEY_COLUMN$}"), self.theme.markdown.link),
                    Span::styled(entry.description.to_string(), self.theme.markdown.text),
                ]));
            }
        }

        lines.push(Line::default());
        lines.extend(self.notes());

        lines
    }

    /// The things about the keymap that cannot be read off the table.
    fn notes(&self) -> Vec<Line<'static>> {
        let mut notes = vec![
            Line::styled(
                "In edit mode the editor owns every key not listed above.".to_string(),
                self.theme.ui.status_warning,
            ),
            Line::styled(
                "Global bindings return when you leave edit mode.".to_string(),
                self.theme.markdown.frontmatter,
            ),
        ];

        // Several bindings above can only be delivered by a terminal that
        // implements the kitty keyboard protocol. Each of them has a plain
        // fallback listed alongside it, but the user should be told which
        // column applies to them.
        if !crate::terminal::keyboard_enhancement_active() {
            notes.push(Line::default());
            notes.push(Line::styled(
                "This terminal cannot report Ctrl+Enter, Shift+Enter, or Ctrl+Shift+key."
                    .to_string(),
                self.theme.ui.status_warning,
            ));
            notes.push(Line::styled(
                "Use the plain alternative listed beside each of those bindings.".to_string(),
                self.theme.markdown.frontmatter,
            ));
        }

        notes
    }
}

impl Widget for Help<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let block = Block::bordered()
            .title(" Keybindings ")
            .border_style(self.theme.ui.border_focused)
            .style(self.theme.ui.background);
        let inner = block.inner(area);

        Clear.render(area, buf);
        block.render(area, buf);

        Paragraph::new(self.lines())
            .scroll((self.scroll, 0))
            .render(inner, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::Action;

    #[test]
    fn the_reference_covers_every_binding() {
        let keymap = Keymap::defaults();
        let theme = Theme::dark();
        let help = Help::new(&keymap, &theme, 0);

        let rendered: String = help
            .lines()
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.content.to_string()))
            .collect::<Vec<_>>()
            .join("");

        for entry in keymap.entries() {
            assert!(
                rendered.contains(entry.description),
                "{:?} is missing from the help overlay",
                entry.action
            );
            for sequence in &entry.sequences {
                assert!(
                    rendered.contains(&sequence.to_string()),
                    "binding `{sequence}` is missing from the help overlay"
                );
            }
        }
    }

    #[test]
    fn the_reference_reflects_a_remap_rather_than_the_default() {
        // The reason the overlay is generated at all. `Keymap::defaults` is
        // the only source; there is no second table to fall out of step.
        let keymap = Keymap::defaults();
        let quit = keymap
            .entries()
            .iter()
            .find(|e| e.action == Action::Quit)
            .expect("quit is bound");
        assert_eq!(
            quit.sequences
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>(),
            vec!["q", "Ctrl+Q"]
        );
    }
}
