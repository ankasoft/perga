//! The lazy file-tree model backing the files sidebar.
//!
//! The tree is an arena of [`Node`]s indexed by their path relative to the
//! vault root. The walker streams entries in whatever order it finds them, so
//! insertion creates any missing ancestors and only re-sorts the directories
//! that actually gained a child.
//!
//! # What the tree filters and what the walk filters
//!
//! The walk always finds dotted entries; the tree decides whether to show
//! them. That is what makes `.` and `a` instant — toggling either re-flattens
//! the visible rows rather than re-reading the filesystem. `.gitignore` is the
//! exception: honouring it is a property of the walk, so changing it needs a
//! new walk, and nothing in the UI offers to.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::config::schema::{FilesConfig, SortKey};
use crate::vault::walker::Entry;

/// One entry in the tree.
#[derive(Debug, Clone)]
pub struct Node {
    /// The entry's own name, without any parent components.
    pub name: String,
    /// The path relative to the vault root. Empty for the root itself.
    pub path: PathBuf,
    /// Whether this is a directory.
    pub is_dir: bool,
    /// Whether the name begins with a dot.
    pub hidden: bool,
    /// Last modification time, when the filesystem gave one.
    pub mtime: Option<SystemTime>,
    /// Size in bytes. Zero for directories.
    pub size: u64,
    /// Whether a directory's children are shown.
    pub expanded: bool,
    /// The parent's index, or `None` for the root.
    parent: Option<usize>,
    /// Child indices, kept in display order.
    children: Vec<usize>,
}

/// One visible line of the tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Row {
    /// The node this row draws.
    pub node: usize,
    /// How deep it sits below the root, for indentation.
    pub depth: usize,
}

/// The vault tree.
#[derive(Debug)]
pub struct Tree {
    /// The arena. Index 0 is always the root.
    nodes: Vec<Node>,
    /// Path to arena index, so a streamed entry finds its place in one lookup.
    index: HashMap<PathBuf, usize>,
    /// How entries within a directory are ordered.
    sort: SortKey,
    /// Reverse that order.
    sort_reverse: bool,
    /// Extensions treated as Markdown.
    extensions: Vec<String>,
    /// Show dotted entries.
    pub include_hidden: bool,
    /// Show files that are not Markdown.
    pub show_all: bool,
    /// The active name filter, if any.
    filter: Option<String>,
    /// The selected node.
    selected: usize,
    /// How many entries the walk has delivered.
    pub entries: usize,
    /// Whether the walk has finished.
    pub complete: bool,
}

/// The root's index in the arena. Always present, never drawn as a row.
const ROOT: usize = 0;

impl Tree {
    /// An empty tree configured from the `[files]` table.
    pub fn new(config: &FilesConfig) -> Self {
        let root = Node {
            name: String::new(),
            path: PathBuf::new(),
            is_dir: true,
            hidden: false,
            mtime: None,
            size: 0,
            expanded: true,
            parent: None,
            children: Vec::new(),
        };

        Tree {
            nodes: vec![root],
            index: HashMap::new(),
            sort: config.sort,
            sort_reverse: config.sort_reverse,
            extensions: config.extensions.clone(),
            include_hidden: config.include_hidden,
            show_all: config.show_all,
            filter: None,
            selected: ROOT,
            entries: 0,
            complete: false,
        }
    }

    /// The node at an arena index.
    pub fn node(&self, index: usize) -> &Node {
        &self.nodes[index]
    }

    /// Whether the walk has delivered nothing yet.
    pub fn is_empty(&self) -> bool {
        self.nodes.len() == 1
    }

    /// The active name filter.
    pub fn filter(&self) -> Option<&str> {
        self.filter.as_deref()
    }

    /// Filter the tree by a substring of the entry name.
    ///
    /// An empty filter is the same as none, so backspacing a filter away
    /// restores the tree rather than leaving it matching everything.
    pub fn set_filter(&mut self, filter: Option<String>) {
        self.filter = filter.filter(|f| !f.is_empty());
        self.keep_selection_visible();
    }

