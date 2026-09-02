//! The wiki-link and backlink index, its cache format, and incremental updates.
//!
//! The index is a map from a document to everything that links *to* it, built
//! from the same link extraction the viewport uses. It is built on a background
//! thread, reports progress as it goes, and is cached between runs.
//!
//! # Why the cache is validated per file
//!
//! A directory's mtime does not change when a file nested inside it does, and an
//! "aggregate mtime" for the vault would need the full walk the cache exists to
//! avoid. So the cache records `(path, mtime, size)` for every file it indexed,
//! and the tree walk — which runs anyway — says which of those are new, changed,
//! or gone.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::config::schema::WikiLinkConfig;
use crate::doc::links::{self, LinkKind};
use crate::doc::outline::slugify;

/// The cache format version.
///
/// Bumped whenever the serialised shape changes. A mismatch discards the whole
/// cache rather than trying to migrate it: rebuilding costs seconds and a
/// half-migrated index is wrong in ways nobody can see.
pub const CACHE_VERSION: u32 = 1;

/// One link found in an indexed document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexedLink {
    /// The target exactly as written.
    pub target: String,
    /// Whether it was written as a wiki-link.
    pub wiki: bool,
    /// The one-based line in the source it was written on.
    pub line: usize,
    /// The source line, trimmed, for the backlinks list.
    pub context: String,
}

/// What the index remembers about one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEntry {
    /// Modification time when it was indexed.
    ///
    /// Stored as seconds since the epoch: `SystemTime` has no stable
    /// serialisation and the cache has to survive a version of the standard
    /// library that represents it differently.
    pub mtime: u64,
    /// Size in bytes when it was indexed.
    pub size: u64,
    /// Every link the file contains.
    pub links: Vec<IndexedLink>,
}

/// One inbound link, as the Links sidebar shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Backlink {
    /// The document the link was written in, relative to the vault root.
    pub source: PathBuf,
    /// The one-based line it was written on.
    pub line: usize,
    /// The source line, trimmed.
    pub context: String,
}

/// How a wiki-link target resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WikiResolution {
    /// Exactly one candidate.
    Found {
        /// The path, relative to the vault root.
        path: PathBuf,
        /// The heading slug from the fragment, if there was one.
        anchor: Option<String>,
    },
    /// Several candidates, which the reader has to choose between.
    ///
    /// A silent pick would send them to the wrong note and give them no way to
    /// know it had happened.
    Ambiguous {
        /// The candidates, relative to the vault root, in vault order.
        candidates: Vec<PathBuf>,
        /// The heading slug from the fragment, if there was one.
        anchor: Option<String>,
    },
    /// Nothing in the vault matches.
    ///
    /// The one case where perga offers to create a file — see Section 9.11.
    Missing {
        /// The page name as written, without any fragment.
        page: String,
        /// The heading slug from the fragment, if there was one.
        anchor: Option<String>,
    },
}

/// The serialised form of the index.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct IndexCache {
    /// The format version this was written with.
    pub version: u32,
    /// Every indexed file, keyed by its path relative to the vault root.
    ///
    /// A `BTreeMap` so the cache is byte-identical between runs that indexed
    /// the same vault, which makes a stale cache easy to spot.
    pub files: BTreeMap<PathBuf, FileEntry>,
}

/// The wiki-link and backlink index.
#[derive(Debug, Default)]
pub struct Index {
    /// Every indexed file and the links in it.
    files: BTreeMap<PathBuf, FileEntry>,
    /// Lower-cased file stem to the paths that have it.
    by_stem: HashMap<String, BTreeSet<PathBuf>>,
    /// How many files have been indexed so far.
    pub indexed: usize,
    /// How many files the walk found, once it has finished.
    pub total: Option<usize>,
    /// Whether the index is complete.
    pub ready: bool,
}

impl Index {
    /// An empty index.
    pub fn new() -> Self {
        Index::default()
    }

    /// Rebuild the index from a cache, dropping it on a version mismatch.
    pub fn from_cache(cache: IndexCache) -> Self {
        if cache.version != CACHE_VERSION {
            return Index::new();
        }

        let mut index = Index {
            indexed: cache.files.len(),
            files: cache.files,
            ..Index::new()
        };
        index.rebuild_stems();
        index
    }

