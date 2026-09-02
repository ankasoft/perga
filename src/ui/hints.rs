//! Link hint labels: assignment, matching, and drawing.
//!
//! Pressing `f` labels every link in view and typing a label follows it. On a
//! dense document this beats cycling with `n`, which is why Section 8.4 asks
//! for both.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Widget;

use crate::app::App;
use crate::doc::links::Link;

/// The keys labels are built from, in the order they are handed out.
///
/// Home row first, so the links nearest the top of the viewport — the ones the
/// reader is most likely to want — get the keys that need no finger movement.
pub const KEYS: [char; 9] = ['a', 's', 'd', 'f', 'g', 'h', 'j', 'k', 'l'];

/// Assign a label to each of `count` links.
///
/// Single letters while they last, then two-letter combinations of the same
/// set. Every label is the same length within a run, so no label is a prefix of
/// another and typing is never ambiguous.
pub fn labels(count: usize) -> Vec<String> {
    if count <= KEYS.len() {
        return KEYS.iter().take(count).map(|c| c.to_string()).collect();
    }

    let mut out = Vec::with_capacity(count);
    for &first in &KEYS {
        for &second in &KEYS {
            out.push(format!("{first}{second}"));
            if out.len() == count {
                return out;
            }
        }
    }

    // 81 labels is more links than fit on any terminal; anything beyond that
    // goes unlabelled rather than growing to three letters.
    out
}

/// What typing one more character into hint mode did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HintMatch {
    /// The typed text completes this label; the value is its index.
    Complete(usize),
    /// The typed text is a prefix of at least one label.
    Partial,
    /// Nothing starts with the typed text.
    None,
}

/// Match typed text against the labels for `count` links.
pub fn match_typed(typed: &str, count: usize) -> HintMatch {
    if typed.is_empty() {
        return HintMatch::Partial;
    }

    let labels = labels(count);

    if let Some(index) = labels.iter().position(|label| label == typed) {
        return HintMatch::Complete(index);
    }

    if labels.iter().any(|label| label.starts_with(typed)) {
        HintMatch::Partial
    } else {
        HintMatch::None
    }
}

/// Where a link ended up on screen, in viewport coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Placement {
    /// Row within the viewport's inner area.
    pub row: u16,
    /// Column within the viewport's inner area.
    pub column: u16,
}

/// Find where each hinted link was drawn.
///
/// The rendered lines are searched for the link's own text rather than the
/// position being derived from the byte offset: wrapping, list markers, and
/// table borders all move text sideways, and the text that is actually on
/// screen is the only thing that knows where it ended up.
pub fn place(
    lines: &[Line<'static>],
    links: &[&Link],
    first_line: usize,
    line_of: impl Fn(&Link) -> Option<usize>,
) -> Vec<Option<Placement>> {
    use unicode_width::UnicodeWidthStr;

    // A line can hold several links with the same text, so each match is
    // consumed as it is used.
    let mut used: Vec<(usize, usize)> = Vec::new();

    links
        .iter()
        .map(|link| {
            let line = line_of(link)?.checked_sub(first_line)?;
            let rendered = lines.get(line)?;
            let text: String = rendered.spans.iter().map(|s| s.content.as_ref()).collect();

            let needle = link.text.trim();
            if needle.is_empty() {
                return None;
            }

            let from = used
                .iter()
                .filter(|(l, _)| *l == line)
                .map(|(_, end)| *end)
                .max()
                .unwrap_or(0);

            let at = text.get(from..)?.find(needle)? + from;
            used.push((line, at + needle.len()));

            Some(Placement {
                row: u16::try_from(line).ok()?,
                column: u16::try_from(text[..at].width()).ok()?,
            })
        })
        .collect()
}

/// The hint labels, drawn over the viewport's content.
pub struct Hints<'a> {
    app: &'a App,
    lines: &'a [Line<'static>],
    links: &'a [usize],
    typed: &'a str,
}

impl<'a> Hints<'a> {
    /// Draw labels for `links` over the already-rendered `lines`.
    pub fn new(
        app: &'a App,
        lines: &'a [Line<'static>],
        links: &'a [usize],
        typed: &'a str,
    ) -> Self {
        Hints {
            app,
            lines,
            links,
            typed,
        }
    }
}

