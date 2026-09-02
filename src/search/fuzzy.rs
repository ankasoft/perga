//! The quick switcher's fuzzy matching, built on `nucleo`.
//!
//! Matching runs on the main thread. A vault of 10,000 paths is a few hundred
//! kilobytes of text and scores in well under a frame; the background-thread
//! machinery the content search needs would buy nothing here and would make
//! the results lag the keystrokes that produced them.

use std::path::{Path, PathBuf};

use nucleo::Matcher;

/// One scored candidate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The path, relative to the vault root.
    pub path: PathBuf,
    /// The score `nucleo` gave it. Higher is better.
    pub score: u16,
    /// Which characters of the path's display form matched, for highlighting.
    pub indices: Vec<u32>,
}

/// A fuzzy matcher over the vault's paths.
///
/// The matcher itself is reused between queries: it owns scratch buffers that
/// are expensive to reallocate on every keystroke.
pub struct Fuzzy {
    matcher: Matcher,
}

impl Default for Fuzzy {
    fn default() -> Self {
        Fuzzy::new()
    }
}

impl Fuzzy {
    /// A matcher configured for paths.
    pub fn new() -> Self {
        Fuzzy {
            // The path configuration scores a match in the file name above one
            // in a directory component, which is what a reader typing a
            // filename means.
            matcher: Matcher::new(nucleo::Config::DEFAULT.match_paths()),
        }
    }

    /// Score `paths` against `query`, best first.
    ///
    /// An empty query matches nothing here; the switcher shows the recent list
    /// instead, which is a different thing from "everything, unordered".
    pub fn search(&mut self, paths: &[PathBuf], query: &str, limit: usize) -> Vec<Candidate> {
        if query.trim().is_empty() {
            return Vec::new();
        }

        let mut needle = Vec::new();
        let needle = nucleo::Utf32Str::new(query, &mut needle);

        let mut scored: Vec<Candidate> = paths
            .iter()
            .filter_map(|path| {
                let display = path.to_str()?;
                let mut buffer = Vec::new();
                let haystack = nucleo::Utf32Str::new(display, &mut buffer);

                let mut indices = Vec::new();
                let score = self.matcher.fuzzy_indices(haystack, needle, &mut indices)?;

                Some(Candidate {
                    path: path.clone(),
                    score,
                    indices,
                })
            })
            .collect();

        // Ties broken by the shorter path: with `auth` typed, `api/auth.md`
        // should come before `guides/authentication-notes.md`.
        scored.sort_by(|a, b| {
            b.score
                .cmp(&a.score)
                .then_with(|| path_len(&a.path).cmp(&path_len(&b.path)))
                .then_with(|| a.path.cmp(&b.path))
        });
        scored.truncate(limit);
        scored
    }
}

/// A path's length in characters, for tie-breaking.
fn path_len(path: &Path) -> usize {
    path.to_str().map_or(usize::MAX, str::len)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths() -> Vec<PathBuf> {
        [
            "README.md",
            "docs/api/auth.md",
            "docs/api/errors.md",
            "docs/guides/setup.md",
            "docs/guides/authentication-notes.md",
        ]
        .iter()
        .map(PathBuf::from)
        .collect()
    }

    #[test]
    fn a_query_matches_out_of_order_characters() {
        let mut fuzzy = Fuzzy::new();
        let found = fuzzy.search(&paths(), "dgs", 10);

        assert!(found
            .iter()
            .any(|c| c.path == Path::new("docs/guides/setup.md")));
    }

    #[test]
    fn the_shortest_path_wins_a_tie() {
        let mut fuzzy = Fuzzy::new();
        let found = fuzzy.search(&paths(), "auth", 10);

        assert_eq!(found[0].path, PathBuf::from("docs/api/auth.md"));
    }

    #[test]
    fn the_matched_characters_come_back_for_highlighting() {
        let mut fuzzy = Fuzzy::new();
        let found = fuzzy.search(&paths(), "auth", 10);

        let candidate = &found[0];
        let display = candidate.path.to_str().unwrap();
        let matched: String = candidate
            .indices
            .iter()
            .filter_map(|&at| display.chars().nth(at as usize))
            .collect();

        assert_eq!(matched.to_lowercase(), "auth");
    }

    #[test]
    fn a_query_matching_nothing_returns_nothing() {
        let mut fuzzy = Fuzzy::new();
        assert!(fuzzy.search(&paths(), "zzzzz", 10).is_empty());
    }

    #[test]
    fn an_empty_query_returns_nothing_rather_than_everything() {
        let mut fuzzy = Fuzzy::new();
        assert!(fuzzy.search(&paths(), "", 10).is_empty());
        assert!(fuzzy.search(&paths(), "   ", 10).is_empty());
    }

    #[test]
    fn the_limit_is_respected() {
        let mut fuzzy = Fuzzy::new();
        assert_eq!(fuzzy.search(&paths(), "d", 2).len(), 2);
    }
}