    /// The cache to write for this index.
    pub fn to_cache(&self) -> IndexCache {
        IndexCache {
            version: CACHE_VERSION,
            files: self.files.clone(),
        }
    }

    /// How many files the index knows about.
    pub fn len(&self) -> usize {
        self.files.len()
    }

    /// Whether the index knows about nothing.
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Whether a cached entry is still good for a file of this mtime and size.
    ///
    /// Both, not either: a file edited within the mtime's resolution keeps its
    /// timestamp often enough that size is worth checking too.
    pub fn is_current(&self, path: &Path, mtime: Option<SystemTime>, size: u64) -> bool {
        let Some(entry) = self.files.get(path) else {
            return false;
        };
        entry.size == size && entry.mtime == seconds(mtime)
    }

    /// Record what one file contains, replacing whatever was there.
    pub fn insert(&mut self, path: PathBuf, entry: FileEntry) {
        if let Some(stem) = stem_of(&path) {
            self.by_stem.entry(stem).or_default().insert(path.clone());
        }
        self.files.insert(path, entry);
        self.indexed = self.files.len();
    }

    /// Forget a file that is no longer in the vault.
    ///
    /// Links pointing at it are not rewritten: they become broken, which is
    /// what they are.
    pub fn remove(&mut self, path: &Path) {
        self.files.remove(path);
        if let Some(stem) = stem_of(path) {
            if let Some(paths) = self.by_stem.get_mut(&stem) {
                paths.remove(path);
                if paths.is_empty() {
                    self.by_stem.remove(&stem);
                }
            }
        }
        self.indexed = self.files.len();
    }

    /// Drop every entry for a file the walk no longer reports.
    pub fn retain_paths(&mut self, live: &BTreeSet<PathBuf>) {
        let gone: Vec<PathBuf> = self
            .files
            .keys()
            .filter(|path| !live.contains(*path))
            .cloned()
            .collect();

        for path in gone {
            self.remove(&path);
        }
    }

    /// Every document that links to `target`.
    ///
    /// Inline links are resolved relative to the document they were written
    /// in; wiki-links go through the same resolution the reader's `Enter`
    /// would, so a backlink and a forward link always agree.
    pub fn backlinks(&self, target: &Path, config: &WikiLinkConfig) -> Vec<Backlink> {
        let mut out = Vec::new();

        for (source, entry) in &self.files {
            for link in &entry.links {
                let points_here = if link.wiki {
                    matches!(
                        self.resolve_wiki(&link.target, source, config),
                        WikiResolution::Found { ref path, .. } if path == target
                    )
                } else {
                    self.resolve_relative(&link.target, source).as_deref() == Some(target)
                };

                if points_here {
                    out.push(Backlink {
                        source: source.clone(),
                        line: link.line,
                        context: link.context.clone(),
                    });
                }
            }
        }

        out
    }

    /// Resolve an inline target written in `source` to a vault-relative path.
    ///
    /// `None` for anything that is not a path inside the vault, which is
    /// everything the backlink index has no opinion about.
    fn resolve_relative(&self, target: &str, source: &Path) -> Option<PathBuf> {
        if target.is_empty() || links::is_external(target) {
            return None;
        }

        let path = target.split('#').next().unwrap_or("");
        if path.is_empty() {
            return None;
        }

        let dir = source.parent().unwrap_or(Path::new(""));
        let joined = links::normalise(&dir.join(path.replace('\\', "/")));

        self.files.contains_key(&joined).then_some(joined)
    }

    /// Resolve a wiki-link target written in `source`.
    pub fn resolve_wiki(
        &self,
        target: &str,
        source: &Path,
        config: &WikiLinkConfig,
    ) -> WikiResolution {
        let (page, anchor) = match target.split_once('#') {
            Some((page, heading)) => (page.trim(), Some(slugify(heading))),
            None => (target.trim(), None),
        };

        let by_path = || self.by_relative_path(page, source, config);
        let by_name = || self.by_filename(page, config);

        // `path-first` tries an exact relative path before a filename search;
        // `filename-first` swaps the two. Both then fall back to the other.
        let candidates =
            if config.resolution == crate::config::schema::WikiResolutionOrder::PathFirst {
                by_path().or_else(by_name)
            } else {
                by_name().or_else(by_path)
            };

        match candidates {
            Some(mut candidates) if candidates.len() == 1 => WikiResolution::Found {
                path: candidates.remove(0),
                anchor,
            },
            Some(candidates) => WikiResolution::Ambiguous { candidates, anchor },
            None => WikiResolution::Missing {
                page: page.to_string(),
                anchor,
            },
        }
    }