impl Widget for Hints<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let Some(doc) = self.app.tab().doc.as_ref() else {
            return;
        };

        let tab = self.app.tab();
        let map = tab.layout.line_map();
        let hinted: Vec<&Link> = self
            .links
            .iter()
            .filter_map(|&index| doc.links.get(index))
            .collect();

        let placements = place(self.lines, &hinted, tab.scroll, |link| {
            map.line_of_offset(link.range.start)
        });
        let labels = labels(hinted.len());
        let style = self.app.theme.hints.label;

        for (label, placement) in labels.iter().zip(placements) {
            let Some(placement) = placement else { continue };

            // A label the reader has started typing is only worth drawing if it
            // still matches; the rest are dropped so what is left is the
            // shortlist.
            if !label.starts_with(self.typed) {
                continue;
            }

            let row = area.y + placement.row;
            let column = area.x + placement.column;
            if row >= area.bottom() || column >= area.right() {
                continue;
            }

            let width = area.right() - column;
            buf.set_line(
                column,
                row,
                &Line::from(Span::styled(label.clone(), style)),
                width,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ops::Range;

    use crate::doc::links::LinkKind;

    fn link(text: &str, range: Range<usize>) -> Link {
        Link {
            text: text.to_string(),
            target: "x.md".to_string(),
            range,
            kind: LinkKind::Inline,
        }
    }

    #[test]
    fn short_runs_get_single_letters_from_the_home_row() {
        assert_eq!(labels(3), ["a", "s", "d"]);
        assert_eq!(labels(9).last().unwrap(), "l");
    }

    #[test]
    fn longer_runs_get_two_letters_and_no_label_is_a_prefix_of_another() {
        let labels = labels(12);
        assert_eq!(labels[0], "aa");
        assert_eq!(labels[9], "sa");
        assert!(labels.iter().all(|l| l.len() == 2));
    }

    #[test]
    fn every_label_is_unique() {
        let labels = labels(81);
        let unique: std::collections::HashSet<_> = labels.iter().collect();
        assert_eq!(unique.len(), 81);
    }

    #[test]
    fn typing_narrows_to_one_label() {
        assert_eq!(match_typed("d", 3), HintMatch::Complete(2));
        assert_eq!(match_typed("", 3), HintMatch::Partial);
        assert_eq!(match_typed("z", 3), HintMatch::None);

        // Two-letter labels need both characters.
        assert_eq!(match_typed("s", 12), HintMatch::Partial);
        assert_eq!(match_typed("sa", 12), HintMatch::Complete(9));
        assert_eq!(match_typed("sz", 12), HintMatch::None);
    }

    #[test]
    fn a_label_is_placed_where_its_text_was_drawn() {
        let lines = vec![Line::from("see the docs here")];
        let links = [link("docs", 0..1)];
        let refs: Vec<&Link> = links.iter().collect();

        let placed = place(&lines, &refs, 0, |_| Some(0));
        assert_eq!(placed[0], Some(Placement { row: 0, column: 8 }));
    }

    #[test]
    fn two_links_with_the_same_text_on_one_line_get_different_columns() {
        let lines = vec![Line::from("docs and docs")];
        let links = [link("docs", 0..1), link("docs", 9..10)];
        let refs: Vec<&Link> = links.iter().collect();

        let placed = place(&lines, &refs, 0, |_| Some(0));
        assert_eq!(placed[0].unwrap().column, 0);
        assert_eq!(placed[1].unwrap().column, 9);
    }

    #[test]
    fn a_column_counts_display_width() {
        let lines = vec![Line::from("日本語 docs")];
        let links = [link("docs", 0..1)];
        let refs: Vec<&Link> = links.iter().collect();

        let placed = place(&lines, &refs, 0, |_| Some(0));
        assert_eq!(placed[0].unwrap().column, 7);
    }

    #[test]
    fn a_link_that_was_not_drawn_has_no_placement() {
        let lines = vec![Line::from("nothing here")];
        let links = [link("docs", 0..1)];
        let refs: Vec<&Link> = links.iter().collect();

        assert_eq!(place(&lines, &refs, 0, |_| Some(0))[0], None);
        // ...and neither has one on a line above the window.
        assert_eq!(place(&lines, &refs, 10, |_| Some(3))[0], None);
    }
}
