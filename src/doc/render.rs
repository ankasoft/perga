//! Block rendering, the owned-line cache, and the byte-offset to line map.
//!
//! # The owned-vs-borrowed boundary
//!
//! `tui_markdown::from_str` returns a `Text` that borrows its input. Storing
//! that alongside the source it borrows from is a fight with the borrow checker
//! that cannot be won cleanly, so everything is converted to
//! `Vec<Line<'static>>` here, at the cache boundary, and nothing downstream
//! ever sees a borrowed line.
//!
//! # Why the cache is keyed by content, not position
//!
//! Inserting one character shifts the byte range of every block after it.
//! Keying the cache by byte range would therefore throw away most of the cache
//! on every keystroke. Hashing the block's *source text* means an unchanged
//! block hits the cache no matter where it moved to. The width is part of the
//! key because wrapping changes the rendered lines, so a resize has to miss.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Arc;

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use tui_markdown::{ImageFallback, Options, StyleSheet};
use unicode_width::UnicodeWidthStr;

use crate::doc::document::{Block, BlockKind, Document};
use crate::doc::highlight::{Highlighter, FALLBACK_CODE_THEME};
use crate::theme::Theme;

/// The marker `tui-markdown` puts in front of an image's alt text.
const TUI_MARKDOWN_IMAGE_MARKER: &str = "[img]";

/// The glyph shown for a completed task list item.
pub const TASK_DONE: &str = "☑";
/// The glyph shown for an outstanding task list item.
pub const TASK_TODO: &str = "☐";

/// How many screens of blocks are rendered beyond the visible window, so that
/// scrolling does not stutter at the edges.
pub const OVERSCAN_SCREENS: usize = 1;

/// The most blocks resolved in one call when the whole document has to be
/// measured, so that a jump to the end of a huge document does not freeze the
/// UI for a frame.
pub const RESOLVE_CHUNK: usize = 400;

/// Bridges perga's [`Theme`] to `tui-markdown`'s style hooks.
///
/// Owns its styles rather than borrowing the theme, because the trait requires
/// `Clone + Send + Sync + 'static`.
#[derive(Debug, Clone)]
pub struct ThemeStyleSheet {
    headings: [Style; 6],
    code: Style,
    link: Style,
    blockquote: Style,
    metadata: Style,
    html: Style,
    footnote: Style,
    table_header: Style,
    table_cell: Style,
    table_border: Style,
    image: Style,
    text: Style,
}

impl ThemeStyleSheet {
    /// Build the style sheet for a theme.
    pub fn new(theme: &Theme) -> Self {
        let md = &theme.markdown;
        ThemeStyleSheet {
            headings: [md.h1, md.h2, md.h3, md.h4, md.h5, md.h6],
            code: md.code_inline,
            link: md.link,
            blockquote: md.blockquote,
            metadata: md.frontmatter,
            html: md.html,
            footnote: md.footnote,
            table_header: md.table_header,
            table_cell: md.text,
            table_border: md.table_border,
            image: md.image_placeholder,
            text: md.text,
        }
    }
}

impl StyleSheet for ThemeStyleSheet {
    fn heading(&self, level: u8) -> Style {
        self.headings[(level.clamp(1, 6) - 1) as usize]
    }

    fn code(&self) -> Style {
        self.code
    }

    fn link(&self) -> Style {
        self.link
    }

    fn blockquote(&self) -> Style {
        self.blockquote
    }

    fn metadata_block(&self) -> Style {
        self.metadata
    }

    fn html(&self) -> Style {
        self.html
    }

    fn footnote_ref(&self) -> Style {
        self.footnote
    }

    fn footnote_def(&self) -> Style {
        self.footnote
    }

    /// Omitted. The fences are markup, and read mode does not show markup; the
    /// block's background and highlighting are what mark it as code.
    ///
    /// Dropping them also makes a code block's rendered lines correspond one to
    /// one with its source lines, which is what keeps the offset map exact
    /// there.
    fn code_block_fence(&self) -> &str {
        ""
    }

    fn table_header(&self) -> Style {
        self.table_header
    }

    fn table_cell(&self) -> Style {
        self.table_cell
    }

    fn table_border(&self) -> Style {
        self.table_border
    }

    fn image_alt(&self) -> Style {
        self.image
    }

    fn math_inline(&self) -> Style {
        self.text
    }

    fn math_display(&self) -> Style {
        self.text
    }
}

/// What a rendered line came from in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct LineOrigin {
    /// The rendered line's index within the document.
    line: usize,
    /// The byte offset in the source that the line begins at.
    offset: usize,
}

/// A bidirectional map between source byte offsets and rendered line numbers.
///
/// Five features depend on this: anchor scrolling, find highlighting, outline
/// synchronisation, link positioning, and edit-mode cursor round-tripping. It
/// is built deliberately rather than falling out of rendering as a side effect,
/// because when it does the latter all five go subtly wrong in different ways.
#[derive(Debug, Clone, Default)]
pub struct LineMap {
    /// Sorted by both fields at once: rendered lines and source offsets both
    /// increase together, so one binary search serves both directions.
    origins: Vec<LineOrigin>,
}

impl LineMap {
    /// Forget everything. Called when the width changes or the document
    /// reloads.
    fn clear(&mut self) {
        self.origins.clear();
    }

    /// Record that `line` begins at `offset`.
    fn push(&mut self, line: usize, offset: usize) {
        if let Some(last) = self.origins.last() {
            // Several rendered lines can share a source offset when one source
            // line wraps; only the first is an entry point.
            if last.offset >= offset || last.line >= line {
                return;
            }
        }
        self.origins.push(LineOrigin { line, offset });
    }

    /// The rendered line that contains a source offset.
    pub fn line_of_offset(&self, offset: usize) -> Option<usize> {
        let index = match self.origins.binary_search_by_key(&offset, |o| o.offset) {
            Ok(i) => i,
            // The offset falls inside the preceding entry's span.
            Err(0) => return self.origins.first().map(|o| o.line),
            Err(i) => i - 1,
        };
        self.origins.get(index).map(|o| o.line)
    }

    /// The source offset a rendered line begins at.
    pub fn offset_of_line(&self, line: usize) -> Option<usize> {
        let index = match self.origins.binary_search_by_key(&line, |o| o.line) {
            Ok(i) => i,
            Err(0) => return self.origins.first().map(|o| o.offset),
            Err(i) => i - 1,
        };
        self.origins.get(index).map(|o| o.offset)
    }

    /// How many entries the map holds.
    pub fn len(&self) -> usize {
        self.origins.len()
    }

    /// Whether the map is empty.
    pub fn is_empty(&self) -> bool {
        self.origins.is_empty()
    }
}

/// The character a thematic break is drawn with.
const RULE_GLYPH: char = '─';

