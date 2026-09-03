//! Project-wide content search, built on `grep-searcher`.
//!
//! The search runs on its own thread and streams hits as it finds them, so the
//! first results appear while the rest of the vault is still being read. A new
//! query cancels the one in flight through an atomic flag the sink checks; the
//! thread then unwinds out of `grep`'s own iteration rather than being killed,
//! which is what keeps the thread count flat across a session of typing.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use grep_matcher::Matcher;
use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::{sinks::UTF8, BinaryDetection, SearcherBuilder};

use crate::config::schema::{FilesConfig, SearchConfig};
use crate::vault::walker::{self, WalkOptions};

/// How many hits are collected before they are sent on.
const BATCH: usize = 32;

/// One matching line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    /// The file, relative to the vault root.
    pub path: PathBuf,
    /// The one-based line number.
    pub line: u64,
    /// The matching line, with trailing whitespace removed.
    pub text: String,
    /// The byte range of the match within [`Hit::text`].
    pub span: (usize, usize),
}

/// What the searcher reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchEvent {
    /// A batch of hits, in the order the files were walked.
    Hits(Vec<Hit>),
    /// The search finished. `truncated` when it stopped at the result cap.
    Finished {
        /// How many hits were reported in total.
        total: usize,
        /// Whether the cap cut the results short.
        truncated: bool,
    },
    /// The pattern could not be compiled. Shown inline, never a panic.
    BadPattern(String),
}

/// A query as the reader wrote it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    /// The pattern, with any `/…/` wrapper already removed.
    pub pattern: String,
    /// Whether the pattern is a regular expression.
    pub regex: bool,
}

impl Query {
    /// Read a query the way the prompt accepts it.
    ///
    /// A `/pattern/` wrapper turns on regex for that search alone, which is
    /// the shorthand Section 9.7 asks for; otherwise the configured default
    /// decides.
    pub fn parse(input: &str, config: &SearchConfig) -> Self {
        let trimmed = input.trim();

        if trimmed.len() >= 2 && trimmed.starts_with('/') && trimmed.ends_with('/') {
            return Query {
                pattern: trimmed[1..trimmed.len() - 1].to_string(),
                regex: true,
            };
        }

        Query {
            pattern: trimmed.to_string(),
            regex: config.regex,
        }
    }

    /// Build the matcher for this query.
    pub fn matcher(&self, config: &SearchConfig) -> Result<RegexMatcher, String> {
        RegexMatcherBuilder::new()
            // Smart case is `grep`'s own: lower case matches either, and an
            // upper-case character in the pattern means it.
            .case_smart(config.smart_case)
            .fixed_strings(!self.regex)
            .line_terminator(Some(b'\n'))
            .build(&self.pattern)
            .map_err(|e| format!("{e}"))
    }
}

/// A running search.
///
/// Dropping the handle cancels it, so the searcher for a query the reader has
/// already replaced stops reading files at the next line.
#[derive(Debug)]
pub struct SearchHandle {
    cancelled: Arc<AtomicBool>,
}

impl SearchHandle {
    /// Stop the search at the next line.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl Drop for SearchHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Search `root` on a background thread, reporting to `sink`.
pub fn spawn(
    root: PathBuf,
    query: Query,
    search: SearchConfig,
    files: FilesConfig,
    walk: WalkOptions,
    sink: impl Fn(SearchEvent) + Send + Sync + 'static,
) -> SearchHandle {
    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancelled);
    let sink = Arc::new(sink);
    let thread_sink = Arc::clone(&sink);

    let spawned = std::thread::Builder::new()
        .name("perga-search".to_string())
        .spawn(move || {
            run(&root, &query, &search, &files, walk, &flag, &*thread_sink);
        });

    if spawned.is_err() {
        sink(SearchEvent::Finished {
            total: 0,
            truncated: false,
        });
    }

    SearchHandle { cancelled }
}

