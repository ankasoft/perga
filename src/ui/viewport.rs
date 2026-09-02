//! The document viewport: windowed block rendering and the scrollbar.
//!
//! A tab with no document open shows the welcome screen instead.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, StatefulWidget, Widget,
};
use unicode_width::UnicodeWidthStr;

use crate::app::{App, Focus};
use crate::doc::render::RenderedDocument;
use crate::ui::welcome::Welcome;

/// Shown at the end of a line that has been clipped.
pub const CLIP_INDICATOR: &str = "…";

/// The document viewport.
pub struct Viewport<'a> {
    app: &'a App,
    lines: &'a [Line<'static>],
    /// Total rendered lines, or `None` while the document is still being
    /// measured, in which case the scrollbar is indeterminate.
    total: Option<usize>,
}

impl<'a> Viewport<'a> {
    /// Build the viewport from lines the caller has already measured.
    ///
    /// Rendering is a pure function of state, so the measuring — which mutates
    /// the layout cache — happens before the frame, not during it.
    pub fn new(app: &'a App, lines: &'a [Line<'static>], total: Option<usize>) -> Self {
        Viewport { app, lines, total }
    }
}

impl Viewport<'_> {
    /// Where the focused link was drawn, as `(row, column, width)`.
    ///
    /// `None` while hint mode is open: the labels are the cue then, and a
    /// second highlight competing with them only adds noise.
    fn focused_link_placement(&self) -> Option<(usize, u16, u16)> {
        use unicode_width::UnicodeWidthStr;

        if self.app.overlay.is_some() {
            return None;
        }

        let tab = self.app.tab();
        let doc = tab.doc.as_ref()?;
        let link = doc.links.get(tab.focused_link?)?;
        let map = tab.layout.line_map();

        let placement = crate::ui::hints::place(self.lines, &[link], tab.scroll, |link| {
            map.line_of_offset(link.range.start)
        })
        .into_iter()
        .next()
        .flatten()?;

        Some((
            usize::from(placement.row),
            placement.column,
            u16::try_from(link.text.trim().width()).ok()?,
        ))
    }
}

impl Widget for Viewport<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let theme = &self.app.theme;
        let focused = self.app.focus == Focus::Viewport;

        let block = Block::bordered().border_style(if focused {
            theme.ui.border_focused
        } else {
            theme.ui.border
        });
        let inner = block.inner(area);
        block.render(area, buf);

        let tab = self.app.tab();

        let Some(_) = &tab.doc else {
            Welcome::new(theme, &self.app.keymap).render(inner, buf);
            return;
        };

        let focused_link = self.focused_link_placement();
        let query = tab.find.as_ref().map(|find| find.query()).unwrap_or("");

        let clipped: Vec<Line<'static>> = self
            .lines
            .iter()
            .enumerate()
            .map(|(row, line)| {
                let mut line = match focused_link {
                    // Restyled before clipping, so a focused link that is
                    // partly off the right-hand side is still marked on the
                    // part that shows.
                    Some((at, column, width)) if at == row => {
                        restyle(line, column, width, theme.markdown.link_focused)
                    }
                    _ => line.clone(),
                };

                if !query.is_empty() {
                    line = highlight_matches(&line, query, theme.sidebar.r#match);
                }

                clip_line(&line, tab.hscroll, inner.width)
            })
            .collect();

        Paragraph::new(clipped).render(inner, buf);

        if inner.height > 0 {
            let mut state = match self.total {
                Some(total) => ScrollbarState::new(total.saturating_sub(usize::from(inner.height)))
                    .position(tab.scroll),
                // Indeterminate: the document has not been measured to the end,
                // so any total would be a guess that jumps when corrected.
                None => ScrollbarState::new(0).position(0),
            };

            Scrollbar::new(ScrollbarOrientation::VerticalRight)
                .style(theme.ui.scrollbar)
                .begin_symbol(None)
                .end_symbol(None)
                .render(area, buf, &mut state);
        }
    }
}

