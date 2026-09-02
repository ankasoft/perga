//! Inline link extraction and target resolution.
//!
//! Extraction runs once per parse, alongside the block list, so a link carries
//! the byte range it came from and can be placed on screen through the
//! offset↔line map. Resolution is a pure function of the target string, the
//! directory the containing document lives in, and the vault root — it never
//! reads application state, which is what makes it testable against the awkward
//! cases in Section 15.1.
//!
//! Link targets are untrusted input: they arrive in whatever Markdown the user
//! opened. Nothing here executes anything, and the external opener in
//! [`crate::vault::open_external`] passes the URL as a single `argv` element
//! rather than through a shell.

use std::ops::Range;
use std::path::{Component, Path, PathBuf};

use pulldown_cmark::{Event, LinkType, Parser, Tag, TagEnd};

use crate::config::schema::FilesConfig;
use crate::doc::document::parser_options;

/// What kind of link this is in the source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    /// `[text](target)` or a reference link.
    Inline,
    /// `<https://example.com>` or a bare URL GFM turned into a link.
    Autolink,
    /// `[[Page Name]]`, in any of its four spellings.
    ///
    /// Not Markdown: `pulldown-cmark` sees `[[Page]]` as a paragraph
    /// containing brackets, so these are scanned for separately.
    Wiki,
}

/// One link, as written in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// The text the reader sees.
    pub text: String,
    /// The target exactly as written, before any decoding.
    pub target: String,
    /// The byte range of the whole link in the document source.
    pub range: Range<usize>,
    /// What kind of link it is.
    pub kind: LinkKind,
}

/// What a link target turned out to point at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolved {
    /// A Markdown document, optionally at a heading.
    Document {
        /// The absolute path to the document.
        path: PathBuf,
        /// The heading slug from the fragment, if there was one.
        anchor: Option<String>,
    },
    /// A heading in the document the link is written in.
    Anchor {
        /// The heading slug.
        slug: String,
    },
    /// A directory, which is revealed in the tree rather than opened.
    Directory {
        /// The absolute path to the directory.
        path: PathBuf,
    },
    /// A URL, handed to the desktop opener.
    External {
        /// The URL as written.
        url: String,
    },
    /// A file perga does not render, handed to the desktop opener.
    Other {
        /// The absolute path to the file.
        path: PathBuf,
    },
    /// Nothing could be resolved. Never an error, never a created file.
    Broken {
        /// The target as written, for the status bar.
        target: String,
    },
}

/// Extract every link in a document, in reading order.
///
/// Two passes over the same source: `pulldown-cmark` for the Markdown links,
/// and a scanner for the wiki-links it does not know about. The first pass also
/// collects the ranges that hold code, which is what keeps a `[[Page]]` written
/// inside a fenced block from being turned into a link.
pub fn extract(source: &str) -> Vec<Link> {
    let (mut links, verbatim) = extract_markdown(source);

    links.extend(extract_wiki(source, &verbatim));
    links.sort_by_key(|link| link.range.start);
    links
}

/// The Markdown links, and the ranges that must not be scanned for wiki-links.
fn extract_markdown(source: &str) -> (Vec<Link>, Vec<Range<usize>>) {
    let mut links = Vec::new();
    let mut verbatim = Vec::new();
    let mut open: Option<(Range<usize>, String, LinkKind, String)> = None;

    for (event, range) in Parser::new_ext(source, parser_options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) => {
                let kind = match link_type {
                    LinkType::Autolink | LinkType::Email => LinkKind::Autolink,
                    _ => LinkKind::Inline,
                };
                open = Some((range, dest_url.into_string(), kind, String::new()));
            }
            Event::End(TagEnd::Link) => {
                if let Some((range, target, kind, text)) = open.take() {
                    links.push(Link {
                        // A link with no text of its own — an image link, say —
                        // shows its target, which is better than a blank row in
                        // hint mode.
                        text: if text.trim().is_empty() {
                            target.clone()
                        } else {
                            text
                        },
                        target,
                        range,
                        kind,
                    });
                }
            }
            Event::Code(text) => {
                verbatim.push(range);
                if let Some((_, _, _, collected)) = &mut open {
                    collected.push_str(&text);
                }
            }
            Event::Text(text) => {
                if let Some((_, _, _, collected)) = &mut open {
                    collected.push_str(&text);
                }
            }
            // A `[[Page]]` in a fenced block, in raw HTML, or in the
            // frontmatter is text, not a link.
            Event::Start(Tag::CodeBlock(_)) | Event::Start(Tag::MetadataBlock(_)) => {
                verbatim.push(range);
            }
            Event::Html(_) | Event::InlineHtml(_) => verbatim.push(range),
            _ => {}
        }
    }

    (links, verbatim)
}

