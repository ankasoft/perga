//! A loaded document: source text, parsed blocks, links, outline, render cache.
//!
//! Loading a document is deliberately total: a file that is not valid UTF-8, a
//! file with no trailing newline, an empty file, and a file that is nothing but
//! frontmatter all load and render. What varies is whether editing is offered.

use std::collections::BTreeMap;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::doc::outline::{Heading, Slugger};

/// The byte-order mark, which some editors write and which must survive a save.
const BOM: char = '\u{feff}';

/// What kind of content a block holds.
///
/// The distinction that matters most for rendering is [`BlockKind::CodeBlock`]:
/// code is never soft-wrapped, because wrapping it destroys its meaning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BlockKind {
    /// A paragraph of prose.
    Paragraph,
    /// An ATX or setext heading, at this level.
    Heading(u8),
    /// A bullet or ordered list, including any nesting inside it.
    List,
    /// A fenced or indented code block, with the info string's language.
    CodeBlock(Option<String>),
    /// A GFM table.
    Table,
    /// A block quote, including anything nested inside it.
    BlockQuote,
    /// A thematic break.
    Rule,
    /// A block of raw HTML, rendered as literal dimmed text.
    Html,
    /// A footnote definition.
    FootnoteDefinition,
    /// A definition list.
    DefinitionList,
    /// The YAML frontmatter, which is hidden from the rendered body.
    Frontmatter,
}

impl BlockKind {
    /// Whether this block's lines are clipped rather than soft-wrapped.
    pub fn is_clipped(&self) -> bool {
        matches!(self, BlockKind::CodeBlock(_) | BlockKind::Table)
    }
}

/// One top-level block of a document, with the byte range it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// What the block is.
    pub kind: BlockKind,
    /// The block's byte range in [`Document::source`].
    pub range: Range<usize>,
}

impl Block {
    /// The block's source text.
    pub fn source<'a>(&self, document: &'a Document) -> &'a str {
        &document.source[self.range.clone()]
    }
}

/// Whether a document's line endings are `\n` or `\r\n`.
///
/// Recorded at load and restored on save: silently rewriting a colleague's CRLF
/// file to LF produces a diff touching every line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineEnding {
    /// `\n`.
    #[default]
    Lf,
    /// `\r\n`.
    Crlf,
}

impl LineEnding {
    /// Detect the ending a source uses, from the first one it contains.
    pub fn detect(source: &str) -> Self {
        match source.find('\n') {
            Some(0) => LineEnding::Lf,
            Some(i) if source.as_bytes()[i - 1] == b'\r' => LineEnding::Crlf,
            _ => LineEnding::Lf,
        }
    }

    /// The bytes this ending writes.
    pub fn as_str(self) -> &'static str {
        match self {
            LineEnding::Lf => "\n",
            LineEnding::Crlf => "\r\n",
        }
    }
}

/// Why a document cannot be edited, if it cannot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReadOnlyReason {
    /// The file was not valid UTF-8 and was decoded lossily. Saving it back
    /// would silently replace the undecodable bytes with `U+FFFD`.
    NotUtf8,
}

impl ReadOnlyReason {
    /// The message shown in the status bar.
    pub fn message(self) -> &'static str {
        match self {
            ReadOnlyReason::NotUtf8 => "Read-only: file is not valid UTF-8",
        }
    }
}

/// The document's YAML frontmatter, as far as perga reads it.
///
/// Deliberately not a YAML parser. Only top-level `key: value` scalars are
/// read, which is all the tab label needs; anything more structured is left
/// alone and simply hidden from the rendered body.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Frontmatter {
    /// The byte range of the whole frontmatter block, including its fences.
    pub range: Option<Range<usize>>,
    /// The top-level scalar keys.
    pub fields: BTreeMap<String, String>,
}

impl Frontmatter {
    /// The `title` field, used as the tab label when present.
    pub fn title(&self) -> Option<&str> {
        self.fields.get("title").map(String::as_str)
    }
}

