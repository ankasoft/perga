//! The `Action` enum: the only channel through which application state changes.
//!
//! Input events and background-worker messages are both translated into
//! actions, and `App::update` is the single place that applies them. Rendering
//! never mutates state.
//!
//! Actions fall into two groups. Most are *bindable*: they carry no payload,
//! they appear in the keymap table in [`crate::config::keymap`], and they show
//! up in the help overlay. The rest are internal — resize notifications and
//! background-worker progress — and are never reachable from a key.

use std::path::PathBuf;

use crate::ui::overlay::prompt::TextEdit;
use crate::ui::sidebar::SidebarMode;
use crate::vault::index::FileEntry;
use crate::vault::walker::Entry;

/// Every state change in perga.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    // -- Lifecycle ---------------------------------------------------------
    /// Quit, prompting first if any tab has unsaved edits.
    Quit,
    /// Quit without prompting, with this exit code. Reserved for signals.
    ForceQuit(u8),
    /// Suspend the process with `SIGTSTP` after restoring the terminal.
    Suspend,
    /// The terminal was resized to these dimensions.
    Resize(u16, u16),
    /// The background thread finished loading the syntax sets, so code blocks
    /// drawn plain can be drawn highlighted.
    SyntaxReady,

    // -- Vault walking -----------------------------------------------------
    /// A batch of entries from the vault walker.
    VaultEntries(Vec<Entry>),
    /// The walk finished, having found this many entries.
    VaultWalkFinished(usize),
    /// The walk could not be completed.
    VaultWalkFailed(String),
    /// A batch of freshly indexed files.
    IndexBatch(Vec<(PathBuf, FileEntry)>),
    /// The backlink index is complete.
    IndexFinished,

    // -- Focus and chrome --------------------------------------------------
    /// Move focus to the next pane.
    FocusNext,
    /// Move focus to the previous pane.
    FocusPrev,
    /// Show or hide the sidebar.
    ToggleSidebar,
    /// Widen the sidebar by one column.
    SidebarWiden,
    /// Narrow the sidebar by one column.
    SidebarNarrow,
    /// Switch the sidebar to a specific mode.
    SetSidebarMode(SidebarMode),
    /// Turn mouse capture on or off.
    ToggleMouse,
    /// Open or close the help overlay.
    ToggleHelp,
    /// Close the topmost overlay, or leave edit mode.
    Escape,

    // -- Viewport scrolling ------------------------------------------------
    ScrollLineDown,
    ScrollLineUp,
    ScrollHalfPageDown,
    ScrollHalfPageUp,
    ScrollPageDown,
    ScrollPageUp,
    /// Jump to the top of the document.
    ScrollTop,
    /// Jump to the bottom of the document.
    ScrollBottom,
    /// Scroll horizontally left, for clipped code blocks and wide tables.
    ScrollLeft,
    /// Scroll horizontally right.
    ScrollRight,
    /// Scroll to the previous heading.
    PrevHeading,
    /// Scroll to the next heading.
    NextHeading,
    /// Scroll by a mouse wheel notch.
    ScrollWheelDown,
    ScrollWheelUp,

    // -- Links and history -------------------------------------------------
    /// Focus the next link in reading order.
    NextLink,
    /// Focus the previous link in reading order.
    PrevLink,
    /// Follow the focused link in the current tab.
    FollowLink,
    /// Follow the focused link in a new background tab.
    FollowLinkInNewTab,
    /// Label every visible link and follow the one whose label is typed.
    HintMode,
    /// Follow the link a hint label selected. Not bindable; hint mode emits it.
    FollowHintedLink(usize),
    /// Open the candidate a disambiguation overlay selected. Not bindable.
    ChooseCandidate(usize),
    /// Go back in this tab's history.
    HistoryBack,
    /// Go forward in this tab's history.
    HistoryForward,

    // -- Documents ---------------------------------------------------------
    /// Re-read the active document from disk.
    ReloadDocument,
    /// Rename the active document.
    RenameDocument,
    /// Copy the active document's path to the clipboard with OSC 52.
    CopyDocumentPath,
    /// Enter edit mode on the active document.
    EnterEditMode,
    /// Hand the active document to `$EDITOR`.
    OpenInExternalEditor,

    // -- Tabs --------------------------------------------------------------
    NewTab,
    CloseTab,
    NextTab,
    PrevTab,

    // -- Overlays and prompts ----------------------------------------------
    /// Prompt for a path and create a new file there.
    NewFile,
    /// Open the fuzzy quick switcher.
    OpenQuickSwitcher,
    /// Open the incremental find-in-document bar.
    OpenFindInDocument,
    /// Edit the find query. Only reachable while the find bar is open.
    FindEdit(TextEdit),
    /// Step to the next match.
    FindNext,
    /// Step to the previous match.
    FindPrev,
    /// Close the find bar and clear its highlighting.
    CloseFind,
    /// Open the project-wide search prompt.
    OpenProjectSearch,

    // -- Sidebar -----------------------------------------------------------
    //
    // The four movement actions are shared by every sidebar mode; what they do
    // depends on which mode is showing. The rest belong to the files mode.
    /// Move the sidebar selection down.
    SidebarDown,
    /// Activate the selection: expand a directory, open a file, or jump to a
    /// heading, a search hit, or a link.
    SidebarActivate,
    /// Move the sidebar selection up.
    SidebarUp,
    /// Step back: collapse a directory, or move to its parent.
    SidebarBack,
    /// Show or hide dotted entries in the tree.
    TreeToggleHidden,
    /// Show or hide non-Markdown files in the tree.
    TreeToggleAllFiles,
    /// Start filtering the tree by name.
    TreeFilter,
    /// Edit the tree filter. Only reachable while the filter input is open.
    TreeFilterEdit(TextEdit),
    /// Keep the filter and hand focus back to the tree.
    TreeFilterAccept,
    /// Abandon the filter and restore the whole tree.
    TreeFilterCancel,
    /// Open a specific path, resolved against the vault root.
    OpenPath(PathBuf),
    /// Rename the selected tree entry.
    TreeRename,

    // -- Editing -----------------------------------------------------------
    /// Write the active buffer to disk.
    Save,
    /// Undo the last edit.
    Undo,
    /// Redo the last undone edit.
    Redo,
}