/// Scan for `[[Page Name]]` in all four of its spellings.
///
/// A scanner rather than a parser extension: the syntax is not Markdown, and
/// the alternative — post-processing `pulldown-cmark`'s events back into the
/// bracket pairs it split apart — reconstructs less reliably than reading the
/// source does.
fn extract_wiki(source: &str, verbatim: &[Range<usize>]) -> Vec<Link> {
    let bytes = source.as_bytes();
    let mut links = Vec::new();
    let mut at = 0usize;

    while let Some(found) = source[at..].find("[[") {
        let start = at + found;
        at = start + 2;

        let Some(end) = source[at..].find("]]") else {
            break;
        };
        let end = at + end;
        let inner = &source[at..end];
        at = end + 2;

        if verbatim.iter().any(|range| range.contains(&start)) {
            continue;
        }

        // A newline inside the brackets means they were never a link; two
        // paragraphs apart with stray brackets is more likely than a wiki-link
        // spanning a blank line.
        if inner.contains('\n') || inner.trim().is_empty() {
            continue;
        }

        // `![[Page]]` is an embed in some tools. perga does not embed, and a
        // link to the page is the honest fallback.
        let range = if start > 0 && bytes[start - 1] == b'!' {
            start - 1..at
        } else {
            start..at
        };

        let (target, display) = split_wiki(inner);
        links.push(Link {
            text: display,
            target,
            range,
            kind: LinkKind::Wiki,
        });
    }

    links
}

/// Split `Page#Heading|Display` into its target and its display text.
///
/// The pipe comes last so that a display text containing a `#` is not mistaken
/// for a heading, which is the way both Obsidian and MediaWiki read it.
fn split_wiki(inner: &str) -> (String, String) {
    let (target, display) = match inner.split_once('|') {
        Some((target, display)) => (target.trim(), Some(display.trim())),
        None => (inner.trim(), None),
    };

    let shown = display
        .filter(|d| !d.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            // Without a display text the reader sees the page name, and a
            // heading fragment is part of what they are being sent to.
            target.replace('#', " § ")
        });

    (target.to_string(), shown)
}

/// Resolve a link target.
///
/// `document_dir` is the directory of the document the link is written in —
/// relative targets resolve against it, not against the vault root, which is
/// what a Markdown file's own links mean everywhere else.
pub fn resolve(
    target: &str,
    document_dir: &Path,
    vault_root: &Path,
    files: &FilesConfig,
) -> Resolved {
    let raw = target.trim();

    if raw.is_empty() {
        return broken(target);
    }

    if is_external(raw) {
        return Resolved::External {
            url: raw.to_string(),
        };
    }

    let (path_part, fragment) = split_fragment(raw);

    // A bare `#section` stays in the document it was written in.
    if path_part.is_empty() {
        return match fragment {
            Some(slug) if !slug.is_empty() => Resolved::Anchor { slug },
            _ => broken(target),
        };
    }

    // A Markdown file written on Windows has backslashes in its relative
    // targets. They mean the same thing and cost one substitution to accept.
    let path_part = path_part.replace('\\', "/");
    let decoded = percent_decode(&path_part);
    let relative = Path::new(&decoded);

    // A leading `/` is ambiguous: a vault written for a static site generator
    // means "from the vault root", and a document written for a filesystem
    // means the filesystem root. The vault is tried first because that is what
    // the target usually means inside a vault, and the filesystem root is the
    // fallback rather than the other way round.
    let candidates: Vec<PathBuf> = if let Ok(rooted) = relative.strip_prefix("/") {
        vec![normalise(&vault_root.join(rooted)), relative.to_path_buf()]
    } else {
        vec![normalise(&document_dir.join(relative))]
    };

    for candidate in candidates {
        let Ok(metadata) = std::fs::metadata(&candidate) else {
            continue;
        };

        if metadata.is_dir() {
            return Resolved::Directory { path: candidate };
        }

        return if files.is_markdown(&candidate) {
            Resolved::Document {
                path: candidate,
                anchor: fragment.filter(|f| !f.is_empty()),
            }
        } else {
            Resolved::Other { path: candidate }
        };
    }

    broken(target)
}

