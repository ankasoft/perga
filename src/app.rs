//! The `App` struct, the blocking event loop, and top-level state transitions.
//!
//! Everything here is deliberately terminal-free: `App` is constructed, fed
//! [`Action`]s, and asserted on without a backend. The loop that reads from the
//! terminal lives in [`run`], and it does nothing but translate messages into
//! actions and hand them to [`App::update`].

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use crossbeam_channel::Receiver;
use ratatui::layout::Rect;

use crate::action::Action;
use crate::config::keymap::Keymap;
use crate::config::schema::{
    EditorConfig, FilesConfig, GeneralConfig, SearchConfig, SessionConfig, ThemeConfig, UiConfig,
    WatchConfig, WikiLinkConfig,
};
use crate::doc::document::Document;
use crate::doc::highlight::Highlighter;
use crate::doc::links;
use crate::doc::render::{RenderedDocument, Renderer};
use crate::editor::buffer::{self, EditorState, SaveError};
use crate::search::content::{self, SearchEvent, SearchHandle};
use crate::search::in_doc::FindState;
use crate::search::SearchState;
use crate::terminal::{self, Tui};
use crate::theme::Theme;
use crate::ui;
use crate::ui::layout::{self, SidebarPlacement};
use crate::ui::overlay::prompt::{TextEdit, TextInput};
use crate::ui::sidebar::SidebarMode;
use crate::vault::index::{self, IndexEvent, WikiResolution};
use crate::vault::walker::{WalkEvent, WalkOptions};
use crate::vault::watch::WatchEvent;
use crate::vault::Vault;

/// Which pane has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    /// The sidebar.
    Sidebar,
    /// The document viewport.
    #[default]
    Viewport,
    /// An overlay, which swallows all input except `Esc` and `Ctrl+C`.
    Overlay,
}

/// Whether a tab is being read or edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabMode {
    /// Reading. The mode perga opens in, always.
    #[default]
    Read,
    /// Editing.
    Edit,
}

impl TabMode {
    /// The label shown at the left of the status bar.
    pub fn label(self) -> &'static str {
        match self {
            TabMode::Read => "READ",
            TabMode::Edit => "EDIT",
        }
    }
}

/// Somewhere a tab has been, exactly enough to go back to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    /// The document.
    pub path: PathBuf,
    /// The heading the reader was at, when they arrived at one.
    pub anchor: Option<String>,
    /// The rendered line offset, restored exactly on the way back.
    pub scroll: usize,
}

/// The most locations one tab remembers.
///
/// A cap rather than an unbounded stack: cyclic links (A → B → A → …) are
/// ordinary in a wiki, and without a cap a reader following them long enough
/// grows the stack without limit.
pub const MAX_HISTORY: usize = 100;

/// One tab's back and forward stacks.
#[derive(Debug, Clone, Default)]
pub struct History {
    back: Vec<Location>,
    forward: Vec<Location>,
}

impl History {
    /// Record where the reader is leaving from.
    ///
    /// Navigating anywhere new truncates the forward stack: the future the
    /// reader had is not the future they are choosing.
    pub fn push(&mut self, from: Location) {
        self.forward.clear();
        self.back.push(from);

        if self.back.len() > MAX_HISTORY {
            self.back.remove(0);
        }
    }

    /// Step back, given where the reader is now.
    pub fn back(&mut self, current: Location) -> Option<Location> {
        let previous = self.back.pop()?;
        self.forward.push(current);
        Some(previous)
    }

    /// Step forward, given where the reader is now.
    pub fn forward(&mut self, current: Location) -> Option<Location> {
        let next = self.forward.pop()?;
        self.back.push(current);
        Some(next)
    }

    /// How many steps back are available.
    pub fn back_len(&self) -> usize {
        self.back.len()
    }

    /// How many steps forward are available.
    pub fn forward_len(&self) -> usize {
        self.forward.len()
    }
}

/// One tab: a document, its history, and its own scroll and find state.
#[derive(Debug, Default)]
pub struct Tab {
    /// The open document. `None` shows the welcome screen.
    pub doc: Option<Document>,
    /// The document's layout at the current width.
    pub layout: RenderedDocument,
    /// Rendered line offset. Not a `u16`: a 100,000-line document overflows it.
    pub scroll: usize,
    /// Horizontal offset, for clipped code blocks and wide tables.
    pub hscroll: u16,
    /// Read or edit.
    pub mode: TabMode,
    /// Where this tab has been. Per tab, never shared.
    pub history: History,
    /// The link `Enter` would follow, as an index into the document's links.
    pub focused_link: Option<usize>,
    /// This tab's find state, kept when the bar closes so `Ctrl+F` reopens
    /// where the reader left off.
    pub find: Option<FindState>,
    /// The edit buffer, while this tab is in edit mode.
    pub editor: Option<EditorState>,
    /// Whether the buffer has edits that are not on disk.
    ///
    /// Owned by the tab rather than by the editor so that the tab bar and the
    /// quit prompt can ask about a tab that is not the active one.
    pub dirty: bool,
}

impl Tab {
    /// A tab showing this document.
    pub fn with_document(doc: Document) -> Self {
        Tab {
            doc: Some(doc),
            ..Tab::default()
        }
    }

    /// The label shown in the tab bar.
    ///
    /// A tab with no document open shows the welcome screen.
    pub fn label(&self) -> &str {
        match &self.doc {
            Some(doc) => doc.label(),
            None => "welcome",
        }
    }

    /// The label as the tab bar draws it: truncated, with a dirty marker.
    pub fn display_label(&self) -> String {
        let label = truncate_middle(self.label(), MAX_TAB_LABEL);
        if self.dirty {
            format!("{DIRTY_MARKER} {label}")
        } else {
            label
        }
    }

    /// The furthest the viewport may scroll, keeping one screen in view.
    ///
    /// `None` while the document has not been fully measured, in which case
    /// scrolling is not clamped and the scrollbar shows as indeterminate.
    fn max_scroll(&self, height: u16) -> Option<usize> {
        let doc = self.doc.as_ref()?;
        let total = self.layout.total_lines(doc)?;
        Some(total.saturating_sub(usize::from(height).max(1)))
    }
}

/// The most rows the quick switcher scores and shows.
pub const SWITCHER_ROWS: usize = 100;

/// The most documents the session's recent list remembers.
pub const MAX_RECENT: usize = 50;

/// The most tabs that may be open at once.
///
/// Past this the active tab is reused: twenty labels is already more than fits
/// on a comfortable terminal, and an unbounded tab bar is a worse answer than
/// saying so.
pub const MAX_TABS: usize = 20;

/// The widest a tab label is drawn, in columns.
pub const MAX_TAB_LABEL: usize = 20;

/// Marks a tab whose buffer has unsaved edits.
pub const DIRTY_MARKER: &str = "●";

/// The byte offset a zero-based source line begins at.
fn offset_of_source_line(source: &str, line: usize) -> usize {
    if line == 0 {
        return 0;
    }

    source
        .match_indices('\n')
        .nth(line - 1)
        .map_or(source.len(), |(at, _)| at + 1)
}

/// Give a typed path a Markdown extension when it has none.
fn with_markdown_extension(query: &str, files: &FilesConfig) -> String {
    let query = query.trim();
    if files.is_markdown(Path::new(query)) {
        return query.to_string();
    }

    let extension = files.extensions.first().map_or("md", String::as_str);
    format!("{query}.{extension}")
}

/// Shorten a label to `width` columns, eliding the middle.
///
/// The middle rather than the tail: `2024-01-conference-notes.md` and
/// `2024-01-conference-slides.md` are told apart by their ends, and a tail
/// ellipsis makes every file in a dated vault look the same.
pub fn truncate_middle(label: &str, width: usize) -> String {
    use unicode_width::UnicodeWidthChar;

    let total: usize = label.chars().map(|c| c.width().unwrap_or(0)).sum();
    if total <= width || width < 3 {
        return label.to_string();
    }

    let keep = width - 1;
    let head_width = keep.div_ceil(2);
    let tail_width = keep - head_width;

    let mut head = String::new();
    let mut used = 0;
    for c in label.chars() {
        let w = c.width().unwrap_or(0);
        if used + w > head_width {
            break;
        }
        head.push(c);
        used += w;
    }

    let mut tail = String::new();
    let mut used = 0;
    for c in label.chars().rev() {
        let w = c.width().unwrap_or(0);
        if used + w > tail_width {
            break;
        }
        tail.insert(0, c);
        used += w;
    }

    format!("{head}…{tail}")
}

/// The sidebar's own state.
#[derive(Debug, Clone)]
pub struct Sidebar {
    /// Whether the user has it switched on.
    pub visible: bool,
    /// The width the user has chosen, in columns.
    pub width: u16,
    /// Which of the four modes is showing.
    pub mode: SidebarMode,
    /// The tree filter being typed, if the filter line is open.
    ///
    /// Separate from the filter the tree is applying: closing the line keeps
    /// the filter, and cancelling it clears both.
    pub filter: Option<TextInput>,
    /// Which heading the outline mode has selected.
    ///
    /// Selection is per mode: moving down the outline must not move the tree
    /// cursor underneath it.
    pub outline_selected: usize,
}

/// How prominent a status message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Ordinary feedback.
    Info,
    /// Something did not work but the application carries on.
    Warning,
    /// Something failed.
    Error,
}

/// The status bar's transient content.
#[derive(Debug, Clone, Default)]
pub struct StatusLine {
    /// The current message, if any.
    pub message: Option<(String, Severity)>,
    /// A partially typed key sequence, shown as `g…`.
    pub pending: Option<String>,
}

impl StatusLine {
    /// Show a message.
    pub fn set(&mut self, text: impl Into<String>, severity: Severity) {
        self.message = Some((text.into(), severity));
    }

    /// Clear any message.
    pub fn clear(&mut self) {
        self.message = None;
    }
}

/// Which overlay is open, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    /// The keybinding reference, generated from the resolved keymap.
    Help {
        /// How far the reference is scrolled.
        scroll: u16,
    },
    /// The find bar. The state itself lives on the tab, which is what makes
    /// find per tab; this only says the bar has focus.
    Find,
    /// A wiki-link that matched more than one page.
    Disambiguate {
        /// The page name as written.
        page: String,
        /// The candidates, relative to the vault root.
        candidates: Vec<PathBuf>,
        /// The heading fragment to apply once one is chosen.
        anchor: Option<String>,
        /// Which candidate is selected.
        selected: usize,
    },
    /// A single-line prompt. What it is for is in the [`PromptKind`].
    Prompt {
        /// What the prompt will do with what is typed.
        kind: PromptKind,
        /// The line being edited.
        input: TextInput,
    },
    /// The fuzzy quick switcher.
    Switcher {
        /// The query being typed.
        input: TextInput,
        /// The rows, best match first, or the recent list for an empty query.
        rows: Vec<SwitcherRow>,
        /// Which row is selected.
        selected: usize,
    },
    /// A yes/no/cancel question.
    Confirm {
        /// What is being asked.
        question: String,
        /// The choices, as `(key, label)`.
        choices: Vec<(char, String)>,
        /// What confirming will do.
        action: ConfirmAction,
    },
    /// Link hint mode: every visible link wears a label.
    Hints {
        /// The links wearing labels, in reading order.
        links: Vec<usize>,
        /// The label typed so far.
        typed: String,
    },
}

/// What a confirmation is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    /// Leaving edit mode with unsaved edits.
    LeaveEditMode,
    /// Quitting with unsaved edits in some tab.
    Quit,
    /// Saving over a file that changed on disk since it was loaded.
    OverwriteChanged,
    /// Creating the page a broken wiki-link named.
    CreatePage {
        /// Where it would be created.
        path: PathBuf,
    },
    /// Restoring unsaved text left behind by an earlier run.
    RestoreRecovery {
        /// The document the text belongs to.
        path: PathBuf,
    },
}

/// What a prompt will do with what is typed into it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// Run a project-wide search.
    ProjectSearch,
    /// Create a file at the path typed.
    NewFile,
    /// Rename the active document.
    RenameDocument,
    /// Rename the entry the tree has selected.
    RenameTreeEntry,
}

impl PromptKind {
    /// The prefix drawn at the left of the prompt.
    pub fn prefix(self) -> &'static str {
        match self {
            PromptKind::ProjectSearch => "search: ",
            PromptKind::NewFile => "new file: ",
            PromptKind::RenameDocument | PromptKind::RenameTreeEntry => "rename to: ",
        }
    }
}

/// One row of the quick switcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitcherRow {
    /// The path, relative to the vault root.
    pub path: PathBuf,
    /// Which characters of the path matched, for highlighting.
    pub indices: Vec<u32>,
    /// Whether this row is the offer to create a file that does not exist.
    pub create: bool,
}