/// The character a block quote's left edge is drawn with.
const BLOCKQUOTE_BAR: char = '│';

/// The key a rendered block is cached under.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct CacheKey {
    /// Hash of the block's source text, so a block that moved still hits.
    content: u64,
    /// Wrapping changes with the width, so a resize has to miss.
    width: u16,
    /// Code blocks rendered before syntect finished must not be served after.
    highlighted: bool,
}

/// A document laid out at one width.
///
/// Block heights are only known after rendering, which makes "which blocks are
/// visible" circular. It is resolved by rendering blocks in order and keeping a
/// running line offset: everything above the viewport has to be measured
/// anyway, and once measured the heights come from the cache.
#[derive(Debug, Default)]
pub struct RenderedDocument {
    /// The width every cached line was wrapped to.
    width: u16,
    /// The document the measurements belong to. Content, not identity: an
    /// edit changes it, and the layout is then rebuilt from a cache that still
    /// holds every block the edit did not touch.
    content_hash: u64,
    /// Rendered height of each block, filled in as blocks are resolved.
    heights: Vec<Option<u16>>,
    /// Rendered line offset of each block, valid below [`Self::resolved`].
    offsets: Vec<usize>,
    /// How many leading blocks have been measured.
    resolved: usize,
    /// Rendered lines, keyed by content rather than position.
    cache: HashMap<CacheKey, Arc<Vec<Line<'static>>>>,
    /// The offset to line map, extended as blocks resolve.
    line_map: LineMap,
    /// How many blocks have actually been rendered, for the tests that assert
    /// only visible blocks are.
    #[cfg(test)]
    render_count: usize,
}

impl RenderedDocument {
    /// A layout with nothing measured yet.
    pub fn new() -> Self {
        RenderedDocument::default()
    }

    /// How many blocks have actually been rendered.
    #[cfg(test)]
    pub fn render_count(&self) -> usize {
        self.render_count
    }

    /// The offset to line map as far as the document has been measured.
    pub fn line_map(&self) -> &LineMap {
        &self.line_map
    }

    /// Whether every block has been measured, so the total line count is known.
    pub fn is_complete(&self, document: &Document) -> bool {
        self.resolved >= document.blocks.len()
    }

    /// The document's total rendered height, or `None` while blocks remain
    /// unmeasured.
    ///
    /// The scrollbar shows as indeterminate until this is known rather than
    /// guessing at a total and jumping when the guess is corrected.
    pub fn total_lines(&self, document: &Document) -> Option<usize> {
        self.is_complete(document).then(|| self.measured_lines())
    }

    /// The rendered height of everything measured so far.
    pub fn measured_lines(&self) -> usize {
        self.offsets.get(self.resolved).copied().unwrap_or(0)
    }

    /// Discard the layout, keeping the cache.
    ///
    /// The cache is keyed by width, so entries for the new width simply miss
    /// and entries for the old one stay available if the terminal is resized
    /// back.
    fn reset(&mut self, document: &Document, width: u16) {
        self.width = width;
        self.content_hash = document.content_hash;
        self.heights = vec![None; document.blocks.len()];
        self.offsets = vec![0; document.blocks.len() + 1];
        self.resolved = 0;
        self.line_map.clear();
    }

    /// Re-key the layout for a document and width, resetting if either changed.
    fn prepare(&mut self, document: &Document, width: u16) {
        if self.width != width || self.content_hash != document.content_hash {
            self.reset(document, width);
        }
    }

    /// Measure blocks until `target` blocks are resolved, or `limit` blocks
    /// have been rendered in this call.
    ///
    /// The limit is what keeps a jump to the end of a 100,000-line document
    /// from freezing a frame: the caller comes back next frame for more.
    fn resolve_to(
        &mut self,
        document: &Document,
        renderer: &Renderer,
        target: usize,
        limit: usize,
    ) {
        let target = target.min(document.blocks.len());
        let mut rendered = 0;

        while self.resolved < target && rendered < limit {
            let index = self.resolved;
            let block = &document.blocks[index];
            let lines = self.render_block(document, renderer, block);

            let start = self.offsets[index];
            self.record_origins(document, block, start, &lines);

            let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
            self.heights[index] = Some(height);
            self.offsets[index + 1] = start + lines.len();
            self.resolved += 1;
            rendered += 1;
        }
    }

    /// Map each of a block's source lines onto the rendered line it starts at.
    ///
    /// Code blocks render one line per source line, so the correspondence there
    /// is exact. Prose is re-wrapped, so its source lines are distributed over
    /// the rendered ones in proportion — which is right at the block's
    /// boundaries and close everywhere between them.
    fn record_origins(
        &mut self,
        document: &Document,
        block: &Block,
        start_line: usize,
        lines: &[Line<'static>],
    ) {
        self.line_map.push(start_line, block.range.start);

        if lines.is_empty() {
            return;
        }

        let source = &document.source[block.range.clone()];
        let mut source_lines: Vec<usize> = source
            .split_inclusive('\n')
            .scan(block.range.start, |offset, line| {
                let at = *offset;
                *offset += line.len();
                Some(at)
            })
            .collect();

        // A fenced code block renders one line per *content* line, so dropping
        // the fences from the source side makes the correspondence exact
        // rather than interpolated.
        let exact = matches!(block.kind, BlockKind::CodeBlock(_)) && is_fenced(source);
        if exact && source_lines.len() >= 2 {
            source_lines.remove(0);
            source_lines.pop();
        }

        if source_lines.len() <= 1 {
            return;
        }

        for (i, offset) in source_lines.iter().enumerate() {
            let line = if exact {
                start_line + i
            } else if i == 0 {
                continue;
            } else {
                start_line + i * lines.len() / source_lines.len()
            };
            self.line_map.push(line, *offset);
        }
    }

    /// Render one block, through the cache.
    fn render_block(
        &mut self,
        document: &Document,
        renderer: &Renderer,
        block: &Block,
    ) -> Arc<Vec<Line<'static>>> {
        let source = block.source(document);
        let key = CacheKey {
            content: hash_of(source),
            width: self.width,
            highlighted: renderer.highlighter.is_ready(),
        };

        if let Some(cached) = self.cache.get(&key) {
            return Arc::clone(cached);
        }

        let lines = Arc::new(renderer.render(block, source, self.width));
        self.cache.insert(key, Arc::clone(&lines));

        #[cfg(test)]
        {
            self.render_count += 1;
        }

        lines
    }

    /// Which blocks intersect a window of rendered lines, with overscan.
    ///
    /// Returns the rendered lines for the window itself, along with the index
    /// of the first line returned.
    pub fn window(
        &mut self,
        document: &Document,
        renderer: &Renderer,
        first_line: usize,
        height: u16,
    ) -> Vec<Line<'static>> {
        self.prepare(document, renderer.width);