/// A loaded document.
#[derive(Debug, Clone)]
pub struct Document {
    /// Where the document came from.
    pub path: PathBuf,
    /// The source text, with any BOM stripped.
    pub source: String,
    /// The file's modification time at load, for conflict detection on save.
    pub mtime: SystemTime,
    /// Whether the file began with a byte-order mark, so a save can restore it.
    pub had_bom: bool,
    /// The line ending to write on save.
    pub line_ending: LineEnding,
    /// Why the document cannot be edited, if it cannot.
    pub read_only: Option<ReadOnlyReason>,
    /// The frontmatter, if the document has any.
    pub frontmatter: Frontmatter,
    /// Top-level blocks, in document order, with their byte ranges.
    pub blocks: Vec<Block>,
    /// Headings, in document order, with unique slugs.
    pub outline: Vec<Heading>,
}

impl Document {
    /// Read and parse a document from disk.
    pub fn load(path: impl Into<PathBuf>) -> std::io::Result<Self> {
        let path = path.into();
        let bytes = std::fs::read(&path)?;
        let mtime = std::fs::metadata(&path)?.modified()?;

        Ok(Document::from_bytes(path, &bytes, mtime))
    }

    /// Parse a document from bytes already read.
    ///
    /// Invalid UTF-8 is decoded lossily rather than refused: showing a file
    /// with a few replacement characters in it is more useful than showing
    /// nothing. Editing is disabled for such a document, because saving the
    /// lossy decode back would destroy the bytes that did not decode.
    pub fn from_bytes(path: PathBuf, bytes: &[u8], mtime: SystemTime) -> Self {
        let (source, read_only) = match std::str::from_utf8(bytes) {
            Ok(text) => (text.to_string(), None),
            Err(_) => (
                String::from_utf8_lossy(bytes).into_owned(),
                Some(ReadOnlyReason::NotUtf8),
            ),
        };

        let had_bom = source.starts_with(BOM);
        let source = if had_bom {
            source[BOM.len_utf8()..].to_string()
        } else {
            source
        };

        Document::from_source(path, source, mtime, had_bom, read_only)
    }

    /// Parse a document from source text.
    pub fn from_source(
        path: PathBuf,
        source: String,
        mtime: SystemTime,
        had_bom: bool,
        read_only: Option<ReadOnlyReason>,
    ) -> Self {
        let line_ending = LineEnding::detect(&source);
        let frontmatter = parse_frontmatter(&source);
        let body_start = frontmatter.range.as_ref().map_or(0, |r| r.end);
        let (blocks, outline) = parse_blocks(&source, body_start);

        Document {
            path,
            source,
            mtime,
            had_bom,
            line_ending,
            read_only,
            frontmatter,
            blocks,
            outline,
        }
    }

    /// A document with no file behind it, for tests and for scratch content.
    #[cfg(test)]
    pub fn scratch(source: &str) -> Self {
        Document::from_source(
            PathBuf::from("test.md"),
            source.to_string(),
            SystemTime::UNIX_EPOCH,
            false,
            None,
        )
    }

    /// Whether edit mode may be entered.
    pub fn is_editable(&self) -> bool {
        self.read_only.is_none()
    }

    /// The label for this document's tab: the frontmatter title if it has one,
    /// otherwise the file stem.
    pub fn label(&self) -> &str {
        self.frontmatter
            .title()
            .or_else(|| self.path.file_stem().and_then(|s| s.to_str()))
            .unwrap_or("untitled")
    }

    /// The heading with this slug, if the document has one.
    pub fn heading(&self, slug: &str) -> Option<&Heading> {
        self.outline.iter().find(|h| h.slug == slug)
    }

    /// The path relative to `root`, for the title bar, falling back to the full
    /// path for a document outside the vault.
    pub fn display_path(&self, root: &Path) -> String {
        self.path
            .strip_prefix(root)
            .unwrap_or(&self.path)
            .display()
            .to_string()
    }
}