    /// Step 1: the page name read as a path from the vault root, or relative
    /// to the document it was written in.
    fn by_relative_path(
        &self,
        page: &str,
        source: &Path,
        config: &WikiLinkConfig,
    ) -> Option<Vec<PathBuf>> {
        let dir = source.parent().unwrap_or(Path::new(""));

        for base in [Path::new(""), dir] {
            for candidate in self.with_extensions(&base.join(page), config) {
                if self.files.contains_key(&candidate) {
                    return Some(vec![candidate]);
                }
            }
        }

        None
    }

    /// Steps 2 and 3: an exact filename match anywhere in the vault, then a
    /// case-insensitive one.
    fn by_filename(&self, page: &str, config: &WikiLinkConfig) -> Option<Vec<PathBuf>> {
        // A page name with a directory in it is a path, not a filename.
        let name = Path::new(page).file_name()?.to_str()?;
        let paths = self.by_stem.get(&name.to_lowercase())?;

        // An exact-case match wins outright over a case-insensitive one, so a
        // vault holding both `Setup.md` and `setup.md` is not ambiguous when
        // the link says which.
        let exact: Vec<PathBuf> = paths
            .iter()
            .filter(|path| path.file_stem().and_then(|s| s.to_str()) == Some(name))
            .filter(|path| self.has_wiki_extension(path, config))
            .cloned()
            .collect();

        if !exact.is_empty() {
            return Some(exact);
        }

        let folded: Vec<PathBuf> = paths
            .iter()
            .filter(|path| self.has_wiki_extension(path, config))
            .cloned()
            .collect();

        (!folded.is_empty()).then_some(folded)
    }

    /// The candidate paths for a page name, with each searched extension.
    fn with_extensions(&self, base: &Path, config: &WikiLinkConfig) -> Vec<PathBuf> {
        let mut out = Vec::with_capacity(config.extensions.len() + 1);

        // A page name that already names an extension is taken as written.
        if base
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| config.extensions.iter().any(|k| k.eq_ignore_ascii_case(e)))
        {
            out.push(base.to_path_buf());
        }

        for extension in &config.extensions {
            let mut with = base.as_os_str().to_os_string();
            with.push(".");
            with.push(extension);
            out.push(PathBuf::from(with));
        }

        out
    }

    /// Whether a path has one of the extensions wiki-links search.
    fn has_wiki_extension(&self, path: &Path, config: &WikiLinkConfig) -> bool {
        path.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| config.extensions.iter().any(|k| k.eq_ignore_ascii_case(e)))
    }

    /// Rebuild the filename lookup from the file map.
    fn rebuild_stems(&mut self) {
        self.by_stem.clear();
        for path in self.files.keys() {
            if let Some(stem) = stem_of(path) {
                self.by_stem.entry(stem).or_default().insert(path.clone());
            }
        }
    }
}

/// Extract everything the index records about one file.
pub fn entry_for(source: &str, mtime: Option<SystemTime>, size: u64) -> FileEntry {
    let mut line_starts = vec![0usize];
    line_starts.extend(source.match_indices('\n').map(|(at, _)| at + 1));

    let links = links::extract(source)
        .into_iter()
        .map(|link| {
            // The line is found by binary search rather than by counting
            // newlines per link: a document with a thousand links would
            // otherwise be quadratic in its own size.
            let line = match line_starts.binary_search(&link.range.start) {
                Ok(at) => at,
                Err(at) => at - 1,
            };
            let end = line_starts
                .get(line + 1)
                .map_or(source.len(), |next| next - 1);

            IndexedLink {
                target: link.target,
                wiki: link.kind == LinkKind::Wiki,
                line: line + 1,
                context: source[line_starts[line]..end].trim().to_string(),
            }
        })
        .collect();

    FileEntry {
        mtime: seconds(mtime),
        size,
        links,
    }
}