    /// Add a batch of entries from the walker.
    pub fn insert_all(&mut self, entries: impl IntoIterator<Item = Entry>) {
        let mut touched: Vec<usize> = Vec::new();

        for entry in entries {
            self.entries += 1;
            if let Some(parent) = self.insert(entry) {
                if !touched.contains(&parent) {
                    touched.push(parent);
                }
            }
        }

        // Only the directories that gained a child are re-sorted; a batch
        // landing in one directory must not cost a sort of the whole vault.
        for parent in touched {
            self.sort_children(parent);
        }
    }

    /// Add one entry, creating any missing ancestors.
    ///
    /// Returns the parent whose children need re-sorting, or `None` when the
    /// entry was already known.
    fn insert(&mut self, entry: Entry) -> Option<usize> {
        if let Some(&existing) = self.index.get(&entry.path) {
            // A watcher re-reporting a path updates it in place rather than
            // duplicating the row.
            let node = &mut self.nodes[existing];
            node.mtime = entry.mtime;
            node.size = entry.size;
            return None;
        }

        let parent = match entry.path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => self.ensure_directory(parent),
            _ => ROOT,
        };

        let name = entry
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let node = Node {
            hidden: name.starts_with('.'),
            name,
            path: entry.path.clone(),
            is_dir: entry.is_dir,
            mtime: entry.mtime,
            size: entry.size,
            expanded: false,
            parent: Some(parent),
            children: Vec::new(),
        };

        let index = self.nodes.len();
        self.nodes.push(node);
        self.index.insert(entry.path, index);
        self.nodes[parent].children.push(index);