/// Apply the horizontal offset to a line and clip it to the viewport.
///
/// Code blocks and wide tables are never wrapped, so this is how their right
/// hand side is reached. Column arithmetic uses display widths, so a CJK column
/// is not cut in half.
pub fn clip_line(line: &Line<'static>, offset: u16, width: u16) -> Line<'static> {
    let width = usize::from(width);
    if width == 0 {
        return Line::default();
    }

    let total: usize = line.spans.iter().map(|s| s.content.width()).sum();
    if offset == 0 && total <= width {
        return line.clone();
    }

    let offset = usize::from(offset);
    let mut out: Vec<Span<'static>> = Vec::with_capacity(line.spans.len());
    let mut column = 0usize;
    let mut used = 0usize;

    for span in &line.spans {
        let mut kept = String::new();

        for c in span.content.chars() {
            let w = c.to_string().width();

            if column + w <= offset {
                column += w;
                continue;
            }
            if used + w > width {
                break;
            }

            kept.push(c);
            column += w;
            used += w;
        }

        if !kept.is_empty() {
            out.push(Span::styled(kept, span.style));
        }
        if used >= width {
            break;
        }
    }

    // Say so when there is more to the right, rather than leaving the reader to
    // guess whether the line ended there.
    if total > offset + used {
        let indicator_width = CLIP_INDICATOR.width();
        while used + indicator_width > width {
            let Some(last) = out.last_mut() else { break };
            let mut content = last.content.to_string();
            let Some(c) = content.pop() else {
                out.pop();
                continue;
            };
            used -= c.to_string().width();
            if content.is_empty() {
                out.pop();
            } else {
                *last = Span::styled(content, last.style);
            }
        }

        let style = out.last().map_or_else(Default::default, |s| s.style);
        out.push(Span::styled(CLIP_INDICATOR.to_string(), style));
    }

    Line::from(out).style(line.style)
}

/// Restyle every occurrence of `query` in a rendered line.
///
/// Matching is against what is on screen rather than against the source: the
/// reader is looking at rendered text, and a highlight landing on a different
/// run of characters is worse than none at all. See `docs/decisions.md`.
pub fn highlight_matches(
    line: &Line<'static>,
    query: &str,
    style: ratatui::style::Style,
) -> Line<'static> {
    let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
    let found = crate::search::in_doc::matches_in_line(&text, query);

    if found.is_empty() {
        return line.clone();
    }

    // The ranges come back in byte offsets and `restyle` works in columns, so
    // each one is converted before it is applied.
    let mut out = line.clone();
    for (start, end) in found {
        let column = text[..start].width() as u16;
        let width = text[start..end].width() as u16;
        out = restyle(&out, column, width, style);
    }

    out
}

/// Restyle a run of columns within a line.
///
/// Spans are split at the boundaries rather than replaced wholesale, so the
/// styling of the text either side of the run survives.
pub fn restyle(
    line: &Line<'static>,
    from: u16,
    width: u16,
    style: ratatui::style::Style,
) -> Line<'static> {
    let (from, width) = (usize::from(from), usize::from(width));
    let until = from + width;

    let mut out: Vec<Span<'static>> = Vec::with_capacity(line.spans.len() + 2);
    let mut column = 0usize;

    for span in &line.spans {
        let mut current = String::new();
        let mut current_inside = None;

        for c in span.content.chars() {
            let w = c.to_string().width();
            let inside = column >= from && column < until;

            if current_inside != Some(inside) && !current.is_empty() {
                out.push(Span::styled(
                    std::mem::take(&mut current),
                    if current_inside == Some(true) {
                        style
                    } else {
                        span.style
                    },
                ));
            }

            current_inside = Some(inside);
            current.push(c);
            column += w;
        }

        if !current.is_empty() {
            out.push(Span::styled(
                current,
                if current_inside == Some(true) {
                    style
                } else {
                    span.style
                },
            ));
        }
    }

    Line::from(out).style(line.style)
}

/// Measure the active tab's visible window.
///
/// Separated from rendering because it mutates the layout cache, and rendering
/// is a pure function of state.
pub fn measure(app: &mut App) -> (Vec<Line<'static>>, Option<usize>) {
    let inner = app.viewport_inner();
    let renderer = app.renderer(inner.width);
    let scroll = app.tab().scroll;

    let index = app.active_tab;
    let Some(doc) = app.tabs[index].doc.take() else {
        return (Vec::new(), None);
    };

    let layout: &mut RenderedDocument = &mut app.tabs[index].layout;
    let lines = layout.window(&doc, &renderer, scroll, inner.height);
    let total = layout.total_lines(&doc);

    app.tabs[index].doc = Some(doc);
    (lines, total)
}