/// The whole application state.
pub struct App {
    /// The open directory: its root, its tree, and its index.
    pub vault: Vault,
    /// The `[files]` table, which the tree and the walker both read.
    pub files: FilesConfig,
    /// The `[general]` table.
    pub general: GeneralConfig,
    /// The `[wikilinks]` table.
    pub wikilinks: WikiLinkConfig,
    /// The `[search]` table.
    pub search_config: SearchConfig,
    /// The `[editor]` table.
    pub editor_config: EditorConfig,
    /// Writes perga made, so the watcher does not report them back.
    pub own_writes: crate::vault::watch::OwnWrites,
    /// Set when the active document should be handed to `$EDITOR`.
    pub external_edit: Option<PathBuf>,
    /// The `[watch]` table.
    pub watch_config: WatchConfig,
    /// The `[session]` table.
    pub session_config: SessionConfig,
    /// The `[theme]` table. Its `name` follows a runtime switch, so the file
    /// watcher re-reads whichever theme is showing rather than the one that
    /// was configured.
    pub theme_config: ThemeConfig,
    /// Whether `--theme` was given, which pins the theme for this run.
    ///
    /// A session records the theme so a runtime switch survives a restart, and
    /// a flag on the command line is a deliberate choice for *this* run that
    /// the session must not override.
    pub theme_pinned: bool,
    /// The running watch. Dropping it stops watching.
    watch: Option<crate::vault::watch::WatchHandle>,
    /// The watch on the theme directory, for hot reload while authoring one.
    theme_watch: Option<crate::vault::watch::WatchHandle>,
    /// The last project-wide search.
    pub search: SearchState,
    /// Documents opened this session, most recent first.
    ///
    /// What the quick switcher shows before anything has been typed.
    pub recent: Vec<PathBuf>,
    /// The running project search. Dropping it cancels the search.
    search_handle: Option<SearchHandle>,
    /// The fuzzy matcher, reused between keystrokes for its scratch buffers.
    fuzzy: crate::search::fuzzy::Fuzzy,
    /// How a project search reports back.
    search_sink: Option<Arc<dyn Fn(SearchEvent) + Send + Sync>>,
    /// How the index reports back, once the walk has said what to index.
    ///
    /// Held rather than passed in because the build starts when the *walk*
    /// finishes, which is several frames after the application was built.
    index_sink: Option<Arc<dyn Fn(IndexEvent) + Send + Sync>>,
    /// The resolved theme. Nothing in the UI hardcodes a colour.
    pub theme: Theme,
    /// Syntax highlighting, which loads on a background thread.
    pub highlighter: Highlighter,
    /// The resolved keymap, and the source of the help overlay.
    pub keymap: Keymap,
    /// Presentation settings.
    pub ui: UiConfig,
    /// Open tabs. Never empty.
    pub tabs: Vec<Tab>,
    /// Index into [`App::tabs`].
    pub active_tab: usize,
    /// The sidebar.
    pub sidebar: Sidebar,
    /// Which pane has focus.
    pub focus: Focus,
    /// The open overlay, if any.
    pub overlay: Option<Overlay>,
    /// The status bar.
    pub status: StatusLine,
    /// Set when the application should exit after the current update.
    pub should_quit: bool,
    /// Set when the application should suspend itself after the current update.
    pub should_suspend: bool,
    /// The process exit code.
    pub exit_code: u8,
    /// The last known terminal size, so layout decisions can be made outside a
    /// draw call.
    pub area: Rect,
    /// Whether mouse capture is on.
    pub mouse_capture: bool,
}

impl App {
    /// A new application with built-in defaults.
    pub fn new(theme: Theme, keymap: Keymap, ui: UiConfig, files: FilesConfig) -> Self {
        let sidebar = Sidebar {
            visible: ui.sidebar_visible,
            width: ui.sidebar_width,
            mode: ui.sidebar_default_mode,
            filter: None,
            outline_selected: 0,
        };
        let mouse_capture = ui.mouse;

        // Anything the keymap could not make sense of is the user's to know
        // about; it is never a hard error.
        let mut status = StatusLine::default();
        if let Some(warning) = keymap.warnings().first() {
            status.set(warning.clone(), Severity::Warning);
        }

        App {
            vault: Vault::new(".", &files),
            files,
            general: GeneralConfig::default(),
            wikilinks: WikiLinkConfig::default(),
            search_config: SearchConfig::default(),
            editor_config: EditorConfig::default(),
            own_writes: crate::vault::watch::OwnWrites::default(),
            external_edit: None,
            watch_config: WatchConfig::default(),
            session_config: SessionConfig::default(),
            theme_config: ThemeConfig::default(),
            theme_pinned: false,
            watch: None,
            theme_watch: None,
            search: SearchState::default(),
            recent: Vec::new(),
            search_handle: None,
            fuzzy: crate::search::fuzzy::Fuzzy::new(),
            search_sink: None,
            index_sink: None,
            theme,
            highlighter: Highlighter::new(),
            keymap,
            ui,
            tabs: vec![Tab::default()],
            active_tab: 0,
            sidebar,
            focus: Focus::Viewport,
            overlay: None,
            status,
            should_quit: false,
            should_suspend: false,
            exit_code: 0,
            area: Rect::default(),
            mouse_capture,
        }
    }

