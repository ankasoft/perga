//! Find within the open document.
//!
//! Matching is over the document's *source*, so a match has a byte offset and
//! goes through the offset↔line map like everything else that has to know
//! where it ended up on screen. Highlighting is done separately, by searching
//! the rendered text of the lines that are actually visible — see
//! `docs/decisions.md`.

use crate::ui::overlay::prompt::TextInput;

/// One tab's find state.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FindState {
    /// The query being typed.
    pub input: TextInput,
    /// Byte offsets of every match, in document order.
    pub matches: Vec<usize>,
    /// Which match is current, as an index into [`FindState::matches`].
    pub current: Option<usize>,
}

impl FindState {
    /// An empty find, ready to be typed into.
    pub fn new() -> Self {
        FindState::default()
    }

    /// The query as it stands.
    pub fn query(&self) -> &str {
        self.input.value()
    }

    /// How many matches there are.
    pub fn count(&self) -> usize {
        self.matches.len()
    }

    /// What the find bar shows on its right: `3/17`, or nothing yet.
    pub fn position(&self) -> String {
        if self.query().is_empty() {
            return String::new();
        }
        if self.matches.is_empty() {
            return "no matches".to_string();
        }

        match self.current {
            Some(at) => format!("{}/{}", at + 1, self.matches.len()),
            None => format!("{} matches", self.matches.len()),
        }
    }

    /// Re-run the search after the query changed.
    ///
    /// The current match is kept where it can be: typing another character
    /// narrows the results, and jumping back to the first match every
    /// keystroke would fight the reader.
    pub fn refresh(&mut self, source: &str) {
        let previous = self.current_offset();

        self.matches = find_all(source, self.query());
        self.current = match previous {
            // The nearest match at or after where the reader already was.
            Some(offset) => {
                self.matches
                    .iter()
                    .position(|&m| m >= offset)
                    .or(if self.matches.is_empty() {
                        None
                    } else {
                        Some(0)
                    })
            }
            None if self.matches.is_empty() => None,
            None => Some(0),
        };
    }

    /// The byte offset of the current match.
    pub fn current_offset(&self) -> Option<usize> {
        self.matches.get(self.current?).copied()
    }

    /// Step to the next or previous match, wrapping at both ends.
    pub fn step(&mut self, forward: bool) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }

        let count = self.matches.len();
        self.current = Some(match self.current {
            Some(at) if forward => (at + 1) % count,
            Some(at) => (at + count - 1) % count,
            None if forward => 0,
            None => count - 1,
        });

        self.current_offset()
    }
}

/// Whether a query is matched case-sensitively.
///
/// Smart case: a query written in lower case matches either case, and one
/// containing an upper-case letter means it.
pub fn is_case_sensitive(query: &str) -> bool {
    query.chars().any(char::is_uppercase)
}

/// Every offset in `haystack` at which `query` occurs.
///
/// Overlapping matches are not reported: after a match, the search resumes
/// past it, which is what `n` stepping through results means to a reader.
pub fn find_all(haystack: &str, query: &str) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }

    if is_case_sensitive(query) {
        return haystack.match_indices(query).map(|(at, _)| at).collect();
    }

    // Lower-casing can change a string's length — `İ` becomes two chars — so
    // offsets are tracked against the original rather than the folded copy.
    let folded_query = query.to_lowercase();
    let mut out = Vec::new();
    let mut from = 0usize;

    while from < haystack.len() {
        let Some(rest) = haystack.get(from..) else {
            // `from` landed inside a character; step to the next boundary.
            from += 1;
            continue;
        };

        let Some(at) = rest.to_lowercase().find(&folded_query) else {
            break;
        };

        // The folded index is only usable when folding did not move anything,
        // so the offset is recovered by folding prefixes of the original.
        let Some(real) = recover_offset(rest, at) else {
            break;
        };

        out.push(from + real);
        from += real + query.len().max(1);
    }

    out
}

/// Translate an offset in a lower-cased string back to the original.
///
/// Folding is per character and can change lengths, so the original offset is
/// found by walking characters and accumulating both lengths together.
fn recover_offset(original: &str, folded_offset: usize) -> Option<usize> {
    let mut folded = 0usize;

    for (at, c) in original.char_indices() {
        if folded >= folded_offset {
            return Some(at);
        }
        folded += c.to_lowercase().map(char::len_utf8).sum::<usize>();
    }

    (folded >= folded_offset).then_some(original.len())
}