        let height = usize::from(height);
        let overscan = height * OVERSCAN_SCREENS;
        let wanted = first_line + height + overscan;

        // Blocks are measured in order, so reaching line `wanted` means
        // resolving until the running total passes it.
        while self.resolved < document.blocks.len() && self.measured_lines() < wanted {
            let before = self.resolved;
            self.resolve_to(
                document,
                renderer,
                self.resolved + RESOLVE_CHUNK,
                RESOLVE_CHUNK,
            );
            if self.resolved == before {
                break;
            }
        }

        let mut out = Vec::with_capacity(height);

        for index in 0..self.resolved {
            let start = self.offsets[index];
            let end = self.offsets[index + 1];

            if end <= first_line {
                continue;
            }
            if start >= first_line + height {
                break;
            }

            let lines = self.render_block(document, renderer, &document.blocks[index]);
            for (i, line) in lines.iter().enumerate() {
                let at = start + i;
                if at >= first_line && at < first_line + height {
                    out.push(line.clone());
                }
            }
        }

        out
    }

    /// Measure the whole document, a chunk at a time.
    ///
    /// Needed by `G`, by an anchor deep in the document, by restoring a saved
    /// scroll offset, and by the scrollbar's total. Returns whether it finished.
    pub fn resolve_all(&mut self, document: &Document, renderer: &Renderer) -> bool {
        self.prepare(document, renderer.width);
        self.resolve_to(document, renderer, document.blocks.len(), RESOLVE_CHUNK);
        self.is_complete(document)
    }

    /// The rendered line a block starts at, measuring up to it if needed.
    pub fn line_of_block(
        &mut self,
        document: &Document,
        renderer: &Renderer,
        index: usize,
    ) -> Option<usize> {
        self.prepare(document, renderer.width);

        while self.resolved <= index && self.resolved < document.blocks.len() {
            let before = self.resolved;
            self.resolve_to(document, renderer, index + 1, usize::MAX);
            if self.resolved == before {
                break;
            }
        }

        (index < self.resolved).then(|| self.offsets[index])
    }

    /// The rendered line a source offset falls on, measuring up to it if
    /// needed.
    pub fn line_of_offset(
        &mut self,
        document: &Document,
        renderer: &Renderer,
        offset: usize,
    ) -> Option<usize> {
        let index = document
            .blocks
            .iter()
            .position(|b| b.range.end > offset)
            .unwrap_or(document.blocks.len().saturating_sub(1));

        self.line_of_block(document, renderer, index)?;
        self.line_map.line_of_offset(offset)
    }
}

/// Renders blocks to owned lines.
///
/// Stateless apart from the width and the theme: the cache lives in
/// [`RenderedDocument`], because it is per document.
#[derive(Debug, Clone)]
pub struct Renderer {
    styles: ThemeStyleSheet,
    highlighter: Highlighter,
    code_theme: String,
    code_block_bg: Style,
    rule: Style,
    blockquote_bar: Style,
    /// Whether a heading keeps the `#` it was written with.
    heading_markers: bool,
    text: Style,
    task_done: Style,
    task_todo: Style,
    /// The width blocks are wrapped to.
    pub width: u16,
}

impl Renderer {
    /// Build a renderer for a theme.
    /// Whether headings keep the `#` they were written with.
    ///
    /// A builder rather than a fourth parameter: every renderer wants the
    /// default, and one call site sets it from `ui.show_heading_markers`.
    pub fn with_heading_markers(mut self, show: bool) -> Self {
        self.heading_markers = show;
        self
    }

    pub fn new(theme: &Theme, highlighter: Highlighter, width: u16) -> Self {
        Renderer {
            styles: ThemeStyleSheet::new(theme),
            highlighter,
            code_theme: theme
                .code_theme
                .clone()
                .unwrap_or_else(|| FALLBACK_CODE_THEME.to_string()),
            code_block_bg: theme.markdown.code_block_bg,
            rule: theme.markdown.rule,
            blockquote_bar: theme.markdown.blockquote_bar,
            heading_markers: true,
            text: theme.markdown.text,
            task_done: theme.markdown.task_done,
            task_todo: theme.markdown.task_todo,
            width,
        }
    }

    /// Render one block to owned lines.
    fn render(&self, block: &Block, source: &str, width: u16) -> Vec<Line<'static>> {
        // Frontmatter is hidden from the rendered body: it is metadata, and it
        // is exposed through the tab label instead.
        if block.kind == BlockKind::Frontmatter {
            return Vec::new();
        }

        if let BlockKind::CodeBlock(language) = &block.kind {
            return self.render_code(source, language.as_deref(), width);
        }

        if block.kind == BlockKind::Rule {
            return self.render_rule(width);
        }

        let options = Options::new(self.styles.clone()).image_fallback(ImageFallback::AltText);
        let text = tui_markdown::from_str_with_options(source, &options);

        // The owned-vs-borrowed boundary. Nothing past this point borrows the
        // source.
        let mut lines: Vec<Line<'static>> = text
            .lines
            .into_iter()
            .map(|line| {
                let spans = line
                    .spans
                    .into_iter()
                    .map(|span| Span::styled(span.content.into_owned(), span.style))
                    .collect::<Vec<_>>();
                Line::from(spans).style(line.style)
            })
            .collect();

        if !self.heading_markers && matches!(block.kind, BlockKind::Heading(_)) {
            lines = lines.into_iter().map(strip_heading_marker).collect();
        }

        lines = lines.into_iter().map(strip_link_destination).collect();
        lines = lines.into_iter().map(render_image_placeholder).collect();

        if block.kind == BlockKind::List {
            lines = lines
                .into_iter()
                .map(|line| render_task_markers(line, &self.task_done, &self.task_todo))
                .collect();
        }

        if block.kind == BlockKind::BlockQuote {
            return with_trailing_blank(self.render_quote_bars(lines, width));
        }

        if !block.kind.is_clipped() {
            lines = lines
                .into_iter()
                .flat_map(|line| wrap_line(line, width))
                .collect();
        }