/// A modification time as whole seconds since the epoch.
fn seconds(mtime: Option<SystemTime>) -> u64 {
    mtime
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

/// A path's lower-cased file stem, for the filename lookup.
fn stem_of(path: &Path) -> Option<String> {
    Some(path.file_stem()?.to_str()?.to_lowercase())
}

/// What the background indexer reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndexEvent {
    /// A batch of freshly parsed files.
    Indexed(Vec<(PathBuf, FileEntry)>),
    /// Every file has been parsed.
    Finished,
}

/// How many files are parsed before a batch is sent on.
///
/// The same trade-off as the walker's batch size: one message per file floods
/// the event loop, one message for the vault leaves the progress counter stuck
/// at zero until the end.
const INDEX_BATCH: usize = 64;

/// A running index build.
///
/// Dropping the handle cancels it, so a vault switched mid-index does not leave
/// a thread reading files nobody is waiting on.
#[derive(Debug)]
pub struct IndexHandle {
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl IndexHandle {
    /// Stop the build at the next file.
    pub fn cancel(&self) {
        self.cancelled
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
}

impl Drop for IndexHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

/// Parse `files` on a background thread, reporting to `sink`.
///
/// `files` are relative to `root` and have already been filtered to the ones
/// the cache does not cover: a warm start parses only what changed.
pub fn spawn(
    root: PathBuf,
    files: Vec<PathBuf>,
    sink: impl Fn(IndexEvent) + Send + Sync + 'static,
) -> IndexHandle {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    let cancelled = Arc::new(AtomicBool::new(false));
    let flag = Arc::clone(&cancelled);
    let sink = Arc::new(sink);
    let thread_sink = Arc::clone(&sink);

    let spawned = std::thread::Builder::new()
        .name("perga-indexer".to_string())
        .spawn(move || {
            let mut batch = Vec::with_capacity(INDEX_BATCH);

            for path in files {
                if flag.load(Ordering::Relaxed) {
                    return;
                }

                if let Some(entry) = read_entry(&root.join(&path)) {
                    batch.push((path, entry));
                }

                if batch.len() >= INDEX_BATCH {
                    thread_sink(IndexEvent::Indexed(std::mem::take(&mut batch)));
                    batch.reserve(INDEX_BATCH);
                }
            }

            if flag.load(Ordering::Relaxed) {
                return;
            }

            if !batch.is_empty() {
                thread_sink(IndexEvent::Indexed(batch));
            }
            thread_sink(IndexEvent::Finished);
        });

    if spawned.is_err() {
        // Without an indexer perga is still a reader; only backlinks are lost.
        sink(IndexEvent::Finished);
    }

    IndexHandle { cancelled }
}

/// Read and parse one file, skipping anything unreadable.
fn read_entry(path: &Path) -> Option<FileEntry> {
    let bytes = std::fs::read(path).ok()?;
    let metadata = std::fs::metadata(path).ok();

    // Lossy, like the document loader: a file that is not valid UTF-8 still
    // has links worth indexing, and the index never writes anything back.
    let source = String::from_utf8_lossy(&bytes);

    Some(entry_for(
        &source,
        metadata.as_ref().and_then(|m| m.modified().ok()),
        metadata.as_ref().map_or(0, |m| m.len()),
    ))
}

/// Where the index for a vault is cached.
///
/// `$XDG_CACHE_HOME/perga/<vault-hash>/index.bin`. The hash rather than the
/// path itself, because a vault path can contain anything a filesystem allows
/// and a cache directory named after it would be unreadable at best.
pub fn cache_path(root: &Path) -> Option<PathBuf> {
    let dirs = directories::ProjectDirs::from("", "", "perga")?;
    Some(
        dirs.cache_dir()
            .join(format!("{:016x}", hash_of_path(root)))
            .join("index.bin"),
    )
}

/// Load a cached index, treating any problem as no cache at all.
pub fn load_cache(root: &Path) -> Option<IndexCache> {
    let bytes = std::fs::read(cache_path(root)?).ok()?;
    postcard::from_bytes(&bytes).ok()
}

/// Write the cache, reporting a failure to the caller rather than the user.
///
/// A cache that cannot be written costs a slower start next time and nothing
/// else, so it is logged rather than shown.
pub fn save_cache(root: &Path, cache: &IndexCache) -> std::io::Result<()> {
    let Some(path) = cache_path(root) else {
        return Ok(());
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let bytes = postcard::to_allocvec(cache)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

    // Written beside the target and renamed, so an interrupted write leaves the
    // previous cache intact rather than a truncated one that fails to parse.
    let temp = path.with_extension("tmp");
    std::fs::write(&temp, bytes)?;
    std::fs::rename(&temp, &path)
}

/// Hash a vault path for its cache directory name.
fn hash_of_path(path: &Path) -> u64 {
    use std::hash::{DefaultHasher, Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::config::schema::WikiResolutionOrder;

    fn config() -> WikiLinkConfig {
        WikiLinkConfig::default()
    }

    /// An index over a small vault, with the given files and their sources.
    fn index(files: &[(&str, &str)]) -> Index {
        let mut index = Index::new();
        for (path, source) in files {
            index.insert(PathBuf::from(path), entry_for(source, None, 0));
        }
        index.ready = true;
        index
    }

    #[test]
    fn a_unique_filename_resolves_from_anywhere_in_the_vault() {
        let index = index(&[("docs/api/auth.md", ""), ("README.md", "[[Auth]]")]);

        assert_eq!(
            index.resolve_wiki("Auth", Path::new("README.md"), &config()),
            WikiResolution::Found {
                path: PathBuf::from("docs/api/auth.md"),
                anchor: None,
            }
        );
    }

    #[test]
    fn a_folder_qualified_page_resolves_by_path() {
        let index = index(&[
            ("docs/api/ambiguous.md", ""),
            ("docs/guides/ambiguous.md", ""),
        ]);

        assert_eq!(
            index.resolve_wiki("docs/guides/ambiguous", Path::new("README.md"), &config()),
            WikiResolution::Found {
                path: PathBuf::from("docs/guides/ambiguous.md"),
                anchor: None,
            }
        );
    }

    #[test]
    fn two_files_with_one_name_are_ambiguous_rather_than_guessed_at() {
        let index = index(&[
            ("docs/api/ambiguous.md", ""),
            ("docs/guides/ambiguous.md", ""),
        ]);

        let WikiResolution::Ambiguous { candidates, .. } =
            index.resolve_wiki("Ambiguous", Path::new("README.md"), &config())
        else {
            panic!("a silent pick would send the reader to the wrong note");
        };
        assert_eq!(candidates.len(), 2);
    }

    #[test]
    fn a_heading_fragment_is_slugified_with_the_anchor_slugger() {
        let index = index(&[("kurulum.md", "")]);

        assert_eq!(
            index.resolve_wiki("kurulum#Kurulum Kılavuzu", Path::new("a.md"), &config()),
            WikiResolution::Found {
                path: PathBuf::from("kurulum.md"),
                anchor: Some("kurulum-kılavuzu".to_string()),
            }
        );
    }

    #[test]
    fn a_page_nobody_has_written_yet_is_missing_not_broken() {
        let index = index(&[("README.md", "")]);

        assert_eq!(
            index.resolve_wiki("Not Yet Written", Path::new("README.md"), &config()),
            WikiResolution::Missing {
                page: "Not Yet Written".to_string(),
                anchor: None,
            }
        );
    }

    #[test]
    fn case_only_differences_prefer_the_exact_match() {
        let index = index(&[("Setup.md", ""), ("notes/setup.md", "")]);

        assert_eq!(
            index.resolve_wiki("Setup", Path::new("a.md"), &config()),
            WikiResolution::Found {
                path: PathBuf::from("Setup.md"),
                anchor: None,
            }
        );
    }

    #[test]
    fn a_case_insensitive_match_is_the_last_resort() {
        let index = index(&[("docs/Auth.md", "")]);

        assert_eq!(
            index.resolve_wiki("auth", Path::new("a.md"), &config()),
            WikiResolution::Found {
                path: PathBuf::from("docs/Auth.md"),
                anchor: None,
            }
        );
    }

    #[test]
    fn filename_first_prefers_the_name_over_the_path() {
        let files = [("Page.md", ""), ("sub/Page.md", "")];
        let source = Path::new("sub/here.md");

        // `path-first` finds the sibling, because a relative path is tried
        // before the vault-wide name search.
        let path_first = index(&files).resolve_wiki("Page", source, &config());
        assert_eq!(
            path_first,
            WikiResolution::Found {
                path: PathBuf::from("Page.md"),
                anchor: None,
            }
        );

        // `filename-first` finds both and says so.
        let config = WikiLinkConfig {
            resolution: WikiResolutionOrder::FilenameFirst,
            ..config()
        };
        let name_first = index(&files).resolve_wiki("Page", source, &config);
        assert!(matches!(name_first, WikiResolution::Ambiguous { .. }));
    }

    #[test]
    fn backlinks_find_both_wiki_and_relative_links() {
        let index = index(&[
            ("docs/api/auth.md", "# Auth"),
            ("README.md", "See [the API](docs/api/auth.md).\n"),
            ("notes.md", "Rotate with [[Auth]] every year.\n"),
        ]);

        let mut found = index.backlinks(Path::new("docs/api/auth.md"), &config());
        found.sort_by(|a, b| a.source.cmp(&b.source));

        assert_eq!(found.len(), 2);
        assert_eq!(found[0].source, PathBuf::from("README.md"));
        assert_eq!(found[0].line, 1);
        assert_eq!(found[0].context, "See [the API](docs/api/auth.md).");
        assert_eq!(found[1].source, PathBuf::from("notes.md"));
    }

    #[test]
    fn a_link_on_the_third_line_reports_line_three() {
        let entry = entry_for("one\ntwo\nsee [x](y.md) here\n", None, 0);
        assert_eq!(entry.links[0].line, 3);
        assert_eq!(entry.links[0].context, "see [x](y.md) here");
    }

    #[test]
    fn removing_a_file_marks_inbound_links_broken() {
        let mut index = index(&[("target.md", ""), ("source.md", "[[Target]]")]);
        assert_eq!(index.backlinks(Path::new("target.md"), &config()).len(), 1);

        index.remove(Path::new("target.md"));

        assert!(index
            .backlinks(Path::new("target.md"), &config())
            .is_empty());
        assert!(matches!(
            index.resolve_wiki("Target", Path::new("source.md"), &config()),
            WikiResolution::Missing { .. }
        ));
    }

    #[test]
    fn a_cache_round_trips_through_postcard() {
        let index = index(&[("a.md", "[[B]]"), ("b.md", "[[A]]")]);

        let bytes = postcard::to_allocvec(&index.to_cache()).expect("the cache serialises");
        let cache: IndexCache = postcard::from_bytes(&bytes).expect("the cache deserialises");
        let restored = Index::from_cache(cache);

        assert_eq!(restored.len(), 2);
        assert_eq!(
            restored.resolve_wiki("B", Path::new("a.md"), &config()),
            WikiResolution::Found {
                path: PathBuf::from("b.md"),
                anchor: None,
            }
        );
    }

    #[test]
    fn a_cache_from_another_version_is_discarded() {
        let mut cache = index(&[("a.md", "")]).to_cache();
        cache.version = CACHE_VERSION + 1;

        assert!(Index::from_cache(cache).is_empty());
    }

    #[test]
    fn a_cached_entry_is_only_current_when_both_mtime_and_size_match() {
        let when = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000);
        let mut index = Index::new();
        index.insert(PathBuf::from("a.md"), entry_for("x", Some(when), 1));

        assert!(index.is_current(Path::new("a.md"), Some(when), 1));
        assert!(!index.is_current(Path::new("a.md"), Some(when), 2));
        assert!(!index.is_current(Path::new("a.md"), None, 1));
        assert!(!index.is_current(Path::new("b.md"), Some(when), 1));
    }

    #[test]
    fn files_the_walk_no_longer_reports_are_dropped() {
        let mut index = index(&[("a.md", ""), ("b.md", "")]);

        index.retain_paths(&BTreeSet::from([PathBuf::from("a.md")]));

        assert_eq!(index.len(), 1);
        assert!(matches!(
            index.resolve_wiki("b", Path::new("a.md"), &config()),
            WikiResolution::Missing { .. }
        ));
    }
}