        Some(parent)
    }

    /// Find or create the directory node for a relative path.
    fn ensure_directory(&mut self, path: &Path) -> usize {
        if let Some(&existing) = self.index.get(path) {
            return existing;
        }

        // The walk yields parents before children, so this is the rare case of
        // a path arriving without its directory — a watcher event, say.
        let parent = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => self.ensure_directory(parent),
            _ => ROOT,
        };

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        let index = self.nodes.len();
        self.nodes.push(Node {
            hidden: name.starts_with('.'),
            name,
            path: path.to_path_buf(),
            is_dir: true,
            mtime: None,
            size: 0,
            expanded: false,
            parent: Some(parent),
            children: Vec::new(),
        });
        self.index.insert(path.to_path_buf(), index);
        self.nodes[parent].children.push(index);

        index
    }

    /// Order one directory's children: directories first, then by the sort key.
    fn sort_children(&mut self, parent: usize) {
        let mut children = std::mem::take(&mut self.nodes[parent].children);

        children.sort_by(|&a, &b| {
            let (a, b) = (&self.nodes[a], &self.nodes[b]);

            // Directories first, always, whatever the sort key says.
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| self.compare(a, b))
                .then_with(|| a.name.cmp(&b.name))
        });

        self.nodes[parent].children = children;
    }

    /// Compare two entries of the same kind by the configured sort key.
    fn compare(&self, a: &Node, b: &Node) -> std::cmp::Ordering {
        let ordering = match self.sort {
            SortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            // Most recent and largest first: the interesting end of both of
            // those orders is the top.
            SortKey::Mtime => b.mtime.cmp(&a.mtime),
            SortKey::Size => b.size.cmp(&a.size),
        };

        if self.sort_reverse {
            ordering.reverse()
        } else {
            ordering
        }
    }

    /// Whether a node is a Markdown file by extension.
    pub fn is_markdown(&self, node: &Node) -> bool {
        let Some(extension) = Path::new(&node.name).extension().and_then(|e| e.to_str()) else {
            return false;
        };
        self.extensions
            .iter()
            .any(|known| known.eq_ignore_ascii_case(extension))
    }

    /// Whether a node passes the hidden and non-Markdown toggles.
    fn shown(&self, index: usize) -> bool {
        let node = &self.nodes[index];

        if node.hidden && !self.include_hidden {
            return false;
        }

        // A directory is shown even when the tree is Markdown-only: it may
        // contain Markdown, and pruning it would need the whole subtree walked
        // before the first frame.
        node.is_dir || self.show_all || self.is_markdown(node)
    }

    /// Whether a node or any of its descendants matches the active filter.
    fn matches_filter(&self, index: usize) -> bool {
        let Some(filter) = &self.filter else {
            return true;
        };

        let node = &self.nodes[index];
        if node.name.to_lowercase().contains(&filter.to_lowercase()) {
            return true;
        }

        node.children
            .iter()
            .any(|&child| self.shown(child) && self.matches_filter(child))
    }

    /// The visible rows, top to bottom.
    ///
    /// Pure: rendering calls this every frame and must never mutate the tree.
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        self.push_rows(ROOT, 0, &mut rows);
        rows
    }

    fn push_rows(&self, parent: usize, depth: usize, rows: &mut Vec<Row>) {
        for &child in &self.nodes[parent].children {
            if !self.shown(child) || !self.matches_filter(child) {
                continue;
            }

            rows.push(Row { node: child, depth });

            // A filter opens every branch it matches: a match the user cannot
            // see is not a match as far as they are concerned.
            let open = self.nodes[child].expanded || self.filter.is_some();
            if self.nodes[child].is_dir && open {
                self.push_rows(child, depth + 1, rows);
            }
        }
    }

    /// The selected node, or `None` when nothing is selected.
    pub fn selected(&self) -> Option<&Node> {
        (self.selected != ROOT).then(|| &self.nodes[self.selected])
    }

    /// The selected node's index within [`Tree::rows`].
    pub fn selected_row(&self, rows: &[Row]) -> Option<usize> {
        rows.iter().position(|row| row.node == self.selected)
    }

    /// Move the selection down or up by one visible row.
    pub fn move_selection(&mut self, delta: isize) {
        let rows = self.rows();
        if rows.is_empty() {
            self.selected = ROOT;
            return;
        }

        let current = self.selected_row(&rows);
        let next = match current {
            Some(at) => (at as isize + delta).clamp(0, rows.len() as isize - 1) as usize,
            // Nothing selected yet: either end of the list, depending on which
            // way the user moved.
            None if delta < 0 => rows.len() - 1,
            None => 0,
        };

        self.selected = rows[next].node;
    }

    /// Select a specific row.
    pub fn select_row(&mut self, row: usize) {
        let rows = self.rows();
        if let Some(row) = rows.get(row) {
            self.selected = row.node;
        }
    }

    /// Select the node at a path, if the tree knows it.
    pub fn select_path(&mut self, path: &Path) -> bool {
        match self.index.get(path) {
            Some(&index) => {
                self.selected = index;
                true
            }
            None => false,
        }
    }

    /// Expand the selected directory, and report whether anything happened.
    ///
    /// A directory that is already open moves the selection to its first
    /// child, so `l` walks into a tree the way it does in a file manager.
    pub fn expand_selected(&mut self) -> bool {
        let selected = self.selected;
        if selected == ROOT || !self.nodes[selected].is_dir {
            return false;
        }

        if self.nodes[selected].expanded {
            let rows = self.rows();
            if let Some(at) = self.selected_row(&rows) {
                if let Some(next) = rows.get(at + 1) {
                    if self.nodes[next.node].parent == Some(selected) {
                        self.selected = next.node;
                    }
                }
            }
        } else {
            self.nodes[selected].expanded = true;
        }

        true
    }

    /// Collapse the selected directory, or move to its parent.
    pub fn collapse_selected(&mut self) {
        let selected = self.selected;
        if selected == ROOT {
            return;
        }

        if self.nodes[selected].is_dir && self.nodes[selected].expanded {
            self.nodes[selected].expanded = false;
            return;
        }

        if let Some(parent) = self.nodes[selected].parent.filter(|&p| p != ROOT) {
            self.selected = parent;
        }
    }

    /// Expand every directory on the way to a path and select it.
    ///
    /// Used when a document is opened from anywhere other than the tree, so
    /// the tree always shows where the reader is.
    pub fn reveal(&mut self, path: &Path) -> bool {
        let Some(&index) = self.index.get(path) else {
            return false;
        };

        let mut at = self.nodes[index].parent;
        while let Some(node) = at {
            self.nodes[node].expanded = true;
            at = self.nodes[node].parent;
        }

        self.selected = index;
        true
    }

    /// Show or hide dotted entries.
    pub fn toggle_hidden(&mut self) {
        self.include_hidden = !self.include_hidden;
        self.keep_selection_visible();
    }

    /// Show or hide files that are not Markdown.
    pub fn toggle_all_files(&mut self) {
        self.show_all = !self.show_all;
        self.keep_selection_visible();
    }

    /// Drop a selection that a toggle or a filter has just hidden.
    fn keep_selection_visible(&mut self) {
        let rows = self.rows();
        if self.selected_row(&rows).is_none() {
            self.selected = rows.first().map_or(ROOT, |row| row.node);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, is_dir: bool) -> Entry {
        Entry {
            path: PathBuf::from(path),
            is_dir,
            mtime: None,
            size: 0,
        }
    }

    /// A tree with a small vault in it, in an order the walk might produce.
    fn tree() -> Tree {
        let mut tree = Tree::new(&FilesConfig::default());
        tree.insert_all([
            entry("README.md", false),
            entry("docs", true),
            entry("docs/setup.md", false),
            entry("docs/api", true),
            entry("docs/api/auth.md", false),
            entry(".github", true),
            entry(".github/CONTRIBUTING.md", false),
            entry("logo.png", false),
        ]);
        tree
    }

    /// The names of the visible rows, indented by depth.
    fn visible(tree: &Tree) -> Vec<String> {
        tree.rows()
            .iter()
            .map(|row| format!("{}{}", "  ".repeat(row.depth), tree.node(row.node).name))
            .collect()
    }

    #[test]
    fn directories_come_first_and_names_sort_case_insensitively() {
        let mut tree = Tree::new(&FilesConfig::default());
        tree.insert_all([
            entry("zebra.md", false),
            entry("Apple.md", false),
            entry("middle", true),
        ]);

        assert_eq!(visible(&tree), ["middle", "Apple.md", "zebra.md"]);
    }

    #[test]
    fn directories_are_collapsed_until_they_are_expanded() {
        let mut tree = tree();
        assert_eq!(visible(&tree), [".github", "docs", "README.md"]);

        tree.select_path(Path::new("docs"));
        tree.expand_selected();
        assert_eq!(
            visible(&tree),
            [".github", "docs", "  api", "  setup.md", "README.md"]
        );
    }

    #[test]
    fn dotted_directories_are_shown_by_default_and_can_be_hidden() {
        let mut tree = tree();
        assert!(visible(&tree).contains(&".github".to_string()));

        tree.toggle_hidden();
        assert!(!visible(&tree).contains(&".github".to_string()));

        tree.toggle_hidden();
        assert!(visible(&tree).contains(&".github".to_string()));
    }

    #[test]
    fn non_markdown_files_are_hidden_until_asked_for() {
        let mut tree = tree();
        assert!(!visible(&tree).contains(&"logo.png".to_string()));

        tree.toggle_all_files();
        assert!(visible(&tree).contains(&"logo.png".to_string()));
    }

    #[test]
    fn a_toggle_that_hides_the_selection_moves_it_somewhere_visible() {
        let mut tree = tree();
        assert!(tree.select_path(Path::new(".github")));

        tree.toggle_hidden();
        let selected = tree.selected().expect("something stays selected");
        assert_ne!(selected.path, PathBuf::from(".github"));
    }

    #[test]
    fn the_selection_walks_the_visible_rows() {
        let mut tree = tree();

        tree.move_selection(1);
        assert_eq!(tree.selected().unwrap().name, ".github");
        tree.move_selection(1);
        assert_eq!(tree.selected().unwrap().name, "docs");

        // ...and stops at the ends rather than wrapping.
        tree.move_selection(50);
        assert_eq!(tree.selected().unwrap().name, "README.md");
        tree.move_selection(-50);
        assert_eq!(tree.selected().unwrap().name, ".github");
    }

    #[test]
    fn expanding_an_open_directory_steps_into_it() {
        let mut tree = tree();
        tree.select_path(Path::new("docs"));

        tree.expand_selected();
        assert_eq!(tree.selected().unwrap().name, "docs");

        tree.expand_selected();
        assert_eq!(tree.selected().unwrap().path, PathBuf::from("docs/api"));
    }

    #[test]
    fn collapsing_closes_a_directory_then_moves_to_the_parent() {
        let mut tree = tree();
        tree.select_path(Path::new("docs"));
        tree.expand_selected();
        tree.select_path(Path::new("docs/setup.md"));

        // A file has nothing to collapse, so it goes to its parent.
        tree.collapse_selected();
        assert_eq!(tree.selected().unwrap().name, "docs");

        // ...which is open, so the first press closes it.
        tree.collapse_selected();
        assert_eq!(visible(&tree), [".github", "docs", "README.md"]);
    }

    #[test]
    fn revealing_a_path_opens_every_directory_above_it() {
        let mut tree = tree();
        assert!(tree.reveal(Path::new("docs/api/auth.md")));

        assert_eq!(
            visible(&tree),
            [
                ".github",
                "docs",
                "  api",
                "    auth.md",
                "  setup.md",
                "README.md"
            ]
        );
        assert_eq!(
            tree.selected().unwrap().path,
            PathBuf::from("docs/api/auth.md")
        );
    }

    #[test]
    fn a_filter_shows_matches_and_the_directories_holding_them() {
        let mut tree = tree();
        tree.set_filter(Some("auth".to_string()));

        assert_eq!(visible(&tree), ["docs", "  api", "    auth.md"]);

        // An empty filter is no filter, so backspacing it away restores the
        // tree rather than matching everything.
        tree.set_filter(Some(String::new()));
        assert_eq!(tree.filter(), None);
        assert_eq!(visible(&tree), [".github", "docs", "README.md"]);
    }

    #[test]
    fn a_filter_is_case_insensitive() {
        let mut tree = tree();
        tree.set_filter(Some("README".to_string()));
        assert_eq!(visible(&tree), ["README.md"]);

        tree.set_filter(Some("readme".to_string()));
        assert_eq!(visible(&tree), ["README.md"]);
    }

    #[test]
    fn re_reporting_a_path_updates_it_rather_than_duplicating_the_row() {
        let mut tree = tree();
        let before = tree.rows().len();

        tree.insert_all([Entry {
            path: PathBuf::from("README.md"),
            is_dir: false,
            mtime: Some(SystemTime::UNIX_EPOCH),
            size: 42,
        }]);

        assert_eq!(tree.rows().len(), before);
        assert!(tree.select_path(Path::new("README.md")));
        assert_eq!(tree.selected().unwrap().size, 42);
    }

    #[test]
    fn an_entry_arriving_before_its_directory_still_lands_in_place() {
        let mut tree = Tree::new(&FilesConfig::default());
        tree.insert_all([entry("a/b/c.md", false)]);
        assert!(tree.reveal(Path::new("a/b/c.md")));
        assert_eq!(visible(&tree), ["a", "  b", "    c.md"]);
    }

    #[test]
    fn sorting_by_mtime_puts_the_most_recent_first() {
        let config = FilesConfig {
            sort: SortKey::Mtime,
            ..FilesConfig::default()
        };

        let mut tree = Tree::new(&config);
        tree.insert_all([
            Entry {
                path: PathBuf::from("old.md"),
                is_dir: false,
                mtime: Some(SystemTime::UNIX_EPOCH),
                size: 0,
            },
            Entry {
                path: PathBuf::from("new.md"),
                is_dir: false,
                mtime: Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(60)),
                size: 0,
            },
        ]);

        assert_eq!(visible(&tree), ["new.md", "old.md"]);
    }

    #[test]
    fn sort_reverse_turns_the_order_around() {
        let config = FilesConfig {
            sort_reverse: true,
            ..FilesConfig::default()
        };

        let mut tree = Tree::new(&config);
        tree.insert_all([entry("a.md", false), entry("b.md", false), entry("d", true)]);

        // Directories still come first: reversing orders entries of the same
        // kind, it does not turn the tree upside down.
        assert_eq!(visible(&tree), ["d", "b.md", "a.md"]);
    }
}