/// A broken link, reported with the target exactly as the author wrote it.
fn broken(target: &str) -> Resolved {
    Resolved::Broken {
        target: target.trim().to_string(),
    }
}

/// Whether a target names a scheme rather than a path.
///
/// A single-letter scheme is a Windows drive (`C:\notes`), not a URL, so it is
/// deliberately not treated as external.
pub fn is_external(target: &str) -> bool {
    let Some(colon) = target.find(':') else {
        return false;
    };

    let scheme = &target[..colon];
    if scheme.len() < 2 {
        return false;
    }

    let mut chars = scheme.chars();
    chars.next().is_some_and(|c| c.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
}

/// Split a target into its path and its `#fragment`.
fn split_fragment(target: &str) -> (&str, Option<String>) {
    match target.find('#') {
        Some(at) => (
            &target[..at],
            Some(percent_decode(&target[at + 1..]).to_lowercase()),
        ),
        None => (target, None),
    }
}

/// Decode `%20`-style escapes, leaving anything malformed as written.
fn percent_decode(text: &str) -> String {
    if !text.contains('%') {
        return text.to_string();
    }

    let bytes = text.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }

    // A percent escape can encode any byte, and a document can be wrong about
    // what it encoded; a lossy decode is better than refusing the link.
    String::from_utf8_lossy(&out).into_owned()
}

