//! Search: project-wide content search, fuzzy file matching, find-in-document.

pub mod content;
pub mod fuzzy;
pub mod in_doc;

use std::path::PathBuf;

use crate::search::content::Hit;

/// The result of the last project-wide search.
///
/// Lives on the application rather than on a tab: Section 8.2 describes the
/// search mode as showing "the last project-wide search", one set of results
/// the whole window shares.
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    /// The query, as the reader typed it.
    pub query: String,
    /// The hits so far, in walk order.
    pub hits: Vec<Hit>,
    /// Which hit is selected in the sidebar.
    pub selected: usize,
    /// Whether the search is still running.
    pub running: bool,
    /// Whether the result cap cut the results short.
    pub truncated: bool,
    /// How long the search took, once it has finished.
    pub elapsed: Option<std::time::Duration>,
    /// The pattern error, when the query would not compile.
    pub error: Option<String>,
    /// When the search started, for the elapsed time in the mode header.
    pub started: Option<std::time::Instant>,
}

impl SearchState {
    /// Start a new search, discarding whatever the last one found.
    pub fn begin(&mut self, query: String) {
        self.query = query;
        self.hits.clear();
        self.selected = 0;
        self.running = true;
        self.truncated = false;
        self.elapsed = None;
        self.error = None;
        self.started = Some(std::time::Instant::now());
    }

    /// The files the hits fall in, in the order they were found.
    ///
    /// Results are grouped by file in the sidebar, and a `Vec` of runs rather
    /// than a map keeps them in walk order.
    pub fn groups(&self) -> Vec<(PathBuf, std::ops::Range<usize>)> {
        let mut groups: Vec<(PathBuf, std::ops::Range<usize>)> = Vec::new();

        for (at, hit) in self.hits.iter().enumerate() {
            match groups.last_mut() {
                Some((path, range)) if *path == hit.path => range.end = at + 1,
                _ => groups.push((hit.path.clone(), at..at + 1)),
            }
        }

        groups
    }

    /// What the search mode's header says.
    pub fn summary(&self) -> String {
        if let Some(error) = &self.error {
            return format!("bad pattern: {error}");
        }

        if self.query.is_empty() {
            return "no search yet".to_string();
        }

        let count = self.hits.len();
        let capped = if self.truncated { "+" } else { "" };

        match (self.running, self.elapsed) {
            (true, _) => format!("{count}{capped} hits, searching…"),
            (false, Some(elapsed)) => {
                format!("{count}{capped} hits in {} ms", elapsed.as_millis())
            }
            (false, None) => format!("{count}{capped} hits"),
        }
    }

    /// Move the selection, stopping at both ends.
    pub fn move_selection(&mut self, delta: isize) {
        if self.hits.is_empty() {
            self.selected = 0;
            return;
        }

        let last = self.hits.len() as isize - 1;
        self.selected = (self.selected as isize + delta).clamp(0, last) as usize;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(path: &str, line: u64) -> Hit {
        Hit {
            path: PathBuf::from(path),
            line,
            text: "a line".to_string(),
            span: (0, 1),
        }
    }

    #[test]
    fn hits_group_into_runs_in_walk_order() {
        let state = SearchState {
            hits: vec![
                hit("a.md", 1),
                hit("a.md", 4),
                hit("b.md", 2),
                hit("a.md", 9),
            ],
            ..SearchState::default()
        };

        let groups = state.groups();
        assert_eq!(groups.len(), 3, "a file found again is a second group");
        assert_eq!(groups[0], (PathBuf::from("a.md"), 0..2));
        assert_eq!(groups[1], (PathBuf::from("b.md"), 2..3));
    }

    #[test]
    fn the_summary_says_what_the_search_is_doing() {
        let mut state = SearchState::default();
        assert_eq!(state.summary(), "no search yet");

        state.begin("token".to_string());
        state.hits.push(hit("a.md", 1));
        assert_eq!(state.summary(), "1 hits, searching…");

        state.running = false;
        state.elapsed = Some(std::time::Duration::from_millis(12));
        assert_eq!(state.summary(), "1 hits in 12 ms");

        state.truncated = true;
        assert_eq!(state.summary(), "1+ hits in 12 ms");

        state.error = Some("unclosed group".to_string());
        assert_eq!(state.summary(), "bad pattern: unclosed group");
    }

    #[test]
    fn a_new_search_discards_the_last_ones_results() {
        let mut state = SearchState {
            hits: vec![hit("a.md", 1)],
            selected: 1,
            truncated: true,
            ..SearchState::default()
        };

        state.begin("new".to_string());

        assert!(state.hits.is_empty());
        assert_eq!(state.selected, 0);
        assert!(!state.truncated);
        assert!(state.running);
    }

    #[test]
    fn the_selection_stops_at_both_ends() {
        let mut state = SearchState {
            hits: vec![hit("a.md", 1), hit("a.md", 2)],
            ..SearchState::default()
        };

        state.move_selection(5);
        assert_eq!(state.selected, 1);
        state.move_selection(-5);
        assert_eq!(state.selected, 0);
    }
}
