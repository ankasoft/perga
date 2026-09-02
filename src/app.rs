//! The `App` struct, the blocking event loop, and top-level state transitions.
//!
//! Everything here is deliberately terminal-free: `App` is constructed, fed
//! [`Action`]s, and asserted on without a backend. The loop that reads from the
//! terminal lives in [`run`], and it does nothing but translate messages into
//! actions and hand them to [`App::update`].

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossbeam_channel::Receiver;
use ratatui::layout::Rect;

use crate::action::Action;
use crate::config::keymap::Keymap;
use crate::config::schema::{FilesConfig, GeneralConfig, UiConfig, WikiLinkConfig};
use crate::doc::document::Document;
use crate::doc::highlight::Highlighter;
use crate::doc::links;
use crate::doc::render::{RenderedDocument, Renderer};
use crate::search::in_doc::FindState;
use crate::terminal::{self, Tui};
use crate::theme::Theme;
use crate::ui;
use crate::ui::layout::{self, SidebarPlacement};
use crate::ui::overlay::prompt::{TextEdit, TextInput};
use crate::ui::sidebar::SidebarMode;
use crate::vault::index::{self, IndexEvent, WikiResolution};
use crate::vault::walker::{WalkEvent, WalkOptions};
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
    /// Link hint mode: every visible link wears a label.
    Hints {
        /// The links wearing labels, in reading order.
        links: Vec<usize>,
        /// The label typed so far.
        typed: String,
    },
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
        let tab = &mut self.tabs[self.active_tab];
        tab.doc = Some(doc);
        tab.layout = RenderedDocument::new();
        tab.scroll = 0;
        tab.hscroll = 0;
        tab.focused_link = None;

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
            Action::Quit => self.should_quit = true,
            Action::ForceQuit(code) => {
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
            Action::TreeFilter => self.open_tree_filter(),
            Action::TreeFilterEdit(edit) => self.edit_tree_filter(edit),
            Action::TreeFilterAccept => self.sidebar.filter = None,
            Action::TreeFilterCancel => {
                self.sidebar.filter = None;
                self.vault.tree.set_filter(None);
            }
            Action::OpenPath(path) => self.open_path(&path),

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
            Action::HistoryBack => self.navigate_history(false),
            Action::HistoryForward => self.navigate_history(true),

            // Actions whose handlers arrive with the features that need them.
            _ => {}
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
            // The other two modes arrive with the features behind them.
            SidebarMode::Search | SidebarMode::Links => {}
        }
    }

    /// Activate the selection in whichever sidebar mode is showing.
    fn activate_sidebar_selection(&mut self) {
        match self.sidebar.mode {
            SidebarMode::Files => self.tree_expand_or_open(),
            SidebarMode::Outline => self.jump_to_selected_heading(),
            SidebarMode::Search | SidebarMode::Links => {}
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
            WikiResolution::Missing { page, .. } => {
                // Section 9.11 turns this into an offer to create the file;
                // until then it says what it could not find.
                self.status
                    .set(format!("No page `{page}` in this vault"), Severity::Warning);
            }
        }
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

        if app.should_quit {
            break;
        }
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