/// Collapse `.` and `..` lexically.
///
/// Not `canonicalize`: the target may not exist, and resolving symlinks would
/// take a link through a symlinked notes directory somewhere the author did not
/// write.
pub fn normalise(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                // A `..` that would climb past the root is dropped rather than
                // kept: `/..` is `/`, and a relative path cannot be made to
                // escape into nonsense.
                if !out.pop() {
                    out.push(Component::ParentDir);
                }
            }
            other => out.push(other),
        }
    }

    if out.as_os_str().is_empty() {
        out.push(Component::CurDir);
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The committed fixture vault, which the resolution tests resolve against.
    fn vault() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault")
    }

    fn resolve_from(dir: &str, target: &str) -> Resolved {
        resolve(
            target,
            &vault().join(dir),
            &vault(),
            &FilesConfig::default(),
        )
    }

    // -- Extraction --------------------------------------------------------

    #[test]
    fn links_come_out_in_reading_order_with_their_text() {
        let links = extract("See [one](a.md) and then [two](b.md).");

        assert_eq!(links.len(), 2);
        assert_eq!(links[0].text, "one");
        assert_eq!(links[0].target, "a.md");
        assert_eq!(links[1].text, "two");
        assert_eq!(links[1].kind, LinkKind::Inline);
    }

    #[test]
    fn the_range_covers_the_whole_link() {
        let source = "before [text](target.md) after";
        let links = extract(source);
        assert_eq!(&source[links[0].range.clone()], "[text](target.md)");
    }

    #[test]
    fn markup_inside_the_text_is_flattened() {
        let links = extract("[a **bold** `word`](x.md)");
        assert_eq!(links[0].text, "a bold word");
    }

    #[test]
    fn autolinks_are_marked_as_such() {
        let links = extract("<https://example.com> and <a@example.com>");
        assert_eq!(links.len(), 2);
        assert!(links.iter().all(|l| l.kind == LinkKind::Autolink));
        assert_eq!(links[1].target, "a@example.com");
    }

    #[test]
    fn a_reference_link_resolves_to_its_definition() {
        let links = extract("[text][ref]\n\n[ref]: target.md\n");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "target.md");
    }

    #[test]
    fn an_image_link_shows_the_alt_text() {
        let links = extract("[![alt](img.png)](target.md)");
        assert_eq!(links[0].text, "alt");
        assert_eq!(links[0].target, "target.md");
    }

    #[test]
    fn a_link_with_no_text_at_all_falls_back_to_its_target() {
        let links = extract("[](target.md)");
        assert_eq!(links[0].text, "target.md");
    }

    #[test]
    fn a_document_with_no_links_produces_none() {
        assert!(extract("# Heading\n\nJust prose.\n").is_empty());
    }

    // -- Wiki-links --------------------------------------------------------

    #[test]
    fn all_four_wiki_spellings_are_recognised() {
        let links = extract(
            "[[Page Name]], [[Page Name|Display Text]], [[Page#Heading]], and \
             [[folder/Page Name]].",
        );

        assert_eq!(links.len(), 4);
        assert!(links.iter().all(|l| l.kind == LinkKind::Wiki));

        assert_eq!(links[0].target, "Page Name");
        assert_eq!(links[0].text, "Page Name");

        assert_eq!(links[1].target, "Page Name");
        assert_eq!(links[1].text, "Display Text");

        assert_eq!(links[2].target, "Page#Heading");
        assert_eq!(links[2].text, "Page § Heading");

        assert_eq!(links[3].target, "folder/Page Name");
    }

    #[test]
    fn a_display_text_containing_a_hash_is_not_a_heading() {
        let links = extract("[[Page|Issue #1]]");
        assert_eq!(links[0].target, "Page");
        assert_eq!(links[0].text, "Issue #1");
    }

    #[test]
    fn a_wiki_link_in_code_is_left_as_text() {
        let inline = extract("Write `[[Page]]` to link.");
        assert!(inline.is_empty(), "{inline:?}");

        let fenced = extract("```\n[[Page]]\n```\n");
        assert!(fenced.is_empty(), "{fenced:?}");
    }

    #[test]
    fn an_embed_is_treated_as_a_link_to_the_page() {
        let links = extract("![[Diagram]]");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "Diagram");
        assert_eq!(&"![[Diagram]]"[links[0].range.clone()], "![[Diagram]]");
    }

    #[test]
    fn unclosed_and_empty_brackets_are_not_links() {
        assert!(extract("[[unclosed").is_empty());
        assert!(extract("[[]]").is_empty());
        assert!(extract("[[   ]]").is_empty());
        assert!(extract("[[spans\na newline]]").is_empty());
    }

    #[test]
    fn wiki_and_inline_links_come_back_in_one_reading_order() {
        let links = extract("[first](a.md) then [[Second]] then [third](c.md)");

        assert_eq!(links.len(), 3);
        assert_eq!(links[0].target, "a.md");
        assert_eq!(links[1].target, "Second");
        assert_eq!(links[2].target, "c.md");
    }

    // -- Resolution --------------------------------------------------------

    #[test]
    fn a_relative_target_resolves_against_the_documents_own_directory() {
        assert_eq!(
            resolve_from("docs/api", "errors.md"),
            Resolved::Document {
                path: vault().join("docs/api/errors.md"),
                anchor: None,
            }
        );
    }

    /// The acceptance criterion in Section 9.5: two directories up and one
    /// down.
    #[test]
    fn a_target_two_directories_up_and_one_down_resolves() {
        assert_eq!(
            resolve_from("docs/api", "../../docs/guides/setup.md"),
            Resolved::Document {
                path: vault().join("docs/guides/setup.md"),
                anchor: None,
            }
        );
    }

    #[test]
    fn an_anchor_is_carried_through_and_lower_cased() {
        assert_eq!(
            resolve_from("docs/api", "auth.md#Obtaining-A-Token"),
            Resolved::Document {
                path: vault().join("docs/api/auth.md"),
                anchor: Some("obtaining-a-token".to_string()),
            }
        );
    }

    #[test]
    fn a_bare_fragment_stays_in_the_current_document() {
        assert_eq!(
            resolve_from("docs/api", "#obtaining-a-token"),
            Resolved::Anchor {
                slug: "obtaining-a-token".to_string(),
            }
        );
    }

    #[test]
    fn a_leading_slash_is_tried_against_the_vault_root() {
        assert_eq!(
            resolve_from("docs/api", "/README.md"),
            Resolved::Document {
                path: vault().join("README.md"),
                anchor: None,
            }
        );
    }

    #[test]
    fn a_directory_target_is_reported_as_a_directory() {
        assert_eq!(
            resolve_from("", "docs/api"),
            Resolved::Directory {
                path: vault().join("docs/api"),
            }
        );
    }

    #[test]
    fn a_non_markdown_target_is_reported_separately() {
        assert_eq!(
            resolve_from("", ".gitignore"),
            Resolved::Other {
                path: vault().join(".gitignore"),
            }
        );
    }

    #[test]
    fn url_encoded_names_are_decoded_before_they_are_looked_up() {
        assert_eq!(
            resolve_from("", "spaces%20and%20%23hash/awkward%20ışık%20%231.md"),
            Resolved::Document {
                path: vault().join("spaces and #hash/awkward ışık #1.md"),
                anchor: None,
            }
        );
    }

    #[test]
    fn windows_separators_are_accepted() {
        assert_eq!(
            resolve_from("docs", "api\\auth.md"),
            Resolved::Document {
                path: vault().join("docs/api/auth.md"),
                anchor: None,
            }
        );
    }

    #[test]
    fn external_schemes_are_recognised_and_not_touched() {
        for url in [
            "https://example.com/a?b=c#d",
            "http://example.com",
            "mailto:someone@example.com",
        ] {
            assert_eq!(
                resolve_from("", url),
                Resolved::External {
                    url: url.to_string()
                }
            );
        }
    }

    #[test]
    fn a_windows_drive_is_not_mistaken_for_a_url_scheme() {
        assert!(!is_external("C:/notes/file.md"));
        assert!(is_external("https://example.com"));
        assert!(!is_external("./relative.md"));
        assert!(!is_external("no-colon-here"));
    }

    #[test]
    fn an_unresolvable_target_is_broken_and_nothing_is_created() {
        let missing = vault().join("docs/nowhere.md");
        assert_eq!(
            resolve_from("docs", "nowhere.md"),
            Resolved::Broken {
                target: "nowhere.md".to_string(),
            }
        );
        assert!(!missing.exists(), "resolution must never create a file");
    }

    #[test]
    fn an_empty_target_is_broken() {
        assert_eq!(
            resolve_from("", "   "),
            Resolved::Broken {
                target: String::new(),
            }
        );
    }

    #[test]
    fn a_target_outside_the_vault_still_resolves() {
        // The repository's own README, three directories above the vault.
        let outside = resolve(
            "../../../../README.md",
            &vault().join("docs"),
            &vault(),
            &FilesConfig::default(),
        );
        assert_eq!(
            outside,
            Resolved::Document {
                path: Path::new(env!("CARGO_MANIFEST_DIR")).join("README.md"),
                anchor: None,
            }
        );
    }

    // -- Path normalisation ------------------------------------------------

    #[test]
    fn normalisation_collapses_dots() {
        assert_eq!(
            normalise(Path::new("/a/b/../c/./d")),
            PathBuf::from("/a/c/d")
        );
        assert_eq!(normalise(Path::new("a/../../b")), PathBuf::from("../b"));
        assert_eq!(normalise(Path::new("./")), PathBuf::from("."));
    }

    #[test]
    fn percent_decoding_leaves_malformed_escapes_alone() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("plain"), "plain");
    }
}