        with_trailing_blank(lines)
    }

    /// Draw a block quote's `>` markers as a bar down its left edge.
    ///
    /// `tui-markdown` passes the markers through as the characters that were
    /// typed, one span each. The theme has had a `blockquote_bar` key since
    /// the first version and nothing used it.
    ///
    /// The bar is put back *after* wrapping, so it runs down every line of a
    /// quote rather than marking only the first — which is the whole reason a
    /// quote is drawn with a bar rather than a prefix.
    fn render_quote_bars(&self, lines: Vec<Line<'static>>, width: u16) -> Vec<Line<'static>> {
        let mut out = Vec::with_capacity(lines.len());

        for line in lines {
            // Every leading `>` is one level of nesting. A space after them is
            // the separator `tui-markdown` emits, not content.
            let depth = line
                .spans
                .iter()
                .take_while(|span| span.content.as_ref() == ">")
                .count();

            let mut rest: Vec<Span<'static>> = line.spans.into_iter().skip(depth).collect();
            if depth > 0 && rest.first().is_some_and(|s| s.content.as_ref() == " ") {
                rest.remove(0);
            }

            let indent: String = format!("{BLOCKQUOTE_BAR} ").repeat(depth.max(1));
            let inner = usize::from(width).saturating_sub(indent.chars().count());

            for mut wrapped in wrap_line(Line::from(rest).style(line.style), inner as u16) {
                wrapped
                    .spans
                    .insert(0, Span::styled(indent.clone(), self.blockquote_bar));
                out.push(wrapped);
            }
        }

        out
    }

    /// Render a thematic break.
    ///
    /// `tui-markdown` passes `---` through as the three characters that were
    /// typed. A rule is a horizontal line, and the theme has had a `rule` key
    /// for it since the first version.
    fn render_rule(&self, width: u16) -> Vec<Line<'static>> {
        let line = Line::from(Span::styled(
            RULE_GLYPH.to_string().repeat(usize::from(width).max(1)),
            self.rule,
        ));

        with_trailing_blank(vec![line])
    }

    /// Render a fenced code block.
    ///
    /// Never wrapped: wrapping code destroys its meaning. Lines longer than the
    /// viewport are clipped by the viewport and reachable with horizontal
    /// scrolling.
    fn render_code(&self, source: &str, language: Option<&str>, width: u16) -> Vec<Line<'static>> {
        let code = strip_code_fences(source);

        let mut lines = self
            .highlighter
            .highlight(&code, language, &self.code_theme)
            .unwrap_or_else(|| {
                code.lines()
                    .map(|line| Line::from(Span::styled(line.to_string(), self.text)))
                    .collect()
            });

        // The block's background belongs to the theme, not to the syntect
        // theme, so it is applied to whole lines here.
        //
        // A line's style only paints the cells its spans occupy, so a short
        // line would leave the background ending mid-row and the block would
        // read as ragged text rather than as a block. Each line is padded to
        // the viewport width to close it off. A line longer than that is left
        // alone: the viewport clips it and marks it, which is what horizontal
        // scrolling is for.
        for line in &mut lines {
            line.style = line.style.patch(self.code_block_bg);

            let used: usize = line.spans.iter().map(|s| s.content.width()).sum();
            if let Some(gap) = usize::from(width).checked_sub(used).filter(|g| *g > 0) {
                line.spans
                    .push(Span::styled(" ".repeat(gap), self.code_block_bg));
            }
        }

        with_trailing_blank(lines)
    }
}

