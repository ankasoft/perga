//! Heading extraction and GitHub-compatible slugification.
//!
//! There is exactly one slug function, and five features depend on it agreeing
//! with itself: inline anchors (`file.md#section`), bare `#section` links,
//! `[[Page#Heading]]` wiki-link fragments, the outline sidebar, and the
//! `{`/`}` heading motions.

use std::collections::HashMap;

/// One heading in a document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    /// `1` for `#` through `6` for `######`.
    pub level: u8,
    /// The heading's rendered text, with inline markup removed.
    pub text: String,
    /// The anchor this heading is reachable by, unique within the document.
    pub slug: String,
    /// The byte offset of the heading in the source.
    pub offset: usize,
}

/// Turn heading text into a GitHub-style anchor.
///
/// Lower-cased with a Unicode-aware fold, spaces to hyphens, punctuation and
/// symbols dropped, but **every Unicode letter and digit is kept**. This is
/// the part that is easy to get wrong: a naive ASCII filter turns
/// `## Kurulum Kılavuzu` into `kurulum-klavuzu`, silently breaking every link
/// to it. The correct answer is `kurulum-kılavuzu`.
///
/// Duplicates are not handled here; use [`Slugger`] for that.
pub fn slugify(text: &str) -> String {
    let mut slug = String::with_capacity(text.len());

    for c in text.chars() {
        if c.is_alphanumeric() {
            slug.extend(c.to_lowercase());
        } else if c == '-' || c == '_' {
            slug.push(c);
        } else if c.is_whitespace() {
            // Collapse runs of whitespace into a single hyphen.
            if !slug.ends_with('-') && !slug.is_empty() {
                slug.push('-');
            }
        }
        // Everything else (punctuation, symbols, emoji) is dropped.
    }

    // A trailing hyphen comes from trailing whitespace or punctuation.
    while slug.ends_with('-') {
        slug.pop();
    }

    slug
}

/// Assigns unique slugs within one document.
///
/// Two headings with the same text get `heading` and `heading-1`, matching
/// GitHub, so an anchor written against the second one still resolves.
#[derive(Debug, Default)]
pub struct Slugger {
    seen: HashMap<String, usize>,
}

impl Slugger {
    /// A slugger with no headings recorded yet.
    pub fn new() -> Self {
        Slugger::default()
    }

    /// The unique slug for this heading text.
    pub fn slug(&mut self, text: &str) -> String {
        let base = slugify(text);

        match self.seen.get_mut(&base) {
            Some(count) => {
                *count += 1;
                format!("{base}-{count}")
            }
            None => {
                self.seen.insert(base.clone(), 0);
                base
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lowercases_and_hyphenates() {
        assert_eq!(slugify("Obtaining a token"), "obtaining-a-token");
        assert_eq!(slugify("API Reference"), "api-reference");
    }

    #[test]
    fn keeps_unicode_letters_and_digits() {
        // The case a naive ASCII filter gets wrong.
        assert_eq!(slugify("Kurulum Kılavuzu"), "kurulum-kılavuzu");
        assert_eq!(slugify("Ölçüm ve Şema"), "ölçüm-ve-şema");
        assert_eq!(slugify("日本語の見出し"), "日本語の見出し");
        assert_eq!(slugify("Version 2 Notes"), "version-2-notes");
    }

    #[test]
    fn drops_punctuation_and_symbols() {
        assert_eq!(slugify("What's new?"), "whats-new");
        assert_eq!(slugify("C++ / Rust"), "c-rust");
        assert_eq!(slugify("Hello, world!"), "hello-world");
        assert_eq!(slugify("🎉 Release"), "release");
    }

    #[test]
    fn keeps_hyphens_and_underscores() {
        assert_eq!(slugify("well-known_paths"), "well-known_paths");
    }

    #[test]
    fn collapses_whitespace_runs() {
        assert_eq!(slugify("too   much   space"), "too-much-space");
        assert_eq!(slugify("  leading and trailing  "), "leading-and-trailing");
        assert_eq!(slugify("tab\tseparated"), "tab-separated");
    }

    #[test]
    fn an_empty_or_symbol_only_heading_slugs_to_nothing() {
        assert_eq!(slugify(""), "");
        assert_eq!(slugify("!!!"), "");
        // All hyphens, so the trailing-hyphen strip consumes the lot.
        assert_eq!(slugify("---"), "");
    }

    #[test]
    fn duplicates_are_numbered_from_one() {
        let mut slugger = Slugger::new();
        assert_eq!(slugger.slug("Setup"), "setup");
        assert_eq!(slugger.slug("Setup"), "setup-1");
        assert_eq!(slugger.slug("Setup"), "setup-2");
        assert_eq!(slugger.slug("Other"), "other");
        assert_eq!(slugger.slug("Setup"), "setup-3");
    }

    #[test]
    fn duplicates_that_differ_only_in_punctuation_still_collide() {
        let mut slugger = Slugger::new();
        assert_eq!(slugger.slug("Set up!"), "set-up");
        assert_eq!(slugger.slug("Set up?"), "set-up-1");
    }
}