    /// The active tab.
    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }

    /// The active document's path, relative to the vault root.
    pub fn title_path(&self) -> Option<String> {
        self.tab()
            .doc
            .as_ref()
            .map(|doc| doc.display_path(&self.vault.root))
    }

    /// The active document's path relative to the vault root, for matching
    /// against tree rows.
    pub fn active_path(&self) -> Option<PathBuf> {
        let doc = self.tab().doc.as_ref()?;
        self.vault.relative(&doc.path).map(Path::to_path_buf)
    }

    /// The scroll position, as `current/total`, once the total is known.
    pub fn scroll_position(&self) -> Option<(usize, usize)> {
        let tab = self.tab();
        let doc = tab.doc.as_ref()?;
        let total = tab.layout.total_lines(doc)?;
        Some((tab.scroll.saturating_add(1).min(total.max(1)), total))
    }

    /// Point the vault root at a directory, discarding any tree already built
    /// for the previous one.
    pub fn set_vault_root(&mut self, root: impl AsRef<Path>) {
        self.vault = Vault::new(root.as_ref(), &self.files);
    }

    /// Start walking the vault, reporting each batch back through `sink`.
    ///
    /// The sink is what turns a worker message into an [`Action`]; nothing on
    /// the walker thread touches application state.
    pub fn start_walk(&mut self, sink: impl Fn(WalkEvent) + Send + Sync + 'static) {
        let options = WalkOptions {
            respect_gitignore: self.files.respect_gitignore,
            follow_symlinks: self.general.follow_symlinks,
        };
        self.vault.start_walk(options, sink);
    }

    /// Register how the index reports back.
    ///
    /// The build itself starts when the walk finishes and has said which files
    /// the cache does not already cover.
    pub fn on_index(&mut self, sink: impl Fn(IndexEvent) + Send + Sync + 'static) {
        self.index_sink = Some(Arc::new(sink));
    }

    /// Register how a project search reports back.
    pub fn on_search(&mut self, sink: impl Fn(SearchEvent) + Send + Sync + 'static) {
        self.search_sink = Some(Arc::new(sink));
    }

    /// A renderer for the current theme at a given content width.
    pub fn renderer(&self, width: u16) -> Renderer {
        Renderer::new(&self.theme, self.highlighter.clone(), width)
    }

    /// The content area of the viewport, inside its border.
    pub fn viewport_inner(&self) -> Rect {
        let viewport = self.frames().viewport;
        Rect {
            x: viewport.x.saturating_add(1),
            y: viewport.y.saturating_add(1),
            width: viewport.width.saturating_sub(2),
            height: viewport.height.saturating_sub(2),
        }
    }

    /// Open a document in the active tab.
    pub fn open(&mut self, doc: Document) {
        if let Some(reason) = doc.read_only {
            self.status.set(reason.message(), Severity::Warning);
        }
        // Everything about the *view* is replaced; the tab's history and its
        // read-or-edit mode belong to the tab, not to the document in it.
        let path = doc.path.clone();
        let tab = &mut self.tabs[self.active_tab];
        tab.doc = Some(doc);
        tab.layout = RenderedDocument::new();
        tab.scroll = 0;
        tab.hscroll = 0;
        tab.focused_link = None;

        self.remember(&path);
        self.reveal_active_document();
    }

    /// The layout for the current state and terminal size.
    pub fn frames(&self) -> layout::Frames {
        layout::compute(layout::LayoutInput {
            area: self.area,
            sidebar_visible: self.sidebar.visible,
            sidebar_width: self.sidebar.width,
            narrow_threshold: self.ui.narrow_threshold,
            tab_count: self.tabs.len(),
            always_show_tabs: self.ui.always_show_tabs,
            show_status_bar: self.ui.show_status_bar,
        })
    }

    /// Apply one action. The only place application state changes.
    pub fn update(&mut self, action: Action) {
        // Any deliberate action supersedes whatever the status bar was saying.
        if !matches!(action, Action::Resize(..)) {
            self.status.clear();
        }

        match action {
            // -- Lifecycle -----------------------------------------------
            Action::Quit => self.quit(),
            Action::ForceQuit(code) => {
                // Section 13: a signal gives no chance to ask, so unsaved text
                // is parked where the next run can offer it back.
                self.write_recovery_files();
                self.should_quit = true;
                self.exit_code = code;
            }
            Action::Suspend => self.should_suspend = true,
            Action::Resize(width, height) => {
                // The chosen width is deliberately not clamped here. Layout
                // clamps what it draws, so a temporary shrink of the terminal
                // narrows the sidebar on screen without destroying the width
                // the user picked; widening the terminal restores it.
                self.area = Rect::new(0, 0, width, height);
            }

            // -- Focus and chrome ----------------------------------------
            Action::FocusNext => self.cycle_focus(true),
            Action::FocusPrev => self.cycle_focus(false),
            Action::ToggleSidebar => self.toggle_sidebar(),
            Action::SidebarWiden => self.resize_sidebar(1),
            Action::SidebarNarrow => self.resize_sidebar(-1),
            Action::SetSidebarMode(mode) => {
                self.sidebar.mode = mode;
                if !self.sidebar.visible {
                    self.sidebar.visible = true;
                }
            }
            Action::ToggleMouse => self.toggle_mouse(),
            Action::ToggleHelp => self.toggle_help(),
            Action::Escape => self.escape(),
            Action::SyntaxReady => self.status.clear(),

            // -- Viewport scrolling --------------------------------------
            Action::ScrollLineDown => self.scroll_by(1),
            Action::ScrollLineUp => self.scroll_by(-1),
            Action::ScrollHalfPageDown => self.scroll_by(i64::from(self.page() / 2).max(1)),
            Action::ScrollHalfPageUp => self.scroll_by(-i64::from(self.page() / 2).max(1)),
            Action::ScrollPageDown => self.scroll_by(i64::from(self.page()).max(1)),
            Action::ScrollPageUp => self.scroll_by(-i64::from(self.page()).max(1)),
            Action::ScrollWheelDown => {
                self.scroll_by(i64::from(self.ui.mouse_scroll_lines).max(1));
            }
            Action::ScrollWheelUp => {
                self.scroll_by(-i64::from(self.ui.mouse_scroll_lines).max(1));
            }
            Action::ScrollTop => self.scroll_to_top(),
            Action::ScrollBottom => self.scroll_to_bottom(),
            Action::ScrollLeft => self.scroll_horizontally(-1),
            Action::ScrollRight => self.scroll_horizontally(1),
            Action::PrevHeading => self.scroll_to_heading(false),
            Action::NextHeading => self.scroll_to_heading(true),

            // -- Vault walking -------------------------------------------
            Action::VaultEntries(entries) => {
                for entry in &entries {
                    if !entry.is_dir && self.files.is_markdown(&entry.path) {
                        self.vault
                            .markdown
                            .push((entry.path.clone(), entry.mtime, entry.size));
                    }
                }
                self.vault.tree.insert_all(entries);
                // A document opened before its row existed still gets its
                // path expanded, as soon as the batch carrying it lands.
                self.reveal_active_document();
            }
            Action::VaultWalkFinished(total) => {
                self.vault.tree.complete = true;
                self.vault.tree.entries = total;
                self.reveal_active_document();
                self.start_indexing();
            }
            Action::IndexBatch(entries) => {
                for (path, entry) in entries {
                    self.vault.index.insert(path, entry);
                }
            }
            Action::IndexFinished => self.finish_indexing(),
            Action::FilesChanged(paths) => self.files_changed(&paths),
            Action::FilesRemoved(paths) => self.files_removed(&paths),
            Action::WatchStopped(reason) => self.status.set(reason, Severity::Warning),
            Action::ReloadTheme => self.reload_theme(),
            Action::CycleTheme => self.cycle_theme(),
            Action::VaultWalkFailed(reason) => self.status.set(reason, Severity::Error),

            // -- Sidebar ---------------------------------------------------
            Action::SidebarDown => self.move_sidebar_selection(1),
            Action::SidebarUp => self.move_sidebar_selection(-1),
            Action::SidebarActivate => self.activate_sidebar_selection(),
            Action::SidebarBack => self.sidebar_back(),
            Action::TreeToggleHidden => {
                self.vault.tree.toggle_hidden();
                let state = if self.vault.tree.include_hidden {
                    "shown"
                } else {
                    "hidden"
                };
                self.status
                    .set(format!("Dotted entries {state}"), Severity::Info);
            }
            Action::TreeToggleAllFiles => {
                self.vault.tree.toggle_all_files();
                let state = if self.vault.tree.show_all {
                    "All files"
                } else {
                    "Markdown files only"
                };
                self.status.set(state, Severity::Info);
            }
            Action::TreeFilter if self.sidebar.mode == SidebarMode::Search => {
                self.open_prompt(PromptKind::ProjectSearch);
            }
            Action::TreeFilter => self.open_tree_filter(),
            Action::TreeFilterEdit(edit) => self.edit_tree_filter(edit),
            Action::TreeFilterAccept => self.sidebar.filter = None,
            Action::TreeFilterCancel => {
                self.sidebar.filter = None;
                self.vault.tree.set_filter(None);
            }
            Action::OpenPath(path) => self.open_path(&path),

            // -- Project search and the quick switcher ----------------------
            Action::OpenProjectSearch => self.open_prompt(PromptKind::ProjectSearch),
            Action::PromptEdit(edit) => self.edit_prompt(edit),
            Action::PromptAccept => self.accept_prompt(),
            Action::SearchHits(hits) => {
                self.search.hits.extend(hits);
            }
            Action::SearchFinished { total, truncated } => {
                self.search.running = false;
                self.search.truncated = truncated;
                self.search.elapsed = self.search.started.map(|at| at.elapsed());
                let _ = total;
                self.search_handle = None;
            }
            Action::SearchFailed(error) => {
                self.search.running = false;
                self.search.error = Some(error);
                self.search_handle = None;
            }
            Action::OpenQuickSwitcher => self.open_switcher(),
            Action::SwitcherEdit(edit) => self.edit_switcher(edit),
            Action::SwitcherMove(delta) => self.move_switcher(delta),
            Action::SwitcherAccept { new_tab } => self.accept_switcher(new_tab),

            // -- Editing ---------------------------------------------------
            Action::EnterEditMode => self.enter_edit_mode(),
            Action::EditInput(key) => self.edit_input(key),
            Action::EditPaste(text) => self.edit_paste(&text),
            Action::Save => self.save(false),
            Action::Undo => self.edit_history(false),
            Action::Redo => self.edit_history(true),
            Action::ReloadDocument => self.reload_document(),
            Action::NewFile => self.open_prompt(PromptKind::NewFile),
            Action::RenameDocument => self.open_prompt(PromptKind::RenameDocument),
            Action::TreeRename => self.open_prompt(PromptKind::RenameTreeEntry),
            Action::OpenInExternalEditor => self.hand_to_external_editor(),
            Action::CopyDocumentPath => self.copy_document_path(),

            // -- Find in document -------------------------------------------
            Action::OpenFindInDocument => self.open_find(),
            Action::FindEdit(edit) => self.edit_find(edit),
            Action::FindNext => self.step_find(true),
            Action::FindPrev => self.step_find(false),
            Action::CloseFind => self.close_find(),

            // -- Tabs ------------------------------------------------------
            Action::NewTab => self.new_tab(),
            Action::CloseTab => self.close_tab(),
            Action::NextTab => self.switch_tab(1),
            Action::PrevTab => self.switch_tab(-1),

            // -- Links and history ----------------------------------------
            Action::NextLink => self.cycle_link(true),
            Action::PrevLink => self.cycle_link(false),
            Action::FollowLink => self.follow_focused_link(),
            Action::FollowLinkInNewTab => self.follow_focused_link_in_new_tab(),
            Action::HintMode => self.open_hint_mode(),
            Action::FollowHintedLink(index) => self.follow_link(index),
            Action::ChooseCandidate(index) => self.choose_candidate(index),
            Action::Confirm(key) => self.confirm(key),
            Action::HistoryBack => self.navigate_history(false),
            Action::HistoryForward => self.navigate_history(true),
        }
    }

    /// The viewport's height in rendered lines.
    fn page(&self) -> u16 {
        self.viewport_inner().height.max(1)
    }

    /// Scroll the active tab by a signed number of lines.
    fn scroll_by(&mut self, delta: i64) {
        let height = self.page();
        let renderer = self.renderer(self.viewport_inner().width);
        let tab = &mut self.tabs[self.active_tab];

        let Some(doc) = &tab.doc else { return };

        let wanted = (tab.scroll as i64 + delta).max(0) as usize;

        // Scrolling down past what has been measured is what triggers the next
        // chunk of measurement; scrolling up never needs any.
        if delta > 0 {
            tab.layout.window(doc, &renderer, wanted, height);
        }

        tab.scroll = match tab.max_scroll(height) {
            Some(max) => wanted.min(max),
            // Not fully measured yet: allow the move, and the clamp lands once
            // the total is known rather than blocking the scroll now.
            None => wanted.min(tab.layout.measured_lines()),
        };
    }

    /// Jump to the top of the document.
    fn scroll_to_top(&mut self) {
        self.tabs[self.active_tab].scroll = 0;
    }

    /// Jump to the bottom, measuring the rest of the document to find it.
    fn scroll_to_bottom(&mut self) {
        let height = self.page();
        let renderer = self.renderer(self.viewport_inner().width);
        let tab = &mut self.tabs[self.active_tab];

        let Some(doc) = &tab.doc else { return };

        // Measured in chunks, so a jump to the end of a very large document
        // costs several frames rather than one long freeze.
        if !tab.layout.resolve_all(doc, &renderer) {
            self.status.set("Measuring the document…", Severity::Info);
        }

        if let Some(max) = tab.max_scroll(height) {
            tab.scroll = max;
        } else {
            tab.scroll = tab.layout.measured_lines();
        }
    }

    /// Scroll a clipped code block or wide table sideways.
    fn scroll_horizontally(&mut self, delta: i16) {
        let tab = &mut self.tabs[self.active_tab];
        if tab.doc.is_none() {
            return;
        }
        tab.hscroll =
            (i32::from(tab.hscroll) + i32::from(delta)).clamp(0, i32::from(u16::MAX)) as u16;
    }

    /// Scroll to the next or previous heading.
    fn scroll_to_heading(&mut self, forward: bool) {
        let renderer = self.renderer(self.viewport_inner().width);
        let tab = &mut self.tabs[self.active_tab];

        let Some(doc) = &tab.doc else { return };

        let offsets: Vec<usize> = doc.outline.iter().map(|h| h.offset).collect();
        let current = tab.scroll;

        let mut target = None;
        for offset in offsets {
            let Some(line) = tab.layout.line_of_offset(doc, &renderer, offset) else {
                continue;
            };
            if forward && line > current {
                target = Some(line);
                break;
            }
            if !forward && line < current {
                target = Some(line);
            }
        }

        if let Some(line) = target {
            tab.scroll = line;
        }
    }

    /// Expand the selected directory, or open the selected file.
    fn tree_expand_or_open(&mut self) {
        if self.vault.tree.expand_selected() {
            return;
        }

        let Some(node) = self.vault.tree.selected() else {
            return;
        };
        let path = node.path.clone();
        self.open_path(&path);
    }

    /// Open a path from the tree, relative to the vault root.
    ///
    /// A file perga does not render is handed to the desktop opener rather
    /// than shown as mojibake.
    fn open_path(&mut self, relative: &Path) {
        let absolute = self.vault.absolute(relative);

        if !self.files.is_markdown(&absolute) {
            match crate::vault::open_external(&absolute) {
                Ok(()) => self.status.set(
                    format!("Opened {} externally", relative.display()),
                    Severity::Info,
                ),
                Err(e) => self
                    .status
                    .set(format!("Cannot open externally: {e}"), Severity::Error),
            }
            return;
        }

        match Document::load(&absolute) {
            Ok(document) => {
                self.open(document);
                // A sidebar drawn over the viewport is in the way of the
                // document it has just opened.
                if self.sidebar.visible
                    && self.frames().sidebar_placement == SidebarPlacement::Overlaid
                {
                    self.sidebar.visible = false;
                    self.focus = Focus::Viewport;
                }
            }
            Err(e) => self.status.set(
                format!("Cannot read {}: {e}", relative.display()),
                Severity::Error,
            ),
        }
    }

    /// Expand the tree down to the active document and select it.
    fn reveal_active_document(&mut self) {
        if let Some(path) = self.active_path() {
            self.vault.tree.reveal(&path);
        }
    }

    /// Open the tree filter line, pre-filled with whatever filter is applied.
    fn open_tree_filter(&mut self) {
        let existing = self.vault.tree.filter().unwrap_or_default().to_string();
        self.sidebar.filter = Some(TextInput::with_value(existing));
        self.sidebar.visible = true;
        self.focus = Focus::Sidebar;
    }

    /// Apply one edit to the filter line, filtering as the user types.
    fn edit_tree_filter(&mut self, edit: TextEdit) {
        let Some(input) = &mut self.sidebar.filter else {
            return;
        };

        input.apply(edit);
        let value = input.value().to_string();
        self.vault.tree.set_filter(Some(value));
    }

    // -- Links and history -------------------------------------------------

    /// Where the active tab is, for the history stack.
    fn location(&self) -> Option<Location> {
        let tab = self.tab();
        let doc = tab.doc.as_ref()?;
        Some(Location {
            path: doc.path.clone(),
            anchor: None,
            scroll: tab.scroll,
        })
    }

    /// Move the link focus, bringing the newly focused link into view.
    fn cycle_link(&mut self, forward: bool) {
        let Some(doc) = self.tab().doc.as_ref() else {
            return;
        };
        let count = doc.links.len();

        if count == 0 {
            self.status.set("No links in this document", Severity::Info);
            return;
        }

        let next = match self.tab().focused_link {
            Some(at) if forward => (at + 1) % count,
            Some(at) => (at + count - 1) % count,
            // Starting from nothing, `n` takes the first link and `N` the last.
            None if forward => 0,
            None => count - 1,
        };

        self.tabs[self.active_tab].focused_link = Some(next);
        self.scroll_focused_link_into_view();
    }

    /// Scroll just enough to put the focused link on screen.
    fn scroll_focused_link_into_view(&mut self) {
        let height = usize::from(self.page());
        let renderer = self.renderer(self.viewport_inner().width);
        let tab = &mut self.tabs[self.active_tab];

        let Some(doc) = &tab.doc else { return };
        let Some(index) = tab.focused_link else {
            return;
        };
        let Some(link) = doc.links.get(index) else {
            return;
        };

        let Some(line) = tab.layout.line_of_offset(doc, &renderer, link.range.start) else {
            return;
        };

        // Only move when the link is off screen: cycling through links in a
        // paragraph that is already visible should not shift the page.
        if line < tab.scroll {
            tab.scroll = line;
        } else if line >= tab.scroll + height {
            tab.scroll = line.saturating_sub(height.saturating_sub(1));
        }
    }

    /// Resolve and act on the focused link.
    pub fn follow_focused_link(&mut self) {
        let Some(index) = self.tab().focused_link else {
            self.status
                .set("No link focused; press `n` or `f`", Severity::Info);
            return;
        };
        self.follow_link(index);
    }

    /// Resolve and act on one of the active document's links.
    pub fn follow_link(&mut self, index: usize) {
        let Some(doc) = self.tab().doc.as_ref() else {
            return;
        };
        let Some(link) = doc.links.get(index) else {
            return;
        };

        let target = link.target.clone();

        if link.kind == crate::doc::links::LinkKind::Wiki {
            self.follow_wiki_link(&target);
            return;
        }

        let resolved = links::resolve(&target, doc.dir(), &self.vault.root, &self.files);

        match resolved {
            links::Resolved::Document { path, anchor } => self.navigate_to(&path, anchor),
            // An anchor in the current document scrolls; it does not reload.
            links::Resolved::Anchor { slug } => self.jump_to_anchor(&slug),
            links::Resolved::Directory { path } => self.reveal_directory(&path),
            links::Resolved::External { url } => self.open_externally(Path::new(&url), &url),
            links::Resolved::Other { path } => {
                let shown = path.display().to_string();
                self.open_externally(&path, &shown);
            }
            links::Resolved::Broken { target } => self
                .status
                .set(format!("Cannot resolve: {target}"), Severity::Error),
        }
    }

    /// Open a document, recording where the reader came from.
    fn navigate_to(&mut self, path: &Path, anchor: Option<String>) {
        let from = self.location();

        match Document::load(path) {
            Ok(document) => {
                if let Some(from) = from {
                    self.tabs[self.active_tab].history.push(from);
                }
                self.open(document);
                if let Some(slug) = anchor {
                    self.jump_to_anchor(&slug);
                }
            }
            Err(e) => self.status.set(
                format!("Cannot read {}: {e}", path.display()),
                Severity::Error,
            ),
        }
    }

    /// Scroll to a heading in the active document.
    fn jump_to_anchor(&mut self, slug: &str) {
        let renderer = self.renderer(self.viewport_inner().width);
        let tab = &mut self.tabs[self.active_tab];

        let Some(doc) = &tab.doc else { return };
        let Some(offset) = doc.heading(slug).map(|h| h.offset) else {
            self.status
                .set(format!("No heading `{slug}`"), Severity::Warning);
            return;
        };

        if let Some(line) = tab.layout.line_of_offset(doc, &renderer, offset) {
            tab.scroll = line;
        }
    }

    /// Show a directory in the tree rather than trying to open it.
    fn reveal_directory(&mut self, path: &Path) {
        let Some(relative) = self.vault.relative(path).map(Path::to_path_buf) else {
            self.status.set(
                format!("{} is outside the vault", path.display()),
                Severity::Warning,
            );
            return;
        };

        if !self.vault.tree.reveal(&relative) {
            // The walk may not have reached it yet, which is not an error.
            self.status.set(
                format!("{} is not in the tree yet", relative.display()),
                Severity::Info,
            );
            return;
        }

        self.sidebar.mode = SidebarMode::Files;
        self.sidebar.visible = true;
        self.focus = Focus::Sidebar;
    }

    /// Hand something to the desktop opener, reporting either way.
    fn open_externally(&mut self, path: &Path, shown: &str) {
        match crate::vault::open_external(path) {
            Ok(()) => self.status.set(format!("Opened {shown}"), Severity::Info),
            // Without an opener the URL is at least readable, and `y` copies it.
            Err(e) => self
                .status
                .set(format!("Cannot open {shown}: {e}"), Severity::Error),
        }
    }

    /// Step back or forward through the active tab's history.
    fn navigate_history(&mut self, forward: bool) {
        let Some(current) = self.location() else {
            return;
        };

        let index = self.active_tab;
        let step = if forward {
            self.tabs[index].history.forward(current)
        } else {
            self.tabs[index].history.back(current)
        };

        let Some(target) = step else {
            let direction = if forward { "forward" } else { "back" };
            self.status
                .set(format!("No further {direction}"), Severity::Info);
            return;
        };

        // Already here: an anchor jump within one document is a history entry
        // whose path has not changed, so it must not cost a reload.
        let same = self
            .tab()
            .doc
            .as_ref()
            .is_some_and(|d| d.path == target.path);

        if !same {
            match Document::load(&target.path) {
                Ok(document) => self.open(document),
                Err(e) => {
                    self.status.set(
                        format!("Cannot read {}: {e}", target.path.display()),
                        Severity::Error,
                    );
                    return;
                }
            }
        }

        self.restore_scroll(target.scroll);
    }

    /// Put the viewport back at an exact scroll offset.
    ///
    /// Going back restores where the reader was, not the top of the document,
    /// which means measuring far enough down to know the offset is reachable.
    fn restore_scroll(&mut self, scroll: usize) {
        let height = self.page();
        let renderer = self.renderer(self.viewport_inner().width);
        let tab = &mut self.tabs[self.active_tab];

        let Some(doc) = &tab.doc else { return };

        tab.layout.window(doc, &renderer, scroll, height);
        tab.scroll = match tab.max_scroll(height) {
            Some(max) => scroll.min(max),
            None => scroll.min(tab.layout.measured_lines()),
        };
    }

    /// Label every link in view and wait for the label to be typed.
    fn open_hint_mode(&mut self) {
        let Some(doc) = self.tab().doc.as_ref() else {
            return;
        };
        if doc.links.is_empty() {
            self.status.set("No links in this document", Severity::Info);
            return;
        }

        let visible = self.visible_links();
        if visible.is_empty() {
            self.status.set("No links in view", Severity::Info);
            return;
        }

        self.overlay = Some(Overlay::Hints {
            links: visible,
            typed: String::new(),
        });
        self.focus = Focus::Overlay;
    }

    /// The indices of the links inside the current viewport, in reading order.
    pub fn visible_links(&self) -> Vec<usize> {
        let Some(doc) = self.tab().doc.as_ref() else {
            return Vec::new();
        };

        let tab = self.tab();
        let height = usize::from(self.page());
        let map = tab.layout.line_map();
        let window = tab.scroll..tab.scroll + height;

        doc.links
            .iter()
            .enumerate()
            .filter(|(_, link)| {
                map.line_of_offset(link.range.start)
                    .is_some_and(|line| window.contains(&line))
            })
            .map(|(index, _)| index)
            .collect()
    }

    // -- Edit mode -----------------------------------------------------------

    /// Enter edit mode on the active document.
    ///
    /// The cursor lands on the source line the reader was looking at, so
    /// crossing the boundary does not lose their place.
    fn enter_edit_mode(&mut self) {
        let Some(doc) = self.tab().doc.as_ref() else {
            return;
        };

        if let Some(reason) = doc.read_only {
            self.status.set(reason.message(), Severity::Warning);
            return;
        }

        let line = self.source_line_at_scroll();
        let mut editor = EditorState::new(doc);

        editor
            .textarea
            .move_cursor(tui_textarea::CursorMove::Jump(line as u16, 0));

        let index = self.active_tab;
        self.tabs[index].editor = Some(editor);
        self.tabs[index].mode = TabMode::Edit;
        // Section 7.3: edit mode locks focus to the viewport.
        self.focus = Focus::Viewport;

        if let Some(text) = self.pending_recovery() {
            let path = self.tab().doc.as_ref().map(|d| d.path.clone());
            if let Some(path) = path {
                self.overlay = Some(Overlay::Confirm {
                    question: format!(
                        "Unsaved text for {} was left behind. Restore it?",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    choices: vec![('y', "restore".into()), ('n', "discard".into())],
                    action: ConfirmAction::RestoreRecovery { path },
                });
                self.focus = Focus::Overlay;
                let _ = text;
            }
        }
    }

    /// Unsaved text an earlier run left behind for the active document.
    fn pending_recovery(&self) -> Option<String> {
        let doc = self.tab().doc.as_ref()?;
        buffer::read_recovery(&self.vault.root, &doc.path)
    }

    /// The zero-based source line the top of the viewport is showing.
    fn source_line_at_scroll(&self) -> usize {
        let tab = self.tab();
        let Some(doc) = tab.doc.as_ref() else {
            return 0;
        };

        let Some(offset) = tab.layout.line_map().offset_of_line(tab.scroll) else {
            return 0;
        };

        doc.source[..offset.min(doc.source.len())]
            .matches('\n')
            .count()
    }

    /// Leave edit mode, asking about unsaved edits first.
    fn leave_edit_mode(&mut self) {
        if self.tab().mode != TabMode::Edit {
            return;
        }

        if self.tab().dirty {
            self.overlay = Some(Overlay::Confirm {
                question: "This document has unsaved edits.".to_string(),
                choices: vec![
                    ('s', "save".into()),
                    ('d', "discard".into()),
                    ('c', "cancel".into()),
                ],
                action: ConfirmAction::LeaveEditMode,
            });
            self.focus = Focus::Overlay;
            return;
        }

        self.finish_edit_mode();
    }

    /// Drop the buffer and go back to reading, at the line the cursor was on.
    fn finish_edit_mode(&mut self) {
        let index = self.active_tab;
        let cursor = self.tabs[index].editor.as_ref().map(|e| e.cursor().0);

        self.tabs[index].editor = None;
        self.tabs[index].mode = TabMode::Read;
        self.tabs[index].dirty = false;
        self.focus = Focus::Viewport;

        if let Some(line) = cursor {
            self.scroll_to_source_line(line);
        }
    }

    /// Put the rendered view where a source line is.
    fn scroll_to_source_line(&mut self, line: usize) {
        let Some(offset) = self
            .tab()
            .doc
            .as_ref()
            .map(|doc| offset_of_source_line(&doc.source, line))
        else {
            return;
        };

        let renderer = self.renderer(self.viewport_inner().width);
        let height = usize::from(self.page());
        let tab = &mut self.tabs[self.active_tab];

        let Some(doc) = &tab.doc else { return };
        let Some(rendered) = tab.layout.line_of_offset(doc, &renderer, offset) else {
            return;
        };

        // Placed a third of the way down rather than at the very top: the
        // reader was looking at the line, not at the line above it.
        tab.scroll = rendered.saturating_sub(height / 3);
    }

    /// Hand a key press to the text area.
    fn edit_input(&mut self, key: crossterm::event::KeyEvent) {
        let index = self.active_tab;
        let Some(editor) = &mut self.tabs[index].editor else {
            return;
        };

        editor.textarea.input(key);
        self.tabs[index].dirty = self.tabs[index]
            .editor
            .as_ref()
            .is_some_and(EditorState::is_dirty);
    }

    /// Insert pasted text as a single undo step.
    fn edit_paste(&mut self, text: &str) {
        let index = self.active_tab;
        let Some(editor) = &mut self.tabs[index].editor else {
            return;
        };

        // One `insert_str` rather than a character at a time: 5,000 lines
        // pasted must be one undo, not five thousand.
        editor.textarea.insert_str(text);
        self.tabs[index].dirty = self.tabs[index]
            .editor
            .as_ref()
            .is_some_and(EditorState::is_dirty);
    }

    /// Undo or redo in the active buffer.
    fn edit_history(&mut self, redo: bool) {
        let index = self.active_tab;
        let Some(editor) = &mut self.tabs[index].editor else {
            return;
        };

        if redo {
            editor.textarea.redo();
        } else {
            editor.textarea.undo();
        }

        self.tabs[index].dirty = self.tabs[index]
            .editor
            .as_ref()
            .is_some_and(EditorState::is_dirty);
    }

    /// Write the active buffer to disk.
    pub fn save(&mut self, force: bool) {
        let index = self.active_tab;

        let Some(editor) = &self.tabs[index].editor else {
            return;
        };
        let Some(doc) = &self.tabs[index].doc else {
            return;
        };

        let path = doc.path.clone();
        let contents = editor.contents();
        let expected = (!force).then_some(editor.known_mtime);

        match buffer::save(&path, &contents, expected) {
            Ok(mtime) => {
                // Recorded before anything else: the watcher will report this
                // write, and without the record it would reload the document
                // on top of the buffer that produced it.
                self.own_writes.record(path.clone(), mtime);

                if let Some(editor) = &mut self.tabs[index].editor {
                    editor.mark_saved(mtime);
                }
                self.tabs[index].dirty = false;

                // The document is re-parsed so the outline, the links, and the
                // rendered blocks all follow the text that was just written.
                self.reload_from(&path, &contents, mtime);
                buffer::clear_recovery(&self.vault.root, &path);

                if let Some(relative) = self.vault.relative(&path).map(Path::to_path_buf) {
                    self.reindex(&relative);
                }

                self.status.set("Saved", Severity::Info);
            }
            Err(SaveError::Conflict { .. }) => {
                self.overlay = Some(Overlay::Confirm {
                    question: format!(
                        "{} changed on disk since it was opened.",
                        path.file_name().unwrap_or_default().to_string_lossy()
                    ),
                    choices: vec![
                        ('o', "overwrite".into()),
                        ('r', "reload and discard".into()),
                        ('c', "cancel".into()),
                    ],
                    action: ConfirmAction::OverwriteChanged,
                });
                self.focus = Focus::Overlay;
            }
            Err(e) => self
                .status
                .set(format!("Cannot save: {e}"), Severity::Error),
        }
    }

    /// Replace the active document with freshly parsed text.
    fn reload_from(&mut self, path: &Path, source: &str, mtime: SystemTime) {
        let index = self.active_tab;
        let Some(doc) = &self.tabs[index].doc else {
            return;
        };

        let parsed = Document::from_source(
            path.to_path_buf(),
            source.to_string(),
            mtime,
            doc.had_bom,
            doc.read_only,
        );

        self.tabs[index].doc = Some(parsed);
        // The layout keys its cache by content hash, so the blocks that did
        // not change are still hits; only the edited ones re-render.
        self.tabs[index].layout = RenderedDocument::new();
    }

    /// Re-read the active document from disk.
    fn reload_document(&mut self) {
        let Some(path) = self.tab().doc.as_ref().map(|doc| doc.path.clone()) else {
            return;
        };

        if self.tab().dirty {
            self.status.set(
                "This document has unsaved edits; save or discard first",
                Severity::Warning,
            );
            return;
        }

        let scroll = self.tab().scroll;

        match Document::load(&path) {
            Ok(document) => {
                self.open(document);
                // Reloading is not navigating: the reader stays where they
                // were, as closely as the new text allows.
                self.restore_scroll(scroll);
                self.status.set("Reloaded", Severity::Info);
            }
            Err(e) => self.status.set(
                format!("Cannot reload {}: {e}", path.display()),
                Severity::Error,
            ),
        }
    }

    /// Put the active document's path on the clipboard.
    fn copy_document_path(&mut self) {
        let Some(doc) = self.tab().doc.as_ref() else {
            return;
        };

        // The path relative to the vault, which is what a reader pasting it
        // into a note or a message means by "the path".
        let shown = doc.display_path(&self.vault.root);

        match crate::terminal::copy_to_clipboard(&shown) {
            // OSC 52 has no reply, and several emulators disable it, so this
            // says what was sent rather than claiming it arrived.
            Ok(()) => self.status.set(format!("Copied {shown}"), Severity::Info),
            Err(e) => self
                .status
                .set(format!("Cannot copy: {e}"), Severity::Error),
        }
    }

    /// Ask the event loop to hand the active document to `$EDITOR`.
    ///
    /// The loop does the work, because putting the terminal back afterwards is
    /// its job and `App` is deliberately terminal-free.
    fn hand_to_external_editor(&mut self) {
        let Some(path) = self.tab().doc.as_ref().map(|doc| doc.path.clone()) else {
            return;
        };

        if self.tab().dirty {
            self.status.set(
                "Save or discard this buffer before handing it to $EDITOR",
                Severity::Warning,
            );
            return;
        }

        if self.external_command().is_none() {
            self.status.set(
                "No editor: set $EDITOR or editor.external_command",
                Severity::Error,
            );
            return;
        }

        self.external_edit = Some(path);
    }

    /// The command `o` runs, from configuration or the environment.
    pub fn external_command(&self) -> Option<String> {
        let configured = self.editor_config.external_command.trim();
        if !configured.is_empty() {
            return Some(configured.to_string());
        }

        std::env::var("EDITOR")
            .ok()
            .or_else(|| std::env::var("VISUAL").ok())
            .filter(|command| !command.trim().is_empty())
    }

    // -- Creating and renaming ----------------------------------------------

    /// Create a file at the path typed into the prompt.
    fn create_file(&mut self, typed: &str) {
        let relative_to = self
            .active_path()
            .and_then(|path| path.parent().map(Path::to_path_buf))
            .map_or_else(|| self.vault.root.clone(), |dir| self.vault.absolute(&dir));

        let resolved =
            crate::editor::resolve_new_path(typed, &relative_to, &self.vault.root, &self.files);

        let path = match resolved {
            Ok(path) => path,
            Err(e) => {
                self.status.set(e.to_string(), Severity::Error);
                return;
            }
        };

        self.create_and_open(&path);
    }

    /// Create a file, open it in a new tab, and start editing it.
    pub fn create_and_open(&mut self, path: &Path) {
        let contents = if self.editor_config.new_file_frontmatter {
            crate::editor::frontmatter_stub(path)
        } else {
            String::new()
        };

        if let Err(e) = crate::editor::create(path, &contents) {
            self.status
                .set(format!("Cannot create the file: {e}"), Severity::Error);
            return;
        }

        match Document::load(path) {
            Ok(document) => {
                // A new note goes in its own tab: creating one is not the
                // same as leaving whatever was being read. A tab showing the
                // welcome screen is the exception — it is already empty.
                let wants_tab = self.tab().doc.is_some();

                if wants_tab && self.open_in_background_tab(document.clone()) {
                    self.active_tab = self.tabs.len() - 1;
                } else {
                    self.open(document);
                }

                if let Some(relative) = self.vault.relative(path).map(Path::to_path_buf) {
                    self.vault.tree.insert_all([crate::vault::walker::Entry {
                        path: relative.clone(),
                        is_dir: false,
                        mtime: None,
                        size: 0,
                    }]);
                    self.vault.markdown.push((relative.clone(), None, 0));
                    self.reindex(&relative);
                    self.vault.tree.reveal(&relative);
                }

                self.update(Action::EnterEditMode);
                self.status
                    .set(format!("Created {}", path.display()), Severity::Info);
            }
            Err(e) => self.status.set(
                format!("Created but cannot open {}: {e}", path.display()),
                Severity::Error,
            ),
        }
    }

    /// Rename a file, following every tab that had it open.
    fn rename(&mut self, existing: &Path, typed: &str) {
        let resolved = crate::editor::resolve_rename(typed, existing, &self.vault.root);

        let target = match resolved {
            Ok(path) => path,
            Err(e) => {
                self.status.set(e.to_string(), Severity::Error);
                return;
            }
        };

        if target == existing {
            return;
        }

        if let Err(e) = std::fs::rename(existing, &target) {
            self.status
                .set(format!("Cannot rename: {e}"), Severity::Error);
            return;
        }

        // Every tab pointing at the old path follows it, so a rename does not
        // leave a tab reading a file that is no longer there.
        let mut followed = 0;
        for tab in &mut self.tabs {
            if let Some(doc) = &mut tab.doc {
                if doc.path == existing {
                    doc.path = target.clone();
                    followed += 1;
                }
            }
        }

        self.rebuild_after_rename(existing, &target);

        // Section 9.11: links elsewhere are deliberately *not* rewritten, so
        // the reader is told how many now point at nothing.
        let broken = self.count_inbound(existing);
        let name = target.file_name().unwrap_or_default().to_string_lossy();

        if broken > 0 {
            self.status.set(
                format!("Renamed to {name}; {broken} documents now link to the old name"),
                Severity::Warning,
            );
        } else {
            self.status.set(
                format!("Renamed to {name} ({followed} tabs)"),
                Severity::Info,
            );
        }
    }

    /// Move a renamed file in the tree and the index.
    fn rebuild_after_rename(&mut self, from: &Path, to: &Path) {
        let (Some(old), Some(new)) = (
            self.vault.relative(from).map(Path::to_path_buf),
            self.vault.relative(to).map(Path::to_path_buf),
        ) else {
            return;
        };

        self.vault.index.remove(&old);
        self.vault.markdown.retain(|(path, _, _)| path != &old);

        self.vault.tree.insert_all([crate::vault::walker::Entry {
            path: new.clone(),
            is_dir: false,
            mtime: None,
            size: 0,
        }]);
        self.vault.markdown.push((new.clone(), None, 0));
        self.vault.tree.remove(&old);
        self.reindex(&new);
        self.vault.tree.reveal(&new);
    }

    /// How many indexed documents link to a path.
    fn count_inbound(&self, path: &Path) -> usize {
        let Some(relative) = self.vault.relative(path) else {
            return 0;
        };
        self.vault.index.backlinks(relative, &self.wikilinks).len()
    }

    // -- Confirmations -------------------------------------------------------

    /// Quit, asking first when a tab has unsaved edits.
    fn quit(&mut self) {
        if !self.any_dirty() {
            self.should_quit = true;
            return;
        }

        let dirty = self.tabs.iter().filter(|tab| tab.dirty).count();
        self.overlay = Some(Overlay::Confirm {
            question: format!("{dirty} documents have unsaved edits. Quit anyway?"),
            choices: vec![('d', "discard and quit".into())],
            action: ConfirmAction::Quit,
        });
        self.focus = Focus::Overlay;
    }

    /// Answer the open confirmation.
    fn confirm(&mut self, key: char) {
        let Some(Overlay::Confirm { action, .. }) = self.overlay.clone() else {
            return;
        };

        match (&action, key) {
            (ConfirmAction::LeaveEditMode, 's') => {
                self.close_overlay();
                self.save(false);
                // A save that hit a conflict has opened its own overlay; only
                // a clean save leaves edit mode.
                if self.overlay.is_none() && !self.tab().dirty {
                    self.finish_edit_mode();
                }
            }
            (ConfirmAction::LeaveEditMode, 'd') => {
                self.close_overlay();
                self.finish_edit_mode();
            }
            (ConfirmAction::Quit, 'd') => {
                self.close_overlay();
                self.should_quit = true;
            }
            (ConfirmAction::OverwriteChanged, 'o') => {
                self.close_overlay();
                self.save(true);
            }
            (ConfirmAction::OverwriteChanged, 'r') => {
                self.close_overlay();
                self.tabs[self.active_tab].dirty = false;
                self.finish_edit_mode();
                self.reload_document();
            }
            (ConfirmAction::CreatePage { path }, 'y') => {
                let path = path.clone();
                self.close_overlay();
                self.create_and_open(&path);
            }
            (ConfirmAction::RestoreRecovery { path }, 'y') => {
                let path = path.clone();
                self.close_overlay();
                self.restore_recovery(&path);
            }
            (ConfirmAction::RestoreRecovery { path }, 'n') => {
                let path = path.clone();
                self.close_overlay();
                buffer::clear_recovery(&self.vault.root, &path);
            }
            // Anything else — `c`, `n`, `Esc` — leaves things as they were.
            _ => self.close_overlay(),
        }
    }

    /// Put recovered text into the buffer.
    fn restore_recovery(&mut self, path: &Path) {
        let Some(text) = buffer::read_recovery(&self.vault.root, path) else {
            return;
        };

        let index = self.active_tab;
        if let Some(editor) = &mut self.tabs[index].editor {
            editor.textarea.select_all();
            editor.textarea.insert_str(text);
            self.tabs[index].dirty = true;
        }

        buffer::clear_recovery(&self.vault.root, path);
        self.status.set("Restored unsaved text", Severity::Info);
    }

    /// Park every dirty buffer where it can be offered back.
    ///
    /// Called on the way out when a signal arrives: the process is about to
    /// end and there is nowhere to ask the reader what they want.
    pub fn write_recovery_files(&self) {
        for tab in &self.tabs {
            let (Some(doc), Some(editor)) = (&tab.doc, &tab.editor) else {
                continue;
            };
            if !editor.is_dirty() {
                continue;
            }

            if let Err(e) = buffer::write_recovery(&self.vault.root, &doc.path, &editor.text()) {
                tracing::warn!("cannot write a recovery file: {e}");
            }
        }
    }

    /// Whether any tab has unsaved edits.
    pub fn any_dirty(&self) -> bool {
        self.tabs.iter().any(|tab| tab.dirty)
    }

    // -- Prompts -------------------------------------------------------------

    /// Open a prompt, pre-filled where that is the useful thing to do.
    fn open_prompt(&mut self, kind: PromptKind) {
        let prefill = match kind {
            // Re-running a search with one word changed is far more common
            // than starting from nothing.
            PromptKind::ProjectSearch => self.search.query.clone(),
            // A new file starts beside where the reader is, so the common
            // case is typing a name and pressing Enter.
            PromptKind::NewFile => self.new_file_prefix(),
            PromptKind::RenameDocument => self
                .tab()
                .doc
                .as_ref()
                .and_then(|doc| doc.path.file_name())
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default(),
            PromptKind::RenameTreeEntry => self
                .vault
                .tree
                .selected()
                .map(|node| node.name.clone())
                .unwrap_or_default(),
        };

        self.overlay = Some(Overlay::Prompt {
            kind,
            input: TextInput::with_value(prefill),
        });
        self.focus = Focus::Overlay;
    }

    /// Apply one edit to the open prompt.
    fn edit_prompt(&mut self, edit: TextEdit) {
        if let Some(Overlay::Prompt { input, .. }) = &mut self.overlay {
            input.apply(edit);
        }
    }

    /// Act on what was typed into the prompt.
    fn accept_prompt(&mut self) {
        let Some(Overlay::Prompt { kind, input }) = self.overlay.clone() else {
            return;
        };

        self.close_overlay();

        match kind {
            PromptKind::ProjectSearch => self.start_search(input.value()),
            PromptKind::NewFile => self.create_file(input.value()),
            PromptKind::RenameDocument => {
                let path = self.tab().doc.as_ref().map(|doc| doc.path.clone());
                if let Some(path) = path {
                    self.rename(&path, input.value());
                }
            }
            PromptKind::RenameTreeEntry => {
                let path = self
                    .vault
                    .tree
                    .selected()
                    .map(|node| self.vault.absolute(&node.path));
                if let Some(path) = path {
                    self.rename(&path, input.value());
                }
            }
        }
    }

    /// What the new-file prompt opens pre-filled with.
    ///
    /// The directory the reader is in, so a bare name lands beside what they
    /// are looking at. The sidebar's selection wins when it has focus, because
    /// that is what they were pointing at.
    fn new_file_prefix(&self) -> String {
        let from_tree = (self.focus == Focus::Sidebar)
            .then(|| self.vault.tree.selected())
            .flatten()
            .map(|node| {
                if node.is_dir {
                    node.path.clone()
                } else {
                    node.path.parent().unwrap_or(Path::new("")).to_path_buf()
                }
            });

        let directory = from_tree.or_else(|| {
            self.active_path()
                .and_then(|path| path.parent().map(Path::to_path_buf))
        });

        match directory {
            Some(dir) if !dir.as_os_str().is_empty() => format!("{}/", dir.display()),
            _ => String::new(),
        }
    }

    // -- Project search ------------------------------------------------------

    /// Run a project-wide search, cancelling whatever was already running.
    pub fn start_search(&mut self, query: &str) {
        // Dropping the handle cancels the previous search, so a reader
        // re-running a query does not leave a thread reading the vault for the
        // query they have already replaced.
        self.search_handle = None;

        if query.trim().is_empty() {
            self.search = SearchState::default();
            return;
        }

        self.search.begin(query.to_string());
        self.sidebar.mode = SidebarMode::Search;
        self.sidebar.visible = true;

        let Some(sink) = self.search_sink.clone() else {
            // No sink means no event loop; the tests drive `search_now`.
            return;
        };

        let walk = WalkOptions {
            respect_gitignore: self.files.respect_gitignore,
            follow_symlinks: self.general.follow_symlinks,
        };

        self.search_handle = Some(content::spawn(
            self.vault.root.clone(),
            content::Query::parse(query, &self.search_config),
            self.search_config.clone(),
            self.files.clone(),
            walk,
            move |event| sink(event),
        ));
    }

    /// Run a project search synchronously, for tests and for the paths with no
    /// event loop behind them.
    pub fn search_now(&mut self, query: &str) {
        use std::sync::atomic::AtomicBool;
        use std::sync::Mutex;

        self.search.begin(query.to_string());
        self.sidebar.mode = SidebarMode::Search;

        let walk = WalkOptions {
            respect_gitignore: self.files.respect_gitignore,
            follow_symlinks: self.general.follow_symlinks,
        };
        let collected = Mutex::new(Vec::new());
        let outcome = Mutex::new((false, None));

        content::run(
            &self.vault.root,
            &content::Query::parse(query, &self.search_config),
            &self.search_config,
            &self.files,
            walk,
            &AtomicBool::new(false),
            &|event| match event {
                SearchEvent::Hits(hits) => collected.lock().unwrap().extend(hits),
                SearchEvent::Finished { truncated, .. } => {
                    outcome.lock().unwrap().0 = truncated;
                }
                SearchEvent::BadPattern(e) => outcome.lock().unwrap().1 = Some(e),
            },
        );

        self.search.hits = collected.into_inner().unwrap();
        let (truncated, error) = outcome.into_inner().unwrap();
        self.search.truncated = truncated;
        self.search.error = error;
        self.search.running = false;
        self.search.elapsed = self.search.started.map(|at| at.elapsed());
    }

    /// Open the search hit the sidebar has selected, scrolled to its line.
    fn open_selected_hit(&mut self) {
        let Some(hit) = self.search.hits.get(self.search.selected).cloned() else {
            return;
        };

        let absolute = self.vault.absolute(&hit.path);
        self.navigate_to(&absolute, None);

        // The line is one-based and the offset map is in bytes, so the line is
        // converted here rather than every caller doing it.
        if let Some(offset) = self.offset_of_line(hit.line) {
            self.scroll_to_offset(offset);
        }
    }

    /// The byte offset a one-based line begins at in the active document.
    fn offset_of_line(&self, line: u64) -> Option<usize> {
        let source = &self.tab().doc.as_ref()?.source;
        let wanted = line.saturating_sub(1) as usize;

        if wanted == 0 {
            return Some(0);
        }

        source
            .match_indices('\n')
            .nth(wanted - 1)
            .map(|(at, _)| at + 1)
    }

    // -- The quick switcher --------------------------------------------------

    /// Open the fuzzy switcher, showing the recent list until something is
    /// typed.
    fn open_switcher(&mut self) {
        let rows = self.switcher_rows("");

        self.overlay = Some(Overlay::Switcher {
            input: TextInput::new(),
            rows,
            selected: 0,
        });
        self.focus = Focus::Overlay;
    }

    /// Apply one edit to the switcher query and rescore.
    fn edit_switcher(&mut self, edit: TextEdit) {
        let Some(Overlay::Switcher { input, .. }) = &mut self.overlay else {
            return;
        };

        input.apply(edit);
        let query = input.value().to_string();
        let rows = self.switcher_rows(&query);

        if let Some(Overlay::Switcher {
            rows: slot,
            selected,
            ..
        }) = &mut self.overlay
        {
            *slot = rows;
            // The list under the cursor changed, so the cursor goes back to
            // the best match rather than to whatever is now in its old row.
            *selected = 0;
        }
    }

    /// The rows for a switcher query.
    fn switcher_rows(&mut self, query: &str) -> Vec<SwitcherRow> {
        if query.trim().is_empty() {
            // Section 8.3: an empty query shows the recent list, most recent
            // first, not an arbitrary slice of the vault.
            return self
                .recent
                .iter()
                .map(|path| SwitcherRow {
                    path: path.clone(),
                    indices: Vec::new(),
                    create: false,
                })
                .collect();
        }

        let paths: Vec<PathBuf> = self
            .vault
            .markdown
            .iter()
            .map(|(path, _, _)| path.clone())
            .collect();

        let mut rows: Vec<SwitcherRow> = self
            .fuzzy
            .search(&paths, query, SWITCHER_ROWS)
            .into_iter()
            .map(|candidate| SwitcherRow {
                path: candidate.path,
                indices: candidate.indices,
                create: false,
            })
            .collect();

        // A path that matches nothing is a note the reader has not written
        // yet, and the last row offers to write it.
        if rows.is_empty() {
            rows.push(SwitcherRow {
                path: PathBuf::from(with_markdown_extension(query, &self.files)),
                indices: Vec::new(),
                create: true,
            });
        }

        rows
    }

    /// Move the switcher selection, stopping at both ends.
    fn move_switcher(&mut self, delta: isize) {
        let Some(Overlay::Switcher { rows, selected, .. }) = &mut self.overlay else {
            return;
        };

        if rows.is_empty() {
            *selected = 0;
            return;
        }

        let last = rows.len() as isize - 1;
        *selected = (*selected as isize + delta).clamp(0, last) as usize;
    }

    /// Open whatever the switcher has selected.
    fn accept_switcher(&mut self, new_tab: bool) {
        let Some(Overlay::Switcher { rows, selected, .. }) = self.overlay.clone() else {
            return;
        };

        let Some(row) = rows.get(selected).cloned() else {
            return;
        };
        self.close_overlay();

        if row.create {
            // Creating the file is Section 9.11; until then the switcher says
            // what it would create rather than pretending it did.
            self.status.set(
                format!("`{}` does not exist yet", row.path.display()),
                Severity::Warning,
            );
            return;
        }

        let absolute = self.vault.absolute(&row.path);

        if new_tab {
            match Document::load(&absolute) {
                Ok(document) => {
                    self.open_in_background_tab(document);
                }
                Err(e) => self.status.set(
                    format!("Cannot read {}: {e}", row.path.display()),
                    Severity::Error,
                ),
            }
            return;
        }

        self.navigate_to(&absolute, None);
    }

    /// Record a document in the session's recent list.
    fn remember(&mut self, path: &Path) {
        let Some(relative) = self.vault.relative(path).map(Path::to_path_buf) else {
            return;
        };

        self.recent.retain(|seen| seen != &relative);
        self.recent.insert(0, relative);
        self.recent.truncate(MAX_RECENT);
    }

    // -- Session -------------------------------------------------------------

    /// Reopen the tabs, the sidebar, and the recent list from the last run.
    ///
    /// A tab whose file has gone since is dropped rather than opened empty: a
    /// session is a convenience, and reconstructing one badly is worse than
    /// opening on the welcome screen.
    pub fn restore_session(&mut self) {
        if !self.session_config.restore {
            return;
        }

        let Some(session) = crate::config::session::Session::load(&self.vault.root) else {
            return;
        };

        self.sidebar.visible = session.sidebar_visible;
        self.sidebar.width = session.sidebar_width;
        self.sidebar.mode = session.sidebar_mode;
        self.recent = session.recent;
        self.recent.truncate(self.session_config.max_recent);

        // Section 9.10: the last theme, when it was chosen at runtime. A
        // `--theme` on the command line is this run's choice and wins.
        if let Some(name) = session.theme.filter(|_| !self.theme_pinned) {
            if name != self.theme.name {
                let dir = self.theme_dir();
                let mut warnings = Vec::new();
                let theme = Theme::resolve(&name, dir.as_deref(), &mut warnings);

                if warnings.is_empty() {
                    self.theme_config.name = name;
                    self.apply_theme(theme);
                }
            }
        }

        let mut restored = Vec::new();
        for saved in &session.tabs {
            let absolute = self.vault.absolute(&saved.path);
            let Ok(document) = Document::load(&absolute) else {
                continue;
            };

            let mut tab = Tab::with_document(document);
            tab.scroll = saved.scroll;
            restored.push(tab);
        }

        if restored.is_empty() {
            return;
        }

        self.active_tab = session.active_tab.min(restored.len() - 1);
        self.tabs = restored;
    }

    /// Write the session for this vault.
    pub fn save_session(&self) {
        if !self.session_config.restore {
            return;
        }

        let session = crate::config::session::Session {
            version: crate::config::session::VERSION,
            tabs: self
                .tabs
                .iter()
                .filter_map(|tab| {
                    let doc = tab.doc.as_ref()?;
                    Some(crate::config::session::SavedTab {
                        path: self.vault.relative(&doc.path)?.to_path_buf(),
                        scroll: tab.scroll,
                    })
                })
                .collect(),
            active_tab: self.active_tab,
            sidebar_visible: self.sidebar.visible,
            sidebar_width: self.sidebar.width,
            sidebar_mode: self.sidebar.mode,
            theme: Some(self.theme.name.clone()),
            recent: self.recent.clone(),
        };

        // A session that cannot be written costs the next run its tabs and
        // nothing else, so it is logged rather than shown.
        if let Err(e) = session.save(&self.vault.root) {
            tracing::warn!("cannot write the session: {e}");
        }
    }

    // -- Live reload ---------------------------------------------------------

    /// Start watching the vault for changes.
    pub fn start_watch(&mut self, sink: impl Fn(WatchEvent) + Send + Sync + 'static) {
        if !self.watch_config.enabled {
            return;
        }

        self.watch = Some(crate::vault::watch::spawn(
            self.vault.root.clone(),
            std::time::Duration::from_millis(self.watch_config.debounce_ms),
            sink,
        ));
    }

    /// The directory user themes are read from.
    pub fn theme_dir(&self) -> Option<PathBuf> {
        if self.theme_config.dir.as_os_str().is_empty() {
            crate::config::user_theme_dir()
        } else {
            Some(self.theme_config.dir.clone())
        }
    }

    /// Watch the theme directory, so authoring a theme shows its effect.
    pub fn start_theme_watch(&mut self, sink: impl Fn(WatchEvent) + Send + Sync + 'static) {
        let Some(dir) = self.theme_dir().filter(|dir| dir.is_dir()) else {
            return;
        };

        self.theme_watch = Some(crate::vault::watch::spawn(
            dir,
            std::time::Duration::from_millis(self.watch_config.debounce_ms),
            sink,
        ));
    }

    /// Every theme that can be switched to, in the order `m t` visits them.
    ///
    /// The three built-ins first, then whatever is in the theme directory, so
    /// the order is stable and a reader who has written none still has three
    /// to cycle. `auto` is not a stop: it is a way of *choosing* a theme at
    /// startup, and offering it as a destination would be a stop whose meaning
    /// depends on the terminal.
    pub fn available_themes(&self) -> Vec<String> {
        let mut names: Vec<String> = crate::theme::builtin::ALL
            .iter()
            .map(|(name, _)| (*name).to_string())
            .collect();

        let Some(dir) = self.theme_dir() else {
            return names;
        };

        let Ok(entries) = std::fs::read_dir(dir) else {
            return names;
        };

        let mut mine: Vec<String> = entries
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|e| e == "toml"))
            .filter_map(|path| path.file_stem()?.to_str().map(str::to_string))
            .filter(|stem| !names.contains(stem))
            .collect();

        // Read order from a directory is not stable; the cycle must be.
        mine.sort();
        names.extend(mine);
        names
    }

    /// Switch to the next theme.
    fn cycle_theme(&mut self) {
        let names = self.available_themes();
        if names.is_empty() {
            return;
        }

        // Wherever the current theme sits in the list, the next one follows;
        // a theme that is not in the list at all starts the cycle over.
        let next = names
            .iter()
            .position(|name| *name == self.theme.name)
            .map_or(0, |at| (at + 1) % names.len());

        let wanted = names[next].clone();
        let dir = self.theme_dir();
        let mut warnings = Vec::new();
        let mut theme = Theme::resolve(&wanted, dir.as_deref(), &mut warnings);

        if let Some(warning) = warnings.first() {
            self.status.set(warning.clone(), Severity::Warning);
            return;
        }

        if theme.code_theme.is_none() {
            theme.code_theme = Some(self.theme_config.code_theme.clone());
        }

        // The watcher re-reads `theme_config.name`, so it has to follow the
        // switch or editing the showing theme would reload the previous one.
        self.theme_config.name = wanted;
        self.apply_theme(theme);

        self.status
            .set(format!("Theme: {}", self.theme.name), Severity::Info);
    }

    /// Install a theme and discard everything drawn with the old one.
    fn apply_theme(&mut self, theme: Theme) {
        self.theme = theme;

        // Every cached line carries the styles it was rendered with, so a new
        // theme means every block has to be drawn again.
        for tab in &mut self.tabs {
            tab.layout = RenderedDocument::new();
        }
    }

    /// Re-read the configured theme from disk.
    pub fn reload_theme(&mut self) {
        let dir = self.theme_dir();
        let mut warnings = Vec::new();

        let mut theme = Theme::resolve(&self.theme_config.name, dir.as_deref(), &mut warnings);
        if theme.code_theme.is_none() {
            theme.code_theme = Some(self.theme_config.code_theme.clone());
        }

        if let Some(warning) = warnings.first() {
            self.status.set(warning.clone(), Severity::Warning);
            return;
        }

        self.apply_theme(theme);

        self.status.set(
            format!("Theme reloaded: {}", self.theme.name),
            Severity::Info,
        );
    }

    /// Handle a debounced batch of changes.
    pub fn files_changed(&mut self, paths: &[PathBuf]) {
        for relative in paths {
            let absolute = self.vault.absolute(relative);
            let mtime = std::fs::metadata(&absolute).and_then(|m| m.modified()).ok();

            // A write perga made is not news; reloading on it would race the
            // dirty flag that same save has just cleared.
            if self.own_writes.claim(&absolute, mtime) {
                continue;
            }

            if !self.files.is_markdown(&absolute) {
                continue;
            }

            let size = std::fs::metadata(&absolute).map(|m| m.len()).unwrap_or(0);
            self.vault.tree.insert_all([crate::vault::walker::Entry {
                path: relative.clone(),
                is_dir: false,
                mtime,
                size,
            }]);
            self.reindex(relative);

            if self.active_path().as_deref() == Some(relative.as_path()) {
                self.active_document_changed();
            }
        }
    }

    /// Handle paths that are gone.
    pub fn files_removed(&mut self, paths: &[PathBuf]) {
        for relative in paths {
            self.vault.tree.remove(relative);
            self.vault.index.remove(relative);
            self.vault.markdown.retain(|(path, _, _)| path != relative);
        }
    }

    /// The document being read changed underneath the reader.
    fn active_document_changed(&mut self) {
        if self.tab().dirty {
            // Never clobber a buffer. The reader is told, and `r` is theirs to
            // press when they are ready.
            self.status
                .set("File changed on disk (r to reload)", Severity::Warning);
            return;
        }

        let scroll = self.tab().scroll;
        let focused = self.tab().focused_link;

        let Some(path) = self.tab().doc.as_ref().map(|doc| doc.path.clone()) else {
            return;
        };

        match Document::load(&path) {
            Ok(document) => {
                self.open(document);
                self.restore_scroll(scroll);
                // The link index may have moved under the reader, so it is
                // kept only where it still names a link.
                let count = self.tab().doc.as_ref().map_or(0, |doc| doc.links.len());
                self.tabs[self.active_tab].focused_link = focused.filter(|at| *at < count);
            }
            Err(e) => self.status.set(
                format!("Cannot reload {}: {e}", path.display()),
                Severity::Warning,
            ),
        }
    }

    // -- Sidebar dispatch ---------------------------------------------------

    /// Move the selection in whichever sidebar mode is showing.
    fn move_sidebar_selection(&mut self, delta: isize) {
        match self.sidebar.mode {
            SidebarMode::Files => self.vault.tree.move_selection(delta),
            SidebarMode::Outline => {
                let count = self.tab().doc.as_ref().map_or(0, |doc| doc.outline.len());
                if count == 0 {
                    return;
                }
                let at = self.sidebar.outline_selected.min(count - 1) as isize;
                self.sidebar.outline_selected = (at + delta).clamp(0, count as isize - 1) as usize;
            }
            SidebarMode::Search => self.search.move_selection(delta),
            // The links mode is a list to read, not one to walk.
            SidebarMode::Links => {}
        }
    }

    /// Activate the selection in whichever sidebar mode is showing.
    fn activate_sidebar_selection(&mut self) {
        match self.sidebar.mode {
            SidebarMode::Files => self.tree_expand_or_open(),
            SidebarMode::Outline => self.jump_to_selected_heading(),
            SidebarMode::Search => self.open_selected_hit(),
            SidebarMode::Links => {}
        }
    }

    /// Step back in whichever sidebar mode is showing.
    fn sidebar_back(&mut self) {
        if self.sidebar.mode == SidebarMode::Files {
            self.vault.tree.collapse_selected();
        }
    }

    // -- Outline ------------------------------------------------------------

    /// Which heading the reader is currently inside.
    ///
    /// The last heading whose rendered line is at or above the top of the
    /// viewport, which is what "the section I am reading" means when a section
    /// is taller than the screen.
    pub fn current_heading(&self) -> usize {
        let tab = self.tab();
        let Some(doc) = tab.doc.as_ref() else {
            return 0;
        };

        let map = tab.layout.line_map();
        let mut current = 0;

        for (index, heading) in doc.outline.iter().enumerate() {
            match map.line_of_offset(heading.offset) {
                Some(line) if line <= tab.scroll => current = index,
                _ => break,
            }
        }

        current
    }

    /// Scroll the viewport to the heading the outline has selected.
    fn jump_to_selected_heading(&mut self) {
        let at = self.sidebar.outline_selected;
        let slug = self
            .tab()
            .doc
            .as_ref()
            .and_then(|doc| doc.outline.get(at))
            .map(|heading| heading.slug.clone());

        // The same slug lookup anchors use: one implementation, so an anchor
        // and the outline can never disagree about where a heading is.
        if let Some(slug) = slug {
            self.jump_to_anchor(&slug);
        }
    }

    // -- Find in document ---------------------------------------------------

    /// Open the find bar, keeping whatever was searched for last.
    fn open_find(&mut self) {
        if self.tab().doc.is_none() {
            return;
        }

        let index = self.active_tab;
        let find = self.tabs[index].find.get_or_insert_with(FindState::new);
        // Reopening selects the whole query, in the sense that typing replaces
        // it: the common case is a new search, not an edit of the old one.
        find.input.apply(TextEdit::End);

        self.overlay = Some(Overlay::Find);
        self.focus = Focus::Overlay;
    }

    /// Apply one edit to the query and re-run the search.
    fn edit_find(&mut self, edit: TextEdit) {
        let index = self.active_tab;
        let Some(source) = self.tabs[index].doc.as_ref().map(|d| d.source.clone()) else {
            return;
        };
        let Some(find) = &mut self.tabs[index].find else {
            return;
        };

        find.input.apply(edit);
        find.refresh(&source);

        if let Some(offset) = find.current_offset() {
            self.scroll_to_offset(offset);
        }
    }

    /// Step to the next or previous match.
    fn step_find(&mut self, forward: bool) {
        let index = self.active_tab;
        let Some(find) = &mut self.tabs[index].find else {
            return;
        };

        let Some(offset) = find.step(forward) else {
            self.status.set("No matches", Severity::Info);
            return;
        };

        self.scroll_to_offset(offset);
    }

    /// Close the find bar and clear its highlighting.
    fn close_find(&mut self) {
        self.tabs[self.active_tab].find = None;
        if self.overlay == Some(Overlay::Find) {
            self.close_overlay();
        }
    }

    /// Bring a source offset into view, moving as little as possible.
    fn scroll_to_offset(&mut self, offset: usize) {
        let height = usize::from(self.page());
        let renderer = self.renderer(self.viewport_inner().width);
        let tab = &mut self.tabs[self.active_tab];

        let Some(doc) = &tab.doc else { return };
        let Some(line) = tab.layout.line_of_offset(doc, &renderer, offset) else {
            return;
        };

        if line < tab.scroll {
            tab.scroll = line;
        } else if line >= tab.scroll + height {
            tab.scroll = line.saturating_sub(height / 2);
        }
    }

    // -- Tabs --------------------------------------------------------------

    /// Open a new tab on the welcome screen and switch to it.
    fn new_tab(&mut self) {
        self.leave_active_tab();

        if self.tabs.len() >= MAX_TABS {
            self.status.set(
                format!("{MAX_TABS} tabs is the maximum; reusing this one"),
                Severity::Warning,
            );
            self.tabs[self.active_tab] = Tab::default();
            return;
        }

        self.tabs.push(Tab::default());
        self.active_tab = self.tabs.len() - 1;
        self.focus = Focus::Viewport;
    }

    /// Close the active tab, or quit when it is the last one.
    fn close_tab(&mut self) {
        if self.tabs.len() == 1 {
            self.update(Action::Quit);
            return;
        }

        self.leave_active_tab();

        self.tabs.remove(self.active_tab);
        // Closing a tab lands on the one to its left, which is where the eye
        // already is; closing the first lands on the new first.
        self.active_tab = self.active_tab.saturating_sub(1);
        self.reveal_active_document();
    }

    /// Move to the next or previous tab, wrapping at both ends.
    fn switch_tab(&mut self, delta: isize) {
        let count = self.tabs.len() as isize;
        if count <= 1 {
            return;
        }

        self.leave_active_tab();

        self.active_tab = (self.active_tab as isize + delta).rem_euclid(count) as usize;
        self.reveal_active_document();
    }

    /// Tidy up whatever belongs to the tab the reader is leaving.
    ///
    /// The find bar shows the *active* tab's state, so it does not follow the
    /// reader to a tab that was not searching for anything.
    fn leave_active_tab(&mut self) {
        if self.overlay == Some(Overlay::Find) {
            self.close_overlay();
        }
    }

    /// Open a document in a new tab without leaving the current one.
    ///
    /// Returns whether the tab was opened; at the cap it is not, and the
    /// caller decides what to do instead.
    fn open_in_background_tab(&mut self, document: Document) -> bool {
        if self.tabs.len() >= MAX_TABS {
            self.status
                .set(format!("{MAX_TABS} tabs is the maximum"), Severity::Warning);
            return false;
        }

        let label = document.label().to_string();
        let mut tab = Tab::with_document(document);
        // A background tab has never been drawn, so it has no measured layout
        // and starts at the top — which is where a freshly opened document is
        // anyway.
        tab.scroll = 0;

        self.tabs.push(tab);
        self.status
            .set(format!("Opened {label} in a new tab"), Severity::Info);
        true
    }

    /// Follow the focused link into a background tab.
    ///
    /// Only a document target makes sense here: an anchor has nowhere else to
    /// go, and a directory or an external URL is not a tab.
    fn follow_focused_link_in_new_tab(&mut self) {
        let Some(index) = self.tab().focused_link else {
            self.status
                .set("No link focused; press `n` or `f`", Severity::Info);
            return;
        };

        let Some(doc) = self.tab().doc.as_ref() else {
            return;
        };
        let Some(link) = doc.links.get(index) else {
            return;
        };

        let target = link.target.clone();

        if link.kind == crate::doc::links::LinkKind::Wiki {
            self.follow_wiki_link(&target);
            return;
        }

        let resolved = links::resolve(&target, doc.dir(), &self.vault.root, &self.files);

        match resolved {
            links::Resolved::Document { path, .. } => match Document::load(&path) {
                Ok(document) => {
                    self.open_in_background_tab(document);
                }
                Err(e) => self.status.set(
                    format!("Cannot read {}: {e}", path.display()),
                    Severity::Error,
                ),
            },
            // Everything else does what it would have done in this tab.
            _ => self.follow_link(index),
        }
    }

    /// Follow a `[[wiki-link]]`, through the index rather than the filesystem.
    fn follow_wiki_link(&mut self, target: &str) {
        if !self.wikilinks.enabled {
            self.status
                .set("Wiki-links are disabled", Severity::Warning);
            return;
        }

        if !self.vault.index.ready {
            // Resolving against a half-built index would send the reader
            // somewhere arbitrary and look like a bug in their notes.
            self.status.set("Still indexing…", Severity::Info);
            return;
        }

        let source = self.active_path().unwrap_or_default();

        match self
            .vault
            .index
            .resolve_wiki(target, &source, &self.wikilinks)
        {
            WikiResolution::Found { path, anchor } => {
                let absolute = self.vault.absolute(&path);
                self.navigate_to(&absolute, anchor);
            }
            WikiResolution::Ambiguous { candidates, anchor } => {
                self.overlay = Some(Overlay::Disambiguate {
                    page: target.to_string(),
                    candidates,
                    anchor,
                    selected: 0,
                });
                self.focus = Focus::Overlay;
            }
            // Section 9.11: a broken wiki-link is the one place perga offers
            // to create a file. A broken *inline* link never does.
            WikiResolution::Missing { page, .. } => self.offer_to_create(&page),
        }
    }

    /// Offer to create the page a wiki-link named but nothing answers to.
    fn offer_to_create(&mut self, page: &str) {
        let configured = &self.wikilinks.new_file_dir;
        let directory = if configured.as_os_str().is_empty() {
            // Empty means the active document's own directory, which is where
            // a note written about what is in front of you belongs.
            self.active_path()
                .and_then(|path| path.parent().map(Path::to_path_buf))
                .unwrap_or_default()
        } else {
            configured.clone()
        };

        let resolved = crate::editor::resolve_new_path(
            page,
            &self.vault.absolute(&directory),
            &self.vault.root,
            &self.files,
        );

        let path = match resolved {
            Ok(path) => path,
            Err(e) => {
                self.status.set(e.to_string(), Severity::Warning);
                return;
            }
        };

        let shown = self
            .vault
            .relative(&path)
            .unwrap_or(&path)
            .display()
            .to_string();

        self.overlay = Some(Overlay::Confirm {
            question: format!("Create \"{shown}\"?"),
            choices: vec![('y', "create".into())],
            action: ConfirmAction::CreatePage { path },
        });
        self.focus = Focus::Overlay;
    }

    /// Open the candidate the disambiguation overlay has selected.
    pub fn choose_candidate(&mut self, index: usize) {
        let Some(Overlay::Disambiguate {
            candidates, anchor, ..
        }) = self.overlay.clone()
        else {
            return;
        };

        let Some(path) = candidates.get(index) else {
            return;
        };
        let absolute = self.vault.absolute(path);

        self.close_overlay();
        self.navigate_to(&absolute, anchor);
    }

    // -- The backlink index --------------------------------------------------

    /// Load the cache and start indexing whatever it does not cover.
    ///
    /// Called when the walk finishes, which is when the set of files in the
    /// vault is known and the cache can be validated against it file by file.
    fn start_indexing(&mut self) {
        if !self.wikilinks.enabled || !self.wikilinks.index_on_start {
            self.vault.index.ready = true;
            return;
        }

        if self.wikilinks.cache {
            if let Some(cache) = index::load_cache(&self.vault.root) {
                self.vault.index = crate::vault::index::Index::from_cache(cache);
            }
        }

        // Anything the walk no longer reports has been deleted since the cache
        // was written; its inbound links become broken, which is what they are.
        let live: std::collections::BTreeSet<PathBuf> = self
            .vault
            .markdown
            .iter()
            .map(|(path, _, _)| path.clone())
            .collect();
        self.vault.index.retain_paths(&live);

        let stale: Vec<PathBuf> = self
            .vault
            .markdown
            .iter()
            .filter(|(path, mtime, size)| !self.vault.index.is_current(path, *mtime, *size))
            .map(|(path, _, _)| path.clone())
            .collect();

        self.vault.index.total = Some(self.vault.markdown.len());
        self.vault.index.indexed = self.vault.markdown.len() - stale.len();

        if stale.is_empty() {
            self.vault.index.ready = true;
            return;
        }

        let Some(sink) = self.index_sink.clone() else {
            // No sink means no event loop — a test, or print mode. Indexing
            // synchronously there would be a surprise; leaving it unbuilt is
            // what the Links mode already knows how to show.
            return;
        };

        self.vault.start_index(stale, move |event| sink(event));
    }

    /// Mark the index complete and write the cache.
    fn finish_indexing(&mut self) {
        self.vault.index.ready = true;
        self.vault.cancel_index();

        if !self.wikilinks.cache {
            return;
        }

        // A cache that cannot be written costs a slower start next time and
        // nothing else, so it is logged rather than shown.
        if let Err(e) = index::save_cache(&self.vault.root, &self.vault.index.to_cache()) {
            tracing::warn!("cannot write the index cache: {e}");
        }
    }

    /// Build the index synchronously, for tests and for the paths with no
    /// event loop behind them.
    pub fn index_now(&mut self) {
        let files: Vec<PathBuf> = self
            .vault
            .markdown
            .iter()
            .map(|(path, _, _)| path.clone())
            .collect();

        for path in files {
            self.reindex(&path);
        }

        self.vault.index.total = Some(self.vault.markdown.len());
        self.vault.index.ready = true;
    }

    /// Re-read one file into the index.
    pub fn reindex(&mut self, relative: &Path) {
        let absolute = self.vault.absolute(relative);

        let Ok(bytes) = std::fs::read(&absolute) else {
            self.vault.index.remove(relative);
            return;
        };
        let metadata = std::fs::metadata(&absolute).ok();

        let entry = index::entry_for(
            &String::from_utf8_lossy(&bytes),
            metadata.as_ref().and_then(|m| m.modified().ok()),
            metadata.as_ref().map_or(0, |m| m.len()),
        );
        self.vault.index.insert(relative.to_path_buf(), entry);
    }

    /// Whether a link in the active document resolves to nothing.
    ///
    /// Used by the links sidebar to mark broken targets. A wiki-link is only
    /// judged once the index is ready; until then nothing is called broken,
    /// because nothing is yet known.
    pub fn is_broken(&self, link: &crate::doc::links::Link) -> bool {
        let Some(doc) = self.tab().doc.as_ref() else {
            return false;
        };

        if link.kind == crate::doc::links::LinkKind::Wiki {
            if !self.wikilinks.enabled || !self.vault.index.ready {
                return false;
            }
            let source = self.active_path().unwrap_or_default();
            return matches!(
                self.vault
                    .index
                    .resolve_wiki(&link.target, &source, &self.wikilinks),
                WikiResolution::Missing { .. }
            );
        }

        matches!(
            links::resolve(&link.target, doc.dir(), &self.vault.root, &self.files),
            links::Resolved::Broken { .. }
        )
    }

    /// Every document that links to the active one.
    pub fn backlinks(&self) -> Vec<crate::vault::index::Backlink> {
        let Some(path) = self.active_path() else {
            return Vec::new();
        };
        self.vault.index.backlinks(&path, &self.wikilinks)
    }

    /// Move focus between the sidebar and the viewport.
    ///
    /// Edit mode locks focus to the viewport, and an open overlay owns focus
    /// until it closes.
    fn cycle_focus(&mut self, _forward: bool) {
        if self.overlay.is_some() || self.tab().mode == TabMode::Edit {
            return;
        }

        // With only two focusable panes, forwards and backwards agree.
        self.focus = match self.focus {
            Focus::Viewport if self.sidebar.visible => Focus::Sidebar,
            Focus::Sidebar => Focus::Viewport,
            other => other,
        };
    }

    fn toggle_sidebar(&mut self) {
        self.sidebar.visible = !self.sidebar.visible;

        if self.sidebar.visible {
            // An overlaid sidebar is useless without focus: it covers the
            // viewport, so the keys the user is about to press should reach it.
            if self.frames().sidebar_placement == SidebarPlacement::Overlaid {
                self.focus = Focus::Sidebar;
            }
        } else if self.focus == Focus::Sidebar {
            self.focus = Focus::Viewport;
        }
    }

    /// Widen or narrow the sidebar, within what the terminal can hold.
    fn resize_sidebar(&mut self, delta: i32) {
        let requested =
            (i32::from(self.sidebar.width) + delta).clamp(0, i32::from(u16::MAX)) as u16;

        self.sidebar.width = if self.area.width > 0 {
            layout::clamp_sidebar_width(requested, self.area.width)
        } else {
            requested
        };
    }

    fn toggle_mouse(&mut self) {
        self.mouse_capture = !self.mouse_capture;

        match terminal::set_mouse_capture(self.mouse_capture) {
            Ok(()) => {
                let state = if self.mouse_capture { "on" } else { "off" };
                self.status
                    .set(format!("Mouse capture {state}"), Severity::Info);
            }
            Err(e) => {
                self.mouse_capture = !self.mouse_capture;
                self.status
                    .set(format!("Cannot change mouse capture: {e}"), Severity::Error);
            }
        }
    }

    fn toggle_help(&mut self) {
        match self.overlay {
            Some(Overlay::Help { .. }) => self.close_overlay(),
            // Help replaces whatever else was open rather than stacking on it.
            _ => {
                self.overlay = Some(Overlay::Help { scroll: 0 });
                self.focus = Focus::Overlay;
            }
        }
    }

    /// `Esc`: close the overlay, abandon a pending key sequence, or leave edit
    /// mode, in that order.
    fn escape(&mut self) {
        if self.overlay.is_some() {
            self.close_overlay();
            return;
        }

        if self.sidebar.filter.is_some() {
            self.update(Action::TreeFilterCancel);
            return;
        }

        if self.tab().mode == TabMode::Edit {
            self.leave_edit_mode();
            return;
        }

        if !self.keymap.pending().is_empty() {
            self.keymap.clear_pending();
            self.status.pending = None;
        }
    }

    fn close_overlay(&mut self) {
        // Closing the find bar clears its highlighting, which is what `Esc`
        // means there; the query survives so `Ctrl+F` reopens it.
        if self.overlay == Some(Overlay::Find) {
            if let Some(find) = &mut self.tabs[self.active_tab].find {
                find.current = None;
            }
        }

        self.overlay = None;
        self.focus = if self.sidebar.visible
            && self.frames().sidebar_placement == SidebarPlacement::Overlaid
        {
            Focus::Sidebar
        } else {
            Focus::Viewport
        };
    }
}

