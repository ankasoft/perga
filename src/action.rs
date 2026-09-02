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

use crate::ui::sidebar::SidebarMode;

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
    /// Open the project-wide search prompt.
    OpenProjectSearch,

    // -- Sidebar tree ------------------------------------------------------
    /// Move the sidebar selection down.
    TreeDown,
    /// Move the sidebar selection up.
    TreeUp,
    /// Expand the selected directory, or open the selected file.
    TreeExpandOrOpen,
    /// Collapse the selected directory, or move to its parent.
    TreeCollapseOrParent,
    /// Show or hide dotted entries in the tree.
    TreeToggleHidden,
    /// Show or hide non-Markdown files in the tree.
    TreeToggleAllFiles,
    /// Filter the tree by name.
    TreeFilter,
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