/// The parser options perga parses with.
///
/// GFM throughout: tables, footnotes, strikethrough, and task lists, plus the
/// metadata block so frontmatter does not leak into the body as a thematic
/// break followed by a paragraph.
pub fn parser_options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_GFM
        | Options::ENABLE_YAML_STYLE_METADATA_BLOCKS
        | Options::ENABLE_MATH
        | Options::ENABLE_DEFINITION_LIST
}

/// Extract a leading `---` fenced YAML block.
///
/// Line-oriented and dependency-free by design. `serde_yaml` is archived and
/// unmaintained, and nothing in v1 needs more than the top-level scalars — see
/// `docs/decisions.md`.
fn parse_frontmatter(source: &str) -> Frontmatter {
    let mut frontmatter = Frontmatter::default();

    // The opening fence must be the very first line.
    let Some(rest) = source.strip_prefix("---") else {
        return frontmatter;
    };
    let Some(rest) = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))
    else {
        return frontmatter;
    };

    let body_start = source.len() - rest.len();
    let mut offset = body_start;

    for line in rest.split_inclusive('\n') {
        let trimmed = line.trim_end_matches(['\n', '\r']);

        if trimmed == "---" || trimmed == "..." {
            frontmatter.range = Some(0..offset + line.len());
            frontmatter.fields = parse_scalar_fields(&source[body_start..offset]);
            return frontmatter;
        }

        offset += line.len();
    }

    // An unterminated block is not frontmatter; it is a document that happens
    // to start with a thematic break.
    frontmatter
}

/// Read top-level `key: value` scalars out of a frontmatter body.
fn parse_scalar_fields(body: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();

    for line in body.lines() {
        // Only top level: an indented line belongs to a nested structure this
        // extractor deliberately does not understand.
        if line.starts_with([' ', '\t']) || line.trim_start().starts_with('#') {
            continue;
        }

        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() {
            continue;
        }

        let value = value.trim();
        // Strip one layer of matching quotes, which is how most titles are
        // written when they contain a colon.
        let value = value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value);

        if value.is_empty() {
            continue;
        }

        fields.insert(key.to_string(), value.to_string());
    }

    fields
}

/// Group the event stream into top-level blocks with their byte ranges.
///
/// `into_offset_iter` is what makes this possible: every event carries the
/// source range it came from, so a block's range is its `Start` event's range
/// and nothing has to be reconstructed by counting.
fn parse_blocks(source: &str, body_start: usize) -> (Vec<Block>, Vec<Heading>) {
    let mut blocks = Vec::new();
    let mut headings = Vec::new();
    let mut slugger = Slugger::new();

    if body_start > 0 {
        blocks.push(Block {
            kind: BlockKind::Frontmatter,
            range: 0..body_start,
        });
    }

    let body = &source[body_start..];
    let parser = Parser::new_ext(body, parser_options()).into_offset_iter();

    let mut depth = 0usize;
    // Set while inside a heading, so its text can be accumulated for the slug.
    let mut heading: Option<(u8, usize, String)> = None;

    for (event, range) in parser {
        let range = (range.start + body_start)..(range.end + body_start);

        match &event {
            Event::Start(tag) => {
                if depth == 0 {
                    if let Some(kind) = block_kind(tag) {
                        blocks.push(Block {
                            kind: kind.clone(),
                            range: range.clone(),
                        });
                        if let BlockKind::Heading(level) = kind {
                            heading = Some((level, range.start, String::new()));
                        }
                    }
                }
                depth += 1;
            }
            Event::End(end) => {
                depth = depth.saturating_sub(1);
                if depth == 0 && matches!(end, TagEnd::Heading(_)) {
                    if let Some((level, offset, text)) = heading.take() {
                        let text = text.trim().to_string();
                        headings.push(Heading {
                            level,
                            slug: slugger.slug(&text),
                            text,
                            offset,
                        });
                    }
                }
            }
            Event::Rule if depth == 0 => blocks.push(Block {
                kind: BlockKind::Rule,
                range: range.clone(),
            }),
            Event::Html(_) if depth == 0 => blocks.push(Block {
                kind: BlockKind::Html,
                range: range.clone(),
            }),
            _ => {}
        }

        // A heading's slug comes from its rendered text, not its source, so
        // that `## The *hard* way` anchors as `the-hard-way`.
        if let Some((_, _, text)) = &mut heading {
            match &event {
                Event::Text(t) | Event::Code(t) => text.push_str(t),
                Event::SoftBreak | Event::HardBreak => text.push(' '),
                _ => {}
            }
        }
    }

    (blocks, headings)
}