/// A message from any source, waiting to become an [`Action`].
///
/// Terminal input, signals, and every background worker all send these down one
/// channel, so the main loop is a single blocking `recv` and nothing else.
#[derive(Debug)]
pub enum Message {
    /// A terminal input event.
    Input(crossterm::event::Event),
    /// A signal was delivered. The payload is the `libc` signal number.
    Signal(i32),
    /// Syntax highlighting finished loading.
    SyntaxReady,
    /// The vault walker reported something.
    Walk(WalkEvent),
    /// The backlink indexer reported something.
    Index(IndexEvent),
    /// The project searcher reported something.
    Search(SearchEvent),
    /// The filesystem watcher reported something.
    Watch(WatchEvent),
    /// A theme file changed.
    ThemeChanged,
}

/// Run the application until it quits.
///
/// The loop blocks on `recv` and does no polling, which is what makes the 0%
/// idle CPU target hold by construction rather than by tuning a timeout.
pub fn run(terminal: &mut Tui, app: &mut App, messages: &Receiver<Message>) -> anyhow::Result<()> {
    // Seed the size before the first frame so layout-dependent state is right
    // from the start rather than after the first resize.
    let size = terminal.size()?;
    app.update(Action::Resize(size.width, size.height));

    loop {
        terminal.draw(|frame| ui::draw(app, frame))?;

        let Ok(message) = messages.recv() else {
            // Every sender is gone, which can only happen during teardown.
            break;
        };

        for action in crate::event::translate(app, message) {
            app.update(action);
        }

        if app.should_suspend {
            app.should_suspend = false;
            suspend(terminal)?;
        }

        if let Some(path) = app.external_edit.take() {
            match hand_over(terminal, app, &path) {
                Ok(()) => app.update(Action::ReloadDocument),
                Err(e) => app
                    .status
                    .set(format!("$EDITOR failed: {e}"), Severity::Error),
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Run `$EDITOR` on a file with the terminal handed back to it.
///
/// The editor is very likely a full-screen application itself, so perga leaves
/// the alternate screen and raw mode entirely rather than trying to share
/// them, waits for the child, and sets the terminal back up afterwards.
fn hand_over(terminal: &mut Tui, app: &App, path: &Path) -> anyhow::Result<()> {
    use std::process::Command;

    let command = app
        .external_command()
        .ok_or_else(|| anyhow::anyhow!("no editor configured"))?;

    // Split on whitespace so `editor.external_command = "code --wait"` works;
    // the *path* is still a separate argument and never goes through a shell.
    let mut parts = command.split_whitespace();
    let program = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("no editor configured"))?;

    let mouse = terminal::mouse_capture_active();
    terminal::restore()?;

    let status = Command::new(program).args(parts).arg(path).status();

    *terminal = terminal::setup(mouse)?;
    // The editor owned the screen; nothing perga drew before is still there.
    terminal.clear()?;

    let status = status?;
    if !status.success() {
        anyhow::bail!("the editor exited with {status}");
    }

    Ok(())
}

/// Suspend the process with `SIGTSTP` and pick the terminal back up on resume.
///
/// This is what a terminal user expects `Ctrl+Z` to do in a full-screen
/// program: `fg` must bring the screen back intact.
fn suspend(terminal: &mut Tui) -> anyhow::Result<()> {
    let mouse = terminal::mouse_capture_active();

    terminal::restore()?;

    #[cfg(unix)]
    signal_hook::low_level::raise(signal_hook::consts::SIGTSTP)?;

    *terminal = terminal::setup(mouse)?;
    // The screen was handed back to the shell while suspended, so the whole
    // frame has to be repainted rather than diffed against what was there.
    terminal.clear()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An app sized like a comfortable terminal.
    fn app() -> App {
        let mut app = App::new(
            Theme::dark(),
            Keymap::defaults(),
            UiConfig::default(),
            FilesConfig::default(),
        );
        app.update(Action::Resize(120, 40));
        app
    }

    #[test]
    fn opens_in_read_mode_with_the_viewport_focused() {
        let app = app();
        assert_eq!(app.tab().mode, TabMode::Read);
        assert_eq!(app.focus, Focus::Viewport);
        assert!(app.overlay.is_none());
    }

    #[test]
    fn focus_cycles_between_the_sidebar_and_the_viewport() {
        let mut app = app();
        app.update(Action::FocusNext);
        assert_eq!(app.focus, Focus::Sidebar);
        app.update(Action::FocusNext);
        assert_eq!(app.focus, Focus::Viewport);
        app.update(Action::FocusPrev);
        assert_eq!(app.focus, Focus::Sidebar);
    }

    #[test]
    fn hiding_the_sidebar_moves_focus_out_of_it() {
        let mut app = app();
        app.update(Action::FocusNext);
        assert_eq!(app.focus, Focus::Sidebar);

        app.update(Action::ToggleSidebar);
        assert!(!app.sidebar.visible);
        assert_eq!(app.focus, Focus::Viewport);
    }

    #[test]
    fn focus_does_not_move_into_a_hidden_sidebar() {
        let mut app = app();
        app.update(Action::ToggleSidebar);
        app.update(Action::FocusNext);
        assert_eq!(app.focus, Focus::Viewport);
    }

    #[test]
    fn an_overlaid_sidebar_takes_focus_when_it_opens() {
        let mut app = app();
        app.update(Action::Resize(70, 40));
        app.update(Action::ToggleSidebar); // hide
        app.update(Action::ToggleSidebar); // show, overlaid
        assert_eq!(app.frames().sidebar_placement, SidebarPlacement::Overlaid);
        assert_eq!(app.focus, Focus::Sidebar);
    }

    #[test]
    fn a_split_sidebar_does_not_steal_focus_when_it_opens() {
        let mut app = app();
        app.update(Action::ToggleSidebar);
        app.update(Action::ToggleSidebar);
        assert_eq!(app.frames().sidebar_placement, SidebarPlacement::Split);
        assert_eq!(app.focus, Focus::Viewport);
    }

    #[test]
    fn resizing_the_sidebar_stays_within_bounds() {
        let mut app = app();
        for _ in 0..200 {
            app.update(Action::SidebarWiden);
        }
        assert!(app.sidebar.width <= 60, "{}", app.sidebar.width);

        for _ in 0..200 {
            app.update(Action::SidebarNarrow);
        }
        assert_eq!(app.sidebar.width, layout::MIN_SIDEBAR_WIDTH);
    }

    #[test]
    fn a_shrunken_terminal_narrows_the_drawn_sidebar_but_keeps_the_choice() {
        let mut app = app();
        for _ in 0..40 {
            app.update(Action::SidebarWiden);
        }
        let chosen = app.sidebar.width;
        assert_eq!(chosen, 60);

        app.update(Action::Resize(60, 20));
        assert_eq!(app.frames().sidebar.unwrap().width, 30);
        assert_eq!(app.sidebar.width, chosen, "the chosen width was destroyed");

        // ...and it comes back when there is room for it again.
        app.update(Action::Resize(120, 40));
        assert_eq!(app.frames().sidebar.unwrap().width, chosen);
    }

    #[test]
    fn help_opens_takes_focus_and_closes_again() {
        let mut app = app();
        app.update(Action::ToggleHelp);
        assert_eq!(app.overlay, Some(Overlay::Help { scroll: 0 }));
        assert_eq!(app.focus, Focus::Overlay);

        app.update(Action::ToggleHelp);
        assert!(app.overlay.is_none());
        assert_eq!(app.focus, Focus::Viewport);
    }

    #[test]
    fn escape_closes_an_overlay() {
        let mut app = app();
        app.update(Action::ToggleHelp);
        app.update(Action::Escape);
        assert!(app.overlay.is_none());
        assert_eq!(app.focus, Focus::Viewport);
    }

    #[test]
    fn focus_does_not_move_while_an_overlay_is_open() {
        let mut app = app();
        app.update(Action::ToggleHelp);
        app.update(Action::FocusNext);
        assert_eq!(app.focus, Focus::Overlay);
    }

    #[test]
    fn switching_sidebar_mode_reveals_a_hidden_sidebar() {
        let mut app = app();
        app.update(Action::ToggleSidebar);
        assert!(!app.sidebar.visible);

        app.update(Action::SetSidebarMode(SidebarMode::Outline));
        assert!(app.sidebar.visible);
        assert_eq!(app.sidebar.mode, SidebarMode::Outline);
    }

    #[test]
    fn quit_sets_the_flag() {
        let mut app = app();
        app.update(Action::Quit);
        assert!(app.should_quit);
        assert_eq!(app.exit_code, 0);
    }
}