/// The body of the search, factored out so the tests can run it synchronously.
pub fn run(
    root: &Path,
    query: &Query,
    search: &SearchConfig,
    files: &FilesConfig,
    walk: WalkOptions,
    cancelled: &AtomicBool,
    sink: &impl Fn(SearchEvent),
) {
    if query.pattern.is_empty() {
        sink(SearchEvent::Finished {
            total: 0,
            truncated: false,
        });
        return;
    }

    let matcher = match query.matcher(search) {
        Ok(matcher) => matcher,
        // An invalid regex is something the reader typed, not a failure:
        // it is reported inline and the previous results stay put.
        Err(e) => {
            sink(SearchEvent::BadPattern(e));
            return;
        }
    };

    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        // A binary file in a notes vault is an attachment; matching inside one
        // produces unreadable lines rather than useful hits.
        .binary_detection(BinaryDetection::quit(0))
        .build();

    let mut batch: Vec<Hit> = Vec::with_capacity(BATCH);
    let mut total = 0usize;
    let mut truncated = false;

    // The same walk the tree uses, so search and tree agree about what is in
    // the vault, including the ignore rules.
    let paths = collect_paths(root, walk, files, search, cancelled);

    'files: for relative in paths {
        if cancelled.load(Ordering::Relaxed) {
            return;
        }

        let absolute = root.join(&relative);
        let mut hits: Vec<Hit> = Vec::new();

        let outcome = searcher.search_path(
            &matcher,
            &absolute,
            UTF8(|line, text| {
                let span = matcher
                    .find(text.as_bytes())
                    .ok()
                    .flatten()
                    .map_or((0, 0), |m| (m.start(), m.end()));

                hits.push(Hit {
                    path: relative.clone(),
                    line,
                    text: text.trim_end().to_string(),
                    span,
                });

                // Returning `false` stops this file; the cap and the
                // cancellation flag are both checked here so a search of a
                // huge file can be abandoned mid-file.
                Ok(!cancelled.load(Ordering::Relaxed) && total + hits.len() < search.max_results)
            }),
        );

        if let Err(e) = outcome {
            tracing::debug!("skipping {} while searching: {e}", absolute.display());
        }

        total += hits.len();
        batch.extend(hits);

        if total >= search.max_results {
            truncated = true;
        }

        if batch.len() >= BATCH || truncated {
            sink(SearchEvent::Hits(std::mem::take(&mut batch)));
            batch.reserve(BATCH);
        }

        if truncated {
            break 'files;
        }
    }

    if cancelled.load(Ordering::Relaxed) {
        return;
    }

    if !batch.is_empty() {
        sink(SearchEvent::Hits(batch));
    }

    sink(SearchEvent::Finished { total, truncated });
}