/// The block kind for a top-level tag, or `None` for tags that never appear at
/// the top level.
fn block_kind(tag: &Tag) -> Option<BlockKind> {
    Some(match tag {
        Tag::Paragraph => BlockKind::Paragraph,
        Tag::Heading { level, .. } => BlockKind::Heading(*level as u8),
        Tag::List(_) => BlockKind::List,
        Tag::CodeBlock(kind) => {
            let language = match kind {
                pulldown_cmark::CodeBlockKind::Fenced(info) => {
                    // The info string may carry more than a language, as in
                    // ```rust,ignore.
                    let language = info.split([' ', ',']).next().unwrap_or("").trim();
                    (!language.is_empty()).then(|| language.to_ascii_lowercase())
                }
                pulldown_cmark::CodeBlockKind::Indented => None,
            };
            BlockKind::CodeBlock(language)
        }
        Tag::Table(_) => BlockKind::Table,
        Tag::BlockQuote(_) => BlockKind::BlockQuote,
        Tag::HtmlBlock => BlockKind::Html,
        Tag::FootnoteDefinition(_) => BlockKind::FootnoteDefinition,
        Tag::DefinitionList => BlockKind::DefinitionList,
        Tag::MetadataBlock(_) => BlockKind::Frontmatter,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(source: &str) -> Vec<BlockKind> {
        Document::scratch(source)
            .blocks
            .into_iter()
            .map(|b| b.kind)
            .collect()
    }

    #[test]
    fn splits_a_document_into_top_level_blocks() {
        let source = "# Title\n\nSome prose.\n\n- a\n- b\n\n```rust\nfn main() {}\n```\n\n---\n";
        assert_eq!(
            kinds(source),
            vec![
                BlockKind::Heading(1),
                BlockKind::Paragraph,
                BlockKind::List,
                BlockKind::CodeBlock(Some("rust".to_string())),
                BlockKind::Rule,
            ]
        );
    }

    #[test]
    fn nested_content_stays_inside_one_block() {
        // A list containing a nested list and a code block is still one block:
        // splitting it would break the rendering of the list markers.
        let source = "- outer\n  - inner\n    ```\n    code\n    ```\n- second\n";
        assert_eq!(kinds(source), vec![BlockKind::List]);
    }

    #[test]
    fn a_blockquote_containing_a_list_is_one_block() {
        let source = "> quoted\n>\n> - a\n> - b\n";
        assert_eq!(kinds(source), vec![BlockKind::BlockQuote]);
    }

    #[test]
    fn every_block_range_maps_back_to_its_source() {
        let source = "# Title\n\nProse here.\n\n```sh\nls -la\n```\n";
        let document = Document::scratch(source);

        let texts: Vec<&str> = document
            .blocks
            .iter()
            .map(|b| b.source(&document))
            .collect();

        assert_eq!(texts[0].trim(), "# Title");
        assert_eq!(texts[1].trim(), "Prose here.");
        assert!(texts[2].contains("ls -la"));
    }

    #[test]
    fn block_ranges_are_ordered_and_do_not_overlap() {
        let source = "# A\n\ntext\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n> quote\n";
        let document = Document::scratch(source);

        for pair in document.blocks.windows(2) {
            assert!(
                pair[0].range.end <= pair[1].range.start,
                "{:?} overlaps {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn gfm_features_are_enabled() {
        assert_eq!(
            kinds("| a | b |\n|---|---|\n| 1 | 2 |\n"),
            vec![BlockKind::Table]
        );
        assert_eq!(
            kinds("Text[^1]\n\n[^1]: The note.\n"),
            vec![BlockKind::Paragraph, BlockKind::FootnoteDefinition]
        );
        // Task lists and strikethrough are inline, so they only have to parse.
        assert_eq!(kinds("- [x] done\n- [ ] todo\n"), vec![BlockKind::List]);
        assert_eq!(kinds("~~struck~~\n"), vec![BlockKind::Paragraph]);
    }

    #[test]
    fn the_code_block_language_comes_from_the_info_string() {
        assert_eq!(
            kinds("```Rust,ignore\nfn main() {}\n```\n"),
            vec![BlockKind::CodeBlock(Some("rust".to_string()))]
        );
        assert_eq!(kinds("```\nplain\n```\n"), vec![BlockKind::CodeBlock(None)]);
        assert_eq!(kinds("    indented\n"), vec![BlockKind::CodeBlock(None)]);
    }

    #[test]
    fn code_blocks_and_tables_are_clipped_not_wrapped() {
        assert!(BlockKind::CodeBlock(None).is_clipped());
        assert!(BlockKind::Table.is_clipped());
        assert!(!BlockKind::Paragraph.is_clipped());
    }

    #[test]
    fn headings_get_unique_slugs_in_document_order() {
        let source = "# Setup\n\n## Setup\n\n### The *hard* way\n";
        let document = Document::scratch(source);

        let outline: Vec<_> = document
            .outline
            .iter()
            .map(|h| (h.level, h.text.as_str(), h.slug.as_str()))
            .collect();

        assert_eq!(
            outline,
            vec![
                (1, "Setup", "setup"),
                (2, "Setup", "setup-1"),
                (3, "The hard way", "the-hard-way"),
            ]
        );
    }

    #[test]
    fn a_heading_offset_points_at_its_source() {
        let document = Document::scratch("intro\n\n## Later\n");
        let heading = &document.outline[0];
        assert!(document.source[heading.offset..].starts_with("## Later"));
    }

    #[test]
    fn frontmatter_is_extracted_and_hidden_from_the_body() {
        let source = "---\ntitle: My Note\ntags: a, b\n---\n\n# Body\n";
        let document = Document::scratch(source);

        assert_eq!(document.frontmatter.title(), Some("My Note"));
        assert_eq!(
            document.frontmatter.fields.get("tags").map(String::as_str),
            Some("a, b")
        );
        assert_eq!(document.blocks[0].kind, BlockKind::Frontmatter);
        assert_eq!(document.blocks[1].kind, BlockKind::Heading(1));
        // Exactly one frontmatter block: the parser must not emit a second.
        assert_eq!(
            document
                .blocks
                .iter()
                .filter(|b| b.kind == BlockKind::Frontmatter)
                .count(),
            1
        );
    }

    #[test]
    fn a_quoted_frontmatter_title_is_unquoted() {
        let document = Document::scratch("---\ntitle: \"A: colon\"\n---\n\nbody\n");
        assert_eq!(document.frontmatter.title(), Some("A: colon"));
    }

    #[test]
    fn nested_frontmatter_keys_are_ignored_not_misread() {
        let source = "---\ntitle: Top\nnested:\n  key: value\n---\n\nbody\n";
        let document = Document::scratch(source);
        assert_eq!(document.frontmatter.title(), Some("Top"));
        assert!(!document.frontmatter.fields.contains_key("key"));
    }

    #[test]
    fn a_leading_rule_is_not_mistaken_for_frontmatter() {
        let document = Document::scratch("---\n\nJust a rule above.\n");
        assert_eq!(document.frontmatter.range, None);
        assert!(document.frontmatter.fields.is_empty());
    }

    #[test]
    fn a_document_that_is_only_frontmatter_still_loads() {
        let document = Document::scratch("---\ntitle: Nothing else\n---\n");
        assert_eq!(document.frontmatter.title(), Some("Nothing else"));
        assert_eq!(document.blocks.len(), 1);
        assert_eq!(document.blocks[0].kind, BlockKind::Frontmatter);
    }

    #[test]
    fn an_empty_document_loads() {
        let document = Document::scratch("");
        assert!(document.blocks.is_empty());
        assert!(document.outline.is_empty());
        assert!(document.is_editable());
    }

    #[test]
    fn a_document_with_no_trailing_newline_loads() {
        let document = Document::scratch("# Title");
        assert_eq!(document.blocks.len(), 1);
        assert_eq!(document.outline[0].slug, "title");
    }

    #[test]
    fn line_endings_are_detected() {
        assert_eq!(LineEnding::detect("a\nb\n"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("a\r\nb\r\n"), LineEnding::Crlf);
        assert_eq!(LineEnding::detect("no newline"), LineEnding::Lf);
        assert_eq!(LineEnding::detect("\nleading"), LineEnding::Lf);
    }

    #[test]
    fn crlf_frontmatter_is_still_recognised() {
        let document = Document::scratch("---\r\ntitle: Windows\r\n---\r\n\r\nbody\r\n");
        assert_eq!(document.frontmatter.title(), Some("Windows"));
        assert_eq!(document.line_ending, LineEnding::Crlf);
    }

    #[test]
    fn invalid_utf8_loads_lossily_and_disables_editing() {
        let bytes = b"# Title\n\ncaf\xff\n";
        let document = Document::from_bytes(PathBuf::from("bad.md"), bytes, SystemTime::UNIX_EPOCH);

        assert_eq!(document.read_only, Some(ReadOnlyReason::NotUtf8));
        assert!(!document.is_editable());
        assert!(document.source.contains('\u{fffd}'));
        assert_eq!(document.outline[0].slug, "title");
    }

    #[test]
    fn a_byte_order_mark_is_stripped_and_remembered() {
        let mut bytes = "\u{feff}# Title\n".as_bytes().to_vec();
        let document =
            Document::from_bytes(PathBuf::from("bom.md"), &bytes, SystemTime::UNIX_EPOCH);

        assert!(document.had_bom);
        assert!(document.source.starts_with("# Title"));
        assert_eq!(document.blocks[0].kind, BlockKind::Heading(1));

        bytes.clear();
        let plain = Document::from_bytes(
            PathBuf::from("plain.md"),
            b"# Title\n",
            SystemTime::UNIX_EPOCH,
        );
        assert!(!plain.had_bom);
    }

    #[test]
    fn the_tab_label_prefers_the_frontmatter_title() {
        let with_title = Document::from_source(
            PathBuf::from("/vault/notes/raw-name.md"),
            "---\ntitle: Nice Title\n---\n".to_string(),
            SystemTime::UNIX_EPOCH,
            false,
            None,
        );
        assert_eq!(with_title.label(), "Nice Title");

        let without = Document::from_source(
            PathBuf::from("/vault/notes/raw-name.md"),
            "# Heading\n".to_string(),
            SystemTime::UNIX_EPOCH,
            false,
            None,
        );
        assert_eq!(without.label(), "raw-name");
    }

    #[test]
    fn display_path_is_relative_to_the_vault_root() {
        let document = Document::from_source(
            PathBuf::from("/vault/docs/api/auth.md"),
            String::new(),
            SystemTime::UNIX_EPOCH,
            false,
            None,
        );
        assert_eq!(
            document.display_path(Path::new("/vault")),
            "docs/api/auth.md"
        );
        // A document outside the vault keeps its full path rather than
        // producing a misleading relative one.
        assert_eq!(
            document.display_path(Path::new("/elsewhere")),
            "/vault/docs/api/auth.md"
        );
    }
}
