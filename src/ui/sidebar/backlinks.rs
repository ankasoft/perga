//! The links mode: what the active document points at, and what points at it.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::app::App;
use crate::doc::links::LinkKind;

/// Outgoing links and backlinks for the active document.
pub struct LinksMode<'a> {
    app: &'a App,
}

impl<'a> LinksMode<'a> {
    /// Draw the links for the current state.
    pub fn new(app: &'a App) -> Self {
        LinksMode { app }
    }

    /// The lines of both sections.
    fn lines(&self) -> Vec<Line<'static>> {
        let theme = &self.app.theme;

        let Some(doc) = self.app.tab().doc.as_ref() else {
            return vec![Line::styled("no document", theme.sidebar.file_other)];
        };

        let mut lines = vec![Line::styled("outgoing", theme.sidebar.mode_active)];

        if doc.links.is_empty() {
            lines.push(Line::styled("  none", theme.sidebar.file_other));
        }

        for link in &doc.links {
            let broken = self.app.is_broken(link);
            let style = match (broken, link.kind) {
                (true, _) => theme.markdown.link_broken,
                (_, LinkKind::Wiki) => theme.markdown.wikilink,
                (_, LinkKind::Autolink) => theme.markdown.link_external,
                (_, LinkKind::Inline) => theme.markdown.link,
            };

            // The broken marker is text as well as colour: a reader on an
            // ANSI-16 terminal has few colours to tell these apart with.
            let marker = if broken { "✗ " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(marker, style),
                Span::styled(link.text.clone(), style),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::styled(
            self.backlink_heading(),
            theme.sidebar.mode_active,
        ));

        if !self.app.wikilinks.enabled {
            lines.push(Line::styled(
                "  wiki-links are off",
                theme.sidebar.file_other,
            ));
            return lines;
        }

        if !self.app.vault.index.ready {
            return lines;
        }

        let backlinks = self.app.backlinks();
        if backlinks.is_empty() {
            lines.push(Line::styled("  none", theme.sidebar.file_other));
        }

        for backlink in backlinks {
            lines.push(Line::from(Span::styled(
                format!("  {}:{}", backlink.source.display(), backlink.line),
                theme.sidebar.file,
            )));
            lines.push(Line::from(Span::styled(
                format!("    {}", backlink.context),
                theme.sidebar.file_other,
            )));
        }

        lines
    }

    /// The backlinks heading, which doubles as the indexer's progress meter.
    fn backlink_heading(&self) -> String {
        let index = &self.app.vault.index;

        match (index.ready, index.total) {
            (true, _) => "backlinks".to_string(),
            (false, Some(total)) => format!("indexing… ({}/{total} files)", index.indexed),
            (false, None) => "indexing…".to_string(),
        }
    }
}

impl Widget for LinksMode<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if area.height == 0 {
            return;
        }
        Paragraph::new(self.lines()).render(area, buf);
    }
}