/// The files the search will read, in walk order.
fn collect_paths(
    root: &Path,
    walk: WalkOptions,
    files: &FilesConfig,
    search: &SearchConfig,
    cancelled: &AtomicBool,
) -> Vec<PathBuf> {
    let paths = std::sync::Mutex::new(Vec::new());

    walker::walk(root, walk, cancelled, &|event| {
        if let walker::WalkEvent::Entries(entries) = event {
            let mut paths = paths.lock().expect("the walk is the only writer");
            paths.extend(
                entries
                    .into_iter()
                    .filter(|entry| !entry.is_dir)
                    .filter(|entry| search.all_files || files.is_markdown(&entry.path))
                    .map(|entry| entry.path),
            );
        }
    });

    paths.into_inner().expect("the walk has finished")
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Mutex;

    fn vault() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/vault")
    }

    /// Run a search to completion and return everything it reported.
    fn search(pattern: &str, config: SearchConfig) -> (Vec<Hit>, Option<(usize, bool)>) {
        let hits = Mutex::new(Vec::new());
        let finished = Mutex::new(None);
        let query = Query::parse(pattern, &config);

        run(
            &vault(),
            &query,
            &config,
            &FilesConfig::default(),
            WalkOptions::default(),
            &AtomicBool::new(false),
            &|event| match event {
                SearchEvent::Hits(batch) => hits.lock().unwrap().extend(batch),
                SearchEvent::Finished { total, truncated } => {
                    *finished.lock().unwrap() = Some((total, truncated));
                }
                SearchEvent::BadPattern(e) => panic!("the pattern failed: {e}"),
            },
        );

        (hits.into_inner().unwrap(), finished.into_inner().unwrap())
    }

    #[test]
    fn a_literal_query_finds_its_lines() {
        let (hits, finished) = search("Bearer tokens", SearchConfig::default());

        assert_eq!(finished, Some((hits.len(), false)));
        assert!(hits
            .iter()
            .any(|hit| hit.path == Path::new("docs/api/auth.md")));

        let hit = &hits[0];
        assert!(hit.text.contains("Bearer tokens"));
        assert_eq!(&hit.text[hit.span.0..hit.span.1], "Bearer tokens");
        assert!(hit.line > 0);
    }

    #[test]
    fn a_literal_query_is_not_read_as_a_pattern() {
        // `.` and `*` would match everything if this were a regex.
        let (hits, _) = search("a.*b", SearchConfig::default());
        assert!(hits.is_empty(), "{hits:?}");
    }

    #[test]
    fn a_slash_wrapped_query_turns_on_regex_for_that_search() {
        let query = Query::parse("/tok[e]n/", &SearchConfig::default());
        assert!(query.regex);
        assert_eq!(query.pattern, "tok[e]n");

        let (hits, _) = search("/Bearer +tokens/", SearchConfig::default());
        assert!(!hits.is_empty());
    }

    #[test]
    fn smart_case_matches_either_case_until_the_query_says_otherwise() {
        let (lower, _) = search("bearer", SearchConfig::default());
        let (upper, _) = search("Bearer", SearchConfig::default());
        assert!(!lower.is_empty());
        assert_eq!(lower.len(), upper.len());

        // A pattern that only exists in lower case is not found in upper.
        let (missing, _) = search("BEARER", SearchConfig::default());
        assert!(missing.is_empty());
    }

    #[test]
    fn an_invalid_regex_is_reported_rather_than_panicking() {
        let reported = Mutex::new(None);
        let config = SearchConfig {
            regex: true,
            ..SearchConfig::default()
        };

        run(
            &vault(),
            &Query::parse("[unclosed", &config),
            &config,
            &FilesConfig::default(),
            WalkOptions::default(),
            &AtomicBool::new(false),
            &|event| {
                if let SearchEvent::BadPattern(e) = event {
                    *reported.lock().unwrap() = Some(e);
                }
            },
        );

        assert!(reported.into_inner().unwrap().is_some());
    }

    #[test]
    fn the_result_cap_truncates_rather_than_running_on() {
        let config = SearchConfig {
            max_results: 3,
            ..SearchConfig::default()
        };
        let (hits, finished) = search("e", config);

        assert!(hits.len() >= 3);
        assert_eq!(finished.map(|(_, truncated)| truncated), Some(true));
    }

    #[test]
    fn non_markdown_files_are_searched_only_when_asked_for() {
        // The fixture's `.gitignore` holds the word `notes`.
        let (markdown_only, _) = search("notes/", SearchConfig::default());
        assert!(!markdown_only
            .iter()
            .any(|hit| hit.path == Path::new(".gitignore")));

        let config = SearchConfig {
            all_files: true,
            ..SearchConfig::default()
        };
        let (everything, _) = search("notes/", config);
        assert!(everything
            .iter()
            .any(|hit| hit.path == Path::new(".gitignore")));
    }

    #[test]
    fn a_cancelled_search_stops_and_does_not_report_finishing() {
        let cancelled = AtomicBool::new(false);
        let finished = Mutex::new(false);
        let config = SearchConfig::default();

        run(
            &vault(),
            &Query::parse("e", &config),
            &config,
            &FilesConfig::default(),
            WalkOptions::default(),
            &cancelled,
            &|event| match event {
                SearchEvent::Hits(_) => cancelled.store(true, Ordering::Relaxed),
                SearchEvent::Finished { .. } => *finished.lock().unwrap() = true,
                SearchEvent::BadPattern(_) => {}
            },
        );

        assert!(!finished.into_inner().unwrap());
    }

    #[test]
    fn an_empty_query_finishes_without_reading_anything() {
        let (hits, finished) = search("   ", SearchConfig::default());
        assert!(hits.is_empty());
        assert_eq!(finished, Some((0, false)));
    }
}