/// Replace a task list item's `[x]` or `[ ]` with a checkbox glyph.
///
/// `tui-markdown` passes the source markers through as literal text, in the
/// same span as the list bullet: `"- [x] "`. Read mode does not show markup,
/// and a checkbox reads at a glance where a bracketed letter does not.
///
/// Only the line's first span is examined, which is where a list marker always
/// is, so a `[x]` written inside the item's prose is left alone.
fn render_task_markers(line: Line<'static>, done: &Style, todo: &Style) -> Line<'static> {
    let style = line.style;
    let mut spans = line.spans;

    let Some(first) = spans.first() else {
        return Line::from(spans).style(style);
    };

    let content = first.content.to_string();
    let marker_style = first.style;

    let found = ["[x] ", "[X] "]
        .iter()
        .find_map(|m| content.find(m).map(|at| (at, m.len(), TASK_DONE, *done)))
        .or_else(|| {
            content
                .find("[ ] ")
                .map(|at| (at, "[ ] ".len(), TASK_TODO, *todo))
        });

    let Some((at, len, glyph, glyph_style)) = found else {
        return Line::from(spans).style(style);
    };

    let mut replacement = Vec::with_capacity(3);
    if at > 0 {
        replacement.push(Span::styled(content[..at].to_string(), marker_style));
    }
    replacement.push(Span::styled(format!("{glyph} "), glyph_style));
    let rest = &content[at + len..];
    if !rest.is_empty() {
        replacement.push(Span::styled(rest.to_string(), marker_style));
    }

    spans.splice(0..1, replacement);
    Line::from(spans).style(style)
}

/// Whether a code block's source is fenced rather than indented.
fn is_fenced(source: &str) -> bool {
    let first = source.trim_start().trim_end_matches('\r');
    first.starts_with("```") || first.starts_with("~~~")
}

/// Separate a block from the next one with a blank line.
///
/// `tui-markdown` renders a block as its own lines and nothing more, so without
/// this every paragraph, heading, and list runs straight into the next.
fn with_trailing_blank(mut lines: Vec<Line<'static>>) -> Vec<Line<'static>> {
    if !lines.is_empty() {
        lines.push(Line::default());
    }
    lines
}

/// Take the content out of a fenced or indented code block.
fn strip_code_fences(source: &str) -> String {
    let trimmed = source.trim_end_matches(['\n', '\r']);
    let mut lines: Vec<&str> = trimmed.split('\n').collect();

    let is_fence = |line: &str| {
        let line = line.trim_start().trim_end_matches('\r');
        line.starts_with("```") || line.starts_with("~~~")
    };

    if lines.first().is_some_and(|l| is_fence(l)) {
        lines.remove(0);
        if lines.last().is_some_and(|l| is_fence(l)) {
            lines.pop();
        }
    } else {
        // An indented code block: four spaces, or a tab, per line.
        lines = lines
            .into_iter()
            .map(|l| {
                l.strip_prefix("    ")
                    .or_else(|| l.strip_prefix('\t'))
                    .unwrap_or(l)
            })
            .collect();
    }

    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    out
}

/// Drop the `## ` a heading is written with.
///
/// `tui-markdown` emits it as its own leading span, which is what makes this a
/// span to remove rather than text to parse.
fn strip_heading_marker(line: Line<'static>) -> Line<'static> {
    let style = line.style;
    let mut spans = line.spans;

    let is_marker = spans.first().is_some_and(|span| {
        let content = span.content.trim_end();
        !content.is_empty() && content.chars().all(|c| c == '#')
    });

    if is_marker {
        spans.remove(0);
    }

    Line::from(spans).style(style)
}

/// Drop the ` (destination)` that `tui-markdown` appends after a link label.
///
/// Read mode does not show markup, and the destination is not markup the reader
/// asked for. It is emitted as a recognisable triple — an unstyled `" ("`, the
/// destination in the link style, an unstyled `")"` — which is what this
/// matches. The target itself is not lost: links are extracted from the source
/// separately, where they can be resolved properly.
fn strip_link_destination(line: Line<'static>) -> Line<'static> {
    let style = line.style;
    let spans = line.spans;
    let mut out: Vec<Span<'static>> = Vec::with_capacity(spans.len());
    let mut i = 0;

    while i < spans.len() {
        let is_triple = spans[i].content == " ("
            && spans
                .get(i + 2)
                .is_some_and(|close| close.content == ")" && close.style == spans[i].style);

        if is_triple {
            i += 3;
            continue;
        }

        out.push(spans[i].clone());
        i += 1;
    }

    Line::from(out).style(style)
}

/// Rewrite `tui-markdown`'s `[img] alt` marker into perga's placeholder.
///
/// The single place that knows how an image is presented in a terminal that
/// cannot draw one. A future Kitty or Sixel renderer replaces this function and
/// nothing else in the pipeline.
fn render_image_placeholder(line: Line<'static>) -> Line<'static> {
    if !line
        .spans
        .iter()
        .any(|s| s.content == TUI_MARKDOWN_IMAGE_MARKER || s.content == "[img] ")
    {
        return line;
    }

    let style = line.style;
    let mut out: Vec<Span<'static>> = Vec::with_capacity(line.spans.len() + 1);
    let mut spans = line.spans.into_iter().peekable();

    while let Some(span) = spans.next() {
        let is_marker = span.content == TUI_MARKDOWN_IMAGE_MARKER || span.content == "[img] ";
        if !is_marker {
            out.push(span);
            continue;
        }

        let marker_style = span.style;
        out.push(Span::styled("[image: ".to_string(), marker_style));

        // The alt text follows in the same style; the placeholder closes after
        // the last span that shares it.
        while spans.peek().is_some_and(|s| s.style == marker_style) {
            out.push(spans.next().expect("peeked"));
        }

        out.push(Span::styled("]".to_string(), marker_style));
    }

    Line::from(out).style(style)
}

/// Soft-wrap one rendered line to `width` columns.
///
/// Widths come from `unicode-width`, so CJK and emoji occupy the columns they
/// actually occupy rather than one each.
fn wrap_line(line: Line<'static>, width: u16) -> Vec<Line<'static>> {
    let width = usize::from(width);
    if width == 0 {
        return vec![line];
    }

    let total: usize = line.spans.iter().map(|s| s.content.width()).sum();
    if total <= width {
        return vec![line];
    }

    let style = line.style;
    let mut out = Vec::new();
    let mut current: Vec<Span<'static>> = Vec::new();
    let mut used = 0usize;

    for span in line.spans {
        let mut rest = span.content.into_owned();

        while !rest.is_empty() {
            let room = width.saturating_sub(used);
            if room == 0 {
                out.push(Line::from(std::mem::take(&mut current)).style(style));
                used = 0;
                continue;
            }

            let take = split_at_width(&rest, room, used == 0);
            if take == 0 {
                // Nothing fits in the room left; start a new line.
                out.push(Line::from(std::mem::take(&mut current)).style(style));
                used = 0;
                continue;
            }

            let head = rest[..take].to_string();
            rest = rest[take..].trim_start_matches(' ').to_string();

            used += head.width();
            current.push(Span::styled(head, span.style));

            if used >= width && !rest.is_empty() {
                out.push(Line::from(std::mem::take(&mut current)).style(style));
                used = 0;
            }
        }
    }

    if !current.is_empty() || out.is_empty() {
        out.push(Line::from(current).style(style));
    }

    // A break after a space leaves the space dangling at the end of the line:
    // invisible in a terminal, but noise in `perga FILE | cat`.
    for line in &mut out {
        while line
            .spans
            .last()
            .is_some_and(|s| s.content.ends_with(' ') || s.content.is_empty())
        {
            let last = line.spans.last_mut().expect("checked");
            let trimmed = last.content.trim_end_matches(' ').to_string();
            if trimmed.is_empty() {
                line.spans.pop();
            } else {
                *last = Span::styled(trimmed, last.style);
                break;
            }
        }
    }

    out
}

/// How many bytes of `text` fit in `room` columns, breaking at a space when
/// there is one and mid-word only when a single word is wider than the line.
fn split_at_width(text: &str, room: usize, line_is_empty: bool) -> usize {
    let mut used = 0usize;
    let mut last_space = None;
    let mut fits = 0usize;

    for (index, c) in text.char_indices() {
        let w = c.to_string().width();
        if used + w > room {
            break;
        }
        used += w;
        fits = index + c.len_utf8();
        if c == ' ' {
            last_space = Some(fits);
        }
    }

    if fits == text.len() {
        return fits;
    }

    match last_space {
        Some(at) => at,
        // A word longer than the whole line has to be broken, but only once the
        // line is empty — otherwise it would be broken twice over.
        None if line_is_empty => fits,
        None => 0,
    }
}

/// Hash a block's source text.
fn hash_of(source: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    source.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renderer(width: u16) -> Renderer {
        Renderer::new(&Theme::dark(), Highlighter::new(), width)
    }

    fn text_of(lines: &[Line<'static>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect())
            .collect()
    }

    fn render_all(source: &str, width: u16) -> Vec<String> {
        let document = Document::scratch(source);
        let renderer = renderer(width);
        let mut layout = RenderedDocument::new();
        assert!(layout.resolve_all(&document, &renderer));

        let total = layout.total_lines(&document).unwrap();
        text_of(&layout.window(&document, &renderer, 0, total as u16))
    }

    // -- Wrapping ---------------------------------------------------------

    #[test]
    fn wrapped_lines_have_no_trailing_space() {
        let rendered = render_all("one two three four five six seven eight nine ten\n", 20);
        for line in &rendered {
            assert_eq!(line.trim_end(), line, "{line:?} has a trailing space");
        }
    }

    #[test]
    fn prose_wraps_at_the_viewport_width() {
        let rendered = render_all("one two three four five six seven eight nine ten\n", 20);
        assert!(rendered.iter().all(|l| l.width() <= 20), "{rendered:?}");
        assert!(rendered.len() > 1);
        assert_eq!(rendered.join(" ").split_whitespace().count(), 10);
    }

    #[test]
    fn wrapping_uses_display_columns_not_character_counts() {
        // Each of these is two columns wide, so ten of them fill twenty.
        let rendered = render_all("日本語日本語日本語日本語日本語\n", 20);
        for line in &rendered {
            assert!(line.width() <= 20, "{line:?} is {} columns", line.width());
        }
    }

    #[test]
    fn a_word_longer_than_the_line_is_broken_rather_than_lost() {
        let rendered = render_all("supercalifragilisticexpialidocious\n", 10);
        assert!(rendered.len() > 1);
        assert_eq!(
            rendered.concat().replace(' ', ""),
            "supercalifragilisticexpialidocious"
        );
    }

    #[test]
    fn code_blocks_are_never_wrapped() {
        let long = "x".repeat(200);
        let rendered = render_all(&format!("```\n{long}\n```\n"), 40);
        // The code line, plus the blank that separates the block from the next.
        assert_eq!(rendered.len(), 2);
        assert_eq!(rendered[0].trim_end(), long);
    }

    #[test]
    fn code_fences_are_not_shown() {
        let rendered = render_all("```rust\nfn main() {}\n```\n", 80);
        assert_eq!(rendered.iter().filter(|l| l.contains("```")).count(), 0);
        assert_eq!(rendered.len(), 2);
        assert_eq!(rendered[0].trim_end(), "fn main() {}");
    }

    #[test]
    fn an_indented_code_block_loses_its_indent() {
        let rendered = render_all("    let x = 1;\n", 80);
        assert_eq!(rendered[0].trim_end(), "let x = 1;");
    }

    // -- Adaptations of tui-markdown's output ------------------------------

    #[test]
    fn a_link_shows_its_label_and_not_its_target() {
        let rendered = render_all("See [the setup guide](../setup.md) for more.\n", 80);
        let text = rendered.join(" ");
        assert!(text.contains("the setup guide"), "{text:?}");
        assert!(!text.contains("../setup.md"), "{text:?}");
        assert!(!text.contains(" ("), "{text:?}");
    }

    #[test]
    fn an_image_renders_as_a_placeholder() {
        let rendered = render_all("![a diagram](diagram.png)\n", 80);
        let text = rendered.join(" ");
        assert!(text.contains("[image: a diagram]"), "{text:?}");
        assert!(!text.contains("[img]"), "{text:?}");
    }

    #[test]
    fn frontmatter_is_hidden_from_the_body() {
        let rendered = render_all("---\ntitle: Hidden\n---\n\n# Shown\n", 80);
        let text = rendered.join("\n");
        assert!(!text.contains("Hidden"), "{text:?}");
        assert!(text.contains("Shown"), "{text:?}");
    }

    #[test]
    fn raw_html_is_shown_as_literal_text() {
        let rendered = render_all("<div class=\"x\">raw</div>\n", 80);
        assert!(rendered.join(" ").contains("<div"), "{rendered:?}");
    }

    #[test]
    fn math_delimiters_are_preserved_as_text() {
        let rendered = render_all("An equation $a^2 + b^2$ inline.\n", 80);
        let text = rendered.join(" ");
        assert!(text.contains("a^2 + b^2"), "{text:?}");
    }

    #[test]
    fn task_list_items_render_as_checkboxes() {
        let rendered = render_all("- [x] done\n- [ ] todo\n", 40).join("\n");
        assert!(rendered.contains(TASK_DONE), "{rendered:?}");
        assert!(rendered.contains(TASK_TODO), "{rendered:?}");
        assert!(!rendered.contains("[x]"), "{rendered:?}");
        assert!(!rendered.contains("[ ]"), "{rendered:?}");
    }

    #[test]
    fn a_bracketed_word_in_prose_is_not_mistaken_for_a_task() {
        let rendered = render_all("The array is written [x] in the docs.\n", 60).join(" ");
        assert!(rendered.contains("[x]"), "{rendered:?}");
    }

    #[test]
    fn gfm_features_render() {
        assert!(render_all("- [x] done\n- [ ] todo\n", 40)
            .join(" ")
            .contains("done"));
        assert!(render_all("| a | b |\n|---|---|\n| 1 | 2 |\n", 40)
            .join("\n")
            .contains('│'));
        assert!(render_all("~~struck~~\n", 40).join(" ").contains("struck"));
        assert!(render_all("Text[^1]\n\n[^1]: A note.\n", 40)
            .join(" ")
            .contains("A note"));
    }

    // -- The cache ---------------------------------------------------------

    #[test]
    fn only_visible_blocks_are_rendered() {
        // Section 9.2's acceptance criterion, by way of the render counter.
        let source = (0..500)
            .map(|i| format!("Paragraph {i}.\n"))
            .collect::<Vec<_>>()
            .join("\n");

        let document = Document::scratch(&source);
        assert_eq!(document.blocks.len(), 500);

        let renderer = renderer(80);
        let mut layout = RenderedDocument::new();
        layout.window(&document, &renderer, 0, 20);

        assert!(
            layout.render_count() < 500,
            "rendered {} of 500 blocks for a 20-line window",
            layout.render_count()
        );
    }

    #[test]
    fn editing_one_paragraph_re_renders_only_that_block() {
        // The reason the cache is keyed by content and not by byte range:
        // an edit shifts every following block's range, and a range-keyed
        // cache would miss on all of them.
        let before = (0..200)
            .map(|i| format!("Paragraph {i}."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let after = before.replacen("Paragraph 3.", "Paragraph three, edited.", 1);

        let document = Document::scratch(&before);
        let renderer = renderer(80);
        let mut layout = RenderedDocument::new();
        assert!(layout.resolve_all(&document, &renderer));
        let baseline = layout.render_count();
        assert_eq!(baseline, 200);

        let edited = Document::scratch(&after);
        assert!(layout.resolve_all(&edited, &renderer));

        assert_eq!(
            layout.render_count() - baseline,
            1,
            "an edit to one paragraph re-rendered {} blocks",
            layout.render_count() - baseline
        );
    }

    #[test]
    fn a_resize_re_renders_every_block_exactly_once() {
        let source = (0..50)
            .map(|i| format!("Paragraph number {i} with enough words to wrap somewhere."))
            .collect::<Vec<_>>()
            .join("\n\n");

        let document = Document::scratch(&source);
        let mut layout = RenderedDocument::new();

        let wide = renderer(80);
        assert!(layout.resolve_all(&document, &wide));
        assert_eq!(layout.render_count(), 50);

        let narrow = renderer(40);
        assert!(layout.resolve_all(&document, &narrow));
        assert_eq!(layout.render_count(), 100);

        // Resizing back hits the cache the first width left behind.
        assert!(layout.resolve_all(&document, &wide));
        assert_eq!(layout.render_count(), 100);
    }

    #[test]
    fn a_repeated_window_renders_nothing_new() {
        let source = (0..100)
            .map(|i| format!("Paragraph {i}."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let document = Document::scratch(&source);
        let renderer = renderer(80);
        let mut layout = RenderedDocument::new();

        layout.window(&document, &renderer, 0, 20);
        let first = layout.render_count();
        layout.window(&document, &renderer, 0, 20);
        assert_eq!(layout.render_count(), first);
    }

    // -- Scrolling windows -------------------------------------------------

    #[test]
    fn a_window_returns_exactly_the_lines_asked_for() {
        let source = (0..60)
            .map(|i| format!("Line {i}."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let document = Document::scratch(&source);
        let renderer = renderer(80);
        let mut layout = RenderedDocument::new();

        // Each paragraph is two rendered lines: its text and the blank that
        // separates it from the next block.
        let window = layout.window(&document, &renderer, 10, 5);
        assert_eq!(
            text_of(&window),
            vec!["Line 5.", "", "Line 6.", "", "Line 7."]
        );
    }

    #[test]
    fn a_window_past_the_end_is_short_rather_than_wrong() {
        let document = Document::scratch("one\n\ntwo\n");
        let renderer = renderer(80);
        let mut layout = RenderedDocument::new();

        let window = layout.window(&document, &renderer, 1, 10);
        assert_eq!(text_of(&window), vec!["", "two", ""]);
        assert!(layout.window(&document, &renderer, 50, 10).is_empty());
    }

    #[test]
    fn the_total_is_unknown_until_the_document_is_measured() {
        let source = (0..1000)
            .map(|i| format!("Paragraph {i}."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let document = Document::scratch(&source);
        let renderer = renderer(80);
        let mut layout = RenderedDocument::new();

        layout.window(&document, &renderer, 0, 20);
        assert_eq!(layout.total_lines(&document), None);

        while !layout.resolve_all(&document, &renderer) {}
        assert_eq!(layout.total_lines(&document), Some(2000));
    }

    #[test]
    fn measuring_a_large_document_is_chunked() {
        let source = (0..1000)
            .map(|i| format!("Paragraph {i}."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let document = Document::scratch(&source);
        let renderer = renderer(80);
        let mut layout = RenderedDocument::new();

        // One call does not do all of it, so a frame is never held hostage.
        assert!(!layout.resolve_all(&document, &renderer));
        assert!(layout.render_count() <= RESOLVE_CHUNK);
    }

    // -- The offset to line map -------------------------------------------

    #[test]
    fn a_heading_offset_maps_to_the_line_it_renders_on() {
        let source = "# One\n\nprose\n\n## Two\n\nmore prose\n\n### Three\n";
        let document = Document::scratch(source);
        let renderer = renderer(80);
        let mut layout = RenderedDocument::new();
        assert!(layout.resolve_all(&document, &renderer));

        let rendered = text_of(&layout.window(&document, &renderer, 0, 40));

        for heading in &document.outline {
            let line = layout
                .line_map()
                .line_of_offset(heading.offset)
                .expect("every heading is mapped");
            assert!(
                rendered[line].contains(&heading.text),
                "{:?} maps to line {line}, which is {:?}",
                heading.text,
                rendered[line]
            );
        }
    }

    #[test]
    fn the_map_round_trips_at_block_boundaries() {
        let source = "# One\n\nprose here\n\n## Two\n\nmore\n";
        let document = Document::scratch(source);
        let renderer = renderer(80);
        let mut layout = RenderedDocument::new();
        assert!(layout.resolve_all(&document, &renderer));

        for block in &document.blocks {
            let line = layout.line_map().line_of_offset(block.range.start).unwrap();
            let back = layout.line_map().offset_of_line(line).unwrap();
            assert_eq!(
                back, block.range.start,
                "block {block:?} did not round trip"
            );
        }
    }

    #[test]
    fn the_map_is_exact_inside_a_code_block() {
        // Code renders one line per source line, so every source line has its
        // own entry rather than an interpolated one.
        let source = "```rust\nlet a = 1;\nlet b = 2;\nlet c = 3;\n```\n";
        let document = Document::scratch(source);
        let renderer = renderer(80);
        let mut layout = RenderedDocument::new();
        assert!(layout.resolve_all(&document, &renderer));

        let rendered = text_of(&layout.window(&document, &renderer, 0, 10));
        assert_eq!(rendered.len(), 4);

        for needle in ["let a = 1;", "let b = 2;", "let c = 3;"] {
            let offset = source.find(needle).unwrap();
            let line = layout.line_map().line_of_offset(offset).unwrap();
            assert!(
                rendered[line].contains(needle),
                "{needle:?} maps to line {line}: {:?}",
                rendered[line]
            );
        }
    }

    #[test]
    fn the_map_is_monotonic() {
        let source = "# A\n\none two three\n\n```\ncode\n```\n\n- a\n- b\n\n> quote\n";
        let document = Document::scratch(source);
        let renderer = renderer(20);
        let mut layout = RenderedDocument::new();
        assert!(layout.resolve_all(&document, &renderer));

        let mut previous: Option<(usize, usize)> = None;
        for offset in 0..source.len() {
            let Some(line) = layout.line_map().line_of_offset(offset) else {
                continue;
            };
            if let Some((last_offset, last_line)) = previous {
                assert!(
                    line >= last_line,
                    "offset {offset} maps to line {line}, but {last_offset} mapped to {last_line}"
                );
            }
            previous = Some((offset, line));
        }
    }

    #[test]
    fn an_empty_map_answers_nothing_rather_than_panicking() {
        let map = LineMap::default();
        assert!(map.is_empty());
        assert_eq!(map.len(), 0);
        assert_eq!(map.line_of_offset(0), None);
        assert_eq!(map.offset_of_line(0), None);
    }

    #[test]
    fn line_of_offset_measures_only_as_far_as_it_must() {
        let source = (0..1000)
            .map(|i| format!("Paragraph {i}."))
            .collect::<Vec<_>>()
            .join("\n\n");
        let document = Document::scratch(&source);
        let renderer = renderer(80);
        let mut layout = RenderedDocument::new();

        let offset = document.blocks[5].range.start;
        assert_eq!(
            layout.line_of_offset(&document, &renderer, offset),
            Some(10)
        );
        assert!(layout.render_count() < 100, "{}", layout.render_count());
    }

    // -- Degenerate documents ----------------------------------------------

    #[test]
    fn an_empty_document_renders_nothing_and_does_not_panic() {
        let document = Document::scratch("");
        let renderer = renderer(80);
        let mut layout = RenderedDocument::new();

        assert!(layout.resolve_all(&document, &renderer));
        assert_eq!(layout.total_lines(&document), Some(0));
        assert!(layout.window(&document, &renderer, 0, 10).is_empty());
    }

    #[test]
    fn a_zero_width_viewport_does_not_hang() {
        let document = Document::scratch("some text that would wrap forever\n");
        let renderer = renderer(0);
        let mut layout = RenderedDocument::new();
        assert!(layout.resolve_all(&document, &renderer));
    }

    /// `tui-markdown` passes `---` through as three characters. A rule is a
    /// line, and the theme has had a `rule` key for it since the first
    /// version — unused until this.
    #[test]
    fn a_thematic_break_is_drawn_as_a_line() {
        let document = Document::scratch("before\n\n---\n\nafter\n");
        let renderer = renderer(30);
        let mut layout = RenderedDocument::new();

        let text = text_of(&layout.window(&document, &renderer, 0, 20));

        let rule = text
            .iter()
            .find(|line| line.starts_with('─'))
            .expect("the break is a line of rule glyphs, not `---`");

        assert_eq!(rule.chars().count(), 30, "it spans the width");
        assert!(
            !text.iter().any(|line| line.trim() == "---"),
            "the markup is not shown: {text:?}"
        );
    }

    #[test]
    fn a_thematic_break_takes_the_theme_s_rule_style() {
        let theme = Theme::dark();
        let document = Document::scratch("---\n");
        let renderer = renderer(10);
        let mut layout = RenderedDocument::new();

        let lines = layout.window(&document, &renderer, 0, 5);
        let at = text_of(&lines)
            .iter()
            .position(|line| line.starts_with('─'))
            .expect("a rule");

        assert_eq!(lines[at].spans[0].style.fg, theme.markdown.rule.fg);
    }

    /// A line's style only paints the cells its spans occupy, so a short line
    /// of code left the background ending mid-row and the block read as
    /// ragged text rather than as a block.
    #[test]
    fn a_code_block_paints_its_background_across_the_width() {
        let theme = Theme::dark();
        let document = Document::scratch("```\nx\nlonger line\n```\n");
        let renderer = renderer(24);
        let mut layout = RenderedDocument::new();

        let lines = layout.window(&document, &renderer, 0, 10);

        let text = text_of(&lines);

        for (at, rendered) in text.iter().enumerate() {
            if rendered.trim().is_empty() {
                continue;
            }
            assert_eq!(
                rendered.chars().count(),
                24,
                "every code line reaches the edge: {rendered:?}"
            );
            assert_eq!(
                lines[at].spans.last().unwrap().style.bg,
                theme.markdown.code_block_bg.bg,
                "and the padding carries the block's background"
            );
        }
    }

    /// A line wider than the viewport is left alone: clipping and the `…`
    /// marker are the viewport's job, and padding it would defeat them.
    #[test]
    fn a_code_line_wider_than_the_viewport_is_not_padded() {
        let document = Document::scratch("```\nabcdefghijklmnopqrstuvwxyz\n```\n");
        let renderer = renderer(10);
        let mut layout = RenderedDocument::new();

        let code = text_of(&layout.window(&document, &renderer, 0, 5))
            .into_iter()
            .find(|l| l.starts_with("abc"))
            .expect("the code line");

        assert_eq!(code.chars().count(), 26);
    }

    /// `tui-markdown` passes a quote's `>` through as the character that was
    /// typed. The theme has had a `blockquote_bar` key since the first version
    /// and nothing used it.
    #[test]
    fn a_block_quote_is_drawn_with_a_bar() {
        let text = render_all("> quoted\n", 30);
        let quote = text.iter().find(|l| l.contains("quoted")).expect("a quote");

        assert!(quote.starts_with('│'), "{quote:?}");
        assert!(!quote.contains('>'), "the markup is not shown: {quote:?}");
    }

    /// The bar is what a quote is drawn with instead of a prefix, so it has to
    /// run down every line — putting it back before wrapping would mark only
    /// the first.
    #[test]
    fn the_bar_runs_down_a_wrapped_quote() {
        let source = "> one two three four five six seven eight nine ten\n";
        let text = render_all(source, 20);
        let quoted: Vec<&String> = text.iter().filter(|l| l.starts_with('│')).collect();

        assert!(quoted.len() > 1, "the quote should wrap: {text:?}");
        for line in &quoted {
            assert!(line.starts_with("│ "), "{line:?}");
            assert!(line.chars().count() <= 20, "{line:?}");
        }
    }

    #[test]
    fn a_nested_quote_gets_a_bar_per_level() {
        let text = render_all("> > deep\n", 30);
        let quote = text.iter().find(|l| l.contains("deep")).expect("a quote");

        assert!(quote.starts_with("│ │ "), "{quote:?}");
    }

    #[test]
    fn the_bar_takes_the_theme_s_style() {
        let theme = Theme::dark();
        let document = Document::scratch("> quoted\n");
        let renderer = renderer(30);
        let mut layout = RenderedDocument::new();

        let lines = layout.window(&document, &renderer, 0, 5);
        let at = text_of(&lines)
            .iter()
            .position(|l| l.contains("quoted"))
            .expect("a quote");

        assert_eq!(lines[at].spans[0].content.as_ref(), "│ ");
        assert_eq!(
            lines[at].spans[0].style.fg,
            theme.markdown.blockquote_bar.fg
        );
    }

    /// A GitHub alert is a quote whose first line `tui-markdown` turns into an
    /// icon and a label. It must still get the bar.
    #[test]
    fn an_alert_keeps_the_bar() {
        let text = render_all("> [!NOTE]\n> careful\n", 40);

        assert!(
            text.iter().filter(|l| l.starts_with('│')).count() >= 2,
            "{text:?}"
        );
        assert!(text.iter().any(|l| l.contains("Note")), "{text:?}");
    }

    // -- Heading markers ---------------------------------------------------

    #[test]
    fn a_heading_keeps_its_marker_by_default() {
        let text = render_all("## Kurulum\n", 30);
        assert!(text.iter().any(|l| l.starts_with("## Kurulum")), "{text:?}");
    }

    #[test]
    fn the_marker_can_be_turned_off() {
        let document = Document::scratch("# One\n\n### Three\n\nbody\n");
        let renderer = renderer(30).with_heading_markers(false);
        let mut layout = RenderedDocument::new();

        let text = text_of(&layout.window(&document, &renderer, 0, 20));

        assert!(text.iter().any(|l| l == "One"), "{text:?}");
        assert!(text.iter().any(|l| l == "Three"), "{text:?}");
        assert!(!text.iter().any(|l| l.contains('#')), "{text:?}");
    }

    /// The heading keeps its style; only the marker goes.
    #[test]
    fn a_heading_without_its_marker_is_still_styled() {
        let theme = Theme::dark();
        let document = Document::scratch("# One\n");
        let renderer = renderer(30).with_heading_markers(false);
        let mut layout = RenderedDocument::new();

        let lines = layout.window(&document, &renderer, 0, 5);
        let at = text_of(&lines)
            .iter()
            .position(|l| l.contains("One"))
            .expect("the heading");

        // `tui-markdown` puts a heading's style on the line, not on its
        // spans, so removing the marker span cannot lose it.
        assert_eq!(lines[at].style.fg, theme.markdown.h1.fg);
    }

    /// A paragraph beginning with a `#` that is not a heading — an escaped
    /// one, or a fragment link — must not lose it.
    #[test]
    fn only_a_heading_loses_a_leading_hash() {
        let document = Document::scratch("\\# not a heading\n");
        let renderer = renderer(30).with_heading_markers(false);
        let mut layout = RenderedDocument::new();

        let text = text_of(&layout.window(&document, &renderer, 0, 5));
        assert!(text.iter().any(|l| l.contains('#')), "{text:?}");
    }
}