/// Every `(start, end)` column range in a rendered line matching `query`.
///
/// Used for the inline highlighting, which works on what is on screen rather
/// than on the source: the reader is looking at rendered text, and a highlight
/// that lands on a different run of characters is worse than none.
pub fn matches_in_line(line: &str, query: &str) -> Vec<(usize, usize)> {
    find_all(line, query)
        .into_iter()
        .filter_map(|at| {
            // The match is as long as the query except where folding changed
            // a length, so the end is measured rather than assumed.
            let end = recover_end(line, at, query)?;
            Some((at, end))
        })
        .collect()
}

/// Where a match starting at `at` ends in the original line.
fn recover_end(line: &str, at: usize, query: &str) -> Option<usize> {
    let rest = line.get(at..)?;
    let wanted = query.to_lowercase().chars().count();
    let mut folded = 0usize;

    for (offset, c) in rest.char_indices() {
        if folded >= wanted {
            return Some(at + offset);
        }
        folded += c.to_lowercase().count();
    }

    Some(line.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lower_case_query_matches_either_case() {
        assert_eq!(find_all("Token and token", "token"), vec![0, 10]);
    }

    #[test]
    fn an_upper_case_query_means_it() {
        assert!(is_case_sensitive("Token"));
        assert_eq!(find_all("Token and token", "Token"), vec![0]);
    }

    #[test]
    fn matches_do_not_overlap() {
        assert_eq!(find_all("aaaa", "aa"), vec![0, 2]);
    }

    #[test]
    fn an_empty_query_matches_nothing() {
        assert!(find_all("anything", "").is_empty());
    }

    #[test]
    fn offsets_are_into_the_original_text() {
        let source = "başlık ve Başlık";
        let found = find_all(source, "başlık");

        assert_eq!(found.len(), 2);
        for at in found {
            assert!(
                source.is_char_boundary(at),
                "offset {at} is not a character boundary"
            );
        }
        assert_eq!(&source[found_second(source)..], "Başlık");
    }

    /// The offset of the second match, for the assertion above.
    fn found_second(source: &str) -> usize {
        find_all(source, "başlık")[1]
    }

    #[test]
    fn stepping_wraps_in_both_directions() {
        let mut find = FindState::new();
        for c in "token".chars() {
            find.input
                .apply(crate::ui::overlay::prompt::TextEdit::Insert(c));
        }
        find.refresh("token a token b token");

        assert_eq!(find.count(), 3);
        assert_eq!(find.current, Some(0));

        find.step(true);
        find.step(true);
        assert_eq!(find.current, Some(2));

        find.step(true);
        assert_eq!(find.current, Some(0), "the last match wraps to the first");

        find.step(false);
        assert_eq!(find.current, Some(2));
    }

    #[test]
    fn typing_another_character_keeps_the_reader_where_they_were() {
        let source = "alpha beta alphabet";
        let mut find = FindState::new();

        for c in "alpha".chars() {
            find.input
                .apply(crate::ui::overlay::prompt::TextEdit::Insert(c));
        }
        find.refresh(source);
        find.step(true);
        assert_eq!(find.current_offset(), Some(11));

        // Narrowing to `alphab` leaves only the second occurrence, and the
        // reader stays on it rather than being thrown back to the top.
        find.input
            .apply(crate::ui::overlay::prompt::TextEdit::Insert('b'));
        find.refresh(source);
        assert_eq!(find.current_offset(), Some(11));
    }

    #[test]
    fn the_position_says_what_there_is_to_say() {
        let mut find = FindState::new();
        assert_eq!(find.position(), "");

        for c in "zzz".chars() {
            find.input
                .apply(crate::ui::overlay::prompt::TextEdit::Insert(c));
        }
        find.refresh("nothing here");
        assert_eq!(find.position(), "no matches");

        find.input
            .apply(crate::ui::overlay::prompt::TextEdit::Clear);
        for c in "e".chars() {
            find.input
                .apply(crate::ui::overlay::prompt::TextEdit::Insert(c));
        }
        find.refresh("one two three");
        assert_eq!(find.position(), "1/3");
    }

    #[test]
    fn line_matches_are_column_ranges_into_the_line() {
        let line = "see the Token and the token";
        let found = matches_in_line(line, "token");

        assert_eq!(found.len(), 2);
        assert_eq!(&line[found[0].0..found[0].1], "Token");
        assert_eq!(&line[found[1].0..found[1].1], "token");
    }
}