/// Measure and clip one line the way the viewport would, for tests.
#[cfg(test)]
pub fn clip_for_test(line: &Line<'static>, offset: u16, width: u16) -> String {
    clip_line(line, offset, width)
        .spans
        .iter()
        .map(|s| s.content.as_ref())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    fn line(text: &str) -> Line<'static> {
        Line::from(vec![Span::styled(text.to_string(), Style::default())])
    }

    #[test]
    fn a_line_that_fits_is_untouched() {
        assert_eq!(clip_for_test(&line("short"), 0, 20), "short");
    }

    #[test]
    fn a_long_line_is_clipped_with_an_indicator() {
        let clipped = clip_for_test(&line("abcdefghijklmnop"), 0, 10);
        assert_eq!(clipped.width(), 10);
        assert!(clipped.ends_with(CLIP_INDICATOR), "{clipped:?}");
        assert!(clipped.starts_with("abcdefghi"), "{clipped:?}");
    }

    #[test]
    fn the_horizontal_offset_moves_the_window() {
        let clipped = clip_for_test(&line("abcdefghijklmnop"), 5, 10);
        assert!(clipped.starts_with("fgh"), "{clipped:?}");
        assert!(clipped.ends_with(CLIP_INDICATOR), "{clipped:?}");
    }

    #[test]
    fn scrolling_to_the_end_drops_the_indicator() {
        let clipped = clip_for_test(&line("abcdefghij"), 5, 10);
        assert_eq!(clipped, "fghij");
    }

    #[test]
    fn clipping_counts_display_columns() {
        // Each of these is two columns wide.
        let clipped = clip_for_test(&line("日本語日本語日本語"), 0, 9);
        assert!(clipped.width() <= 9, "{clipped:?} is {}", clipped.width());
        assert!(clipped.ends_with(CLIP_INDICATOR), "{clipped:?}");
    }

    #[test]
    fn styles_survive_clipping() {
        let styled = Line::from(vec![
            Span::styled(
                "aaaa".to_string(),
                Style::default().fg(ratatui::style::Color::Red),
            ),
            Span::styled(
                "bbbb".to_string(),
                Style::default().fg(ratatui::style::Color::Blue),
            ),
        ]);
        let clipped = clip_line(&styled, 2, 4);
        assert_eq!(clipped.spans.len(), 3);
        assert_eq!(clipped.spans[0].content, "aa");
        assert_eq!(clipped.spans[0].style.fg, Some(ratatui::style::Color::Red));
        assert_eq!(clipped.spans[1].style.fg, Some(ratatui::style::Color::Blue));
    }

    #[test]
    fn restyling_splits_spans_at_the_boundaries() {
        let styled = Line::from(vec![Span::styled(
            "see the docs here".to_string(),
            Style::default(),
        )]);
        let focus = Style::default().fg(ratatui::style::Color::Red);

        let out = restyle(&styled, 8, 4, focus);
        let text: Vec<&str> = out.spans.iter().map(|s| s.content.as_ref()).collect();

        assert_eq!(text, ["see the ", "docs", " here"]);
        assert_eq!(out.spans[1].style.fg, Some(ratatui::style::Color::Red));
        assert_eq!(out.spans[0].style.fg, None);
    }

    #[test]
    fn restyling_a_run_that_spans_two_spans_keeps_both_halves() {
        let styled = Line::from(vec![
            Span::styled("abcd".to_string(), Style::default()),
            Span::styled("efgh".to_string(), Style::default()),
        ]);
        let focus = Style::default().fg(ratatui::style::Color::Red);

        let out = restyle(&styled, 2, 4, focus);
        let text: Vec<&str> = out.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, ["ab", "cd", "ef", "gh"]);
        assert_eq!(out.spans[1].style.fg, Some(ratatui::style::Color::Red));
        assert_eq!(out.spans[2].style.fg, Some(ratatui::style::Color::Red));
        assert_eq!(out.spans[3].style.fg, None);
    }

    #[test]
    fn restyling_nothing_leaves_the_line_alone() {
        let styled = line("unchanged");
        let out = restyle(
            &styled,
            0,
            0,
            Style::default().fg(ratatui::style::Color::Red),
        );
        let text: String = out.spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "unchanged");
        assert!(out.spans.iter().all(|s| s.style.fg.is_none()));
    }

    #[test]
    fn an_offset_past_the_end_leaves_an_empty_line() {
        assert_eq!(clip_for_test(&line("short"), 100, 10), "");
    }

    #[test]
    fn a_zero_width_viewport_produces_nothing() {
        assert_eq!(clip_for_test(&line("anything"), 0, 0), "");
    }
}
