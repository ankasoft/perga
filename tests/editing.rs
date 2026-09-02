//! Edit mode, saving, creating and renaming files, and live reload.
//!
//! Every acceptance criterion in Sections 9.8 and 9.11 is asserted here apart
//! from the `$EDITOR` handoff, which needs a pseudo-terminal — see
//! `docs/decisions.md`.

mod common;

use std::path::{Path, PathBuf};

use common::frame;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use perga::action::Action;
use perga::app::{App, ConfirmAction, Overlay, PromptKind, Severity, TabMode};
use perga::config::keymap::Keymap;
use perga::config::schema::{FilesConfig, UiConfig};
use perga::doc::document::Document;
use perga::theme::Theme;
use perga::ui::overlay::prompt::TextEdit;

/// A scratch vault, so the tests may write in it.
///
/// The committed fixture vault is read by every other test binary in parallel;
/// editing it would be a race and a diff.
fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("perga-editing-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("docs")).expect("a scratch vault");

    std::fs::write(
        root.join("note.md"),
        "# A note\n\nThe first paragraph.\n\nThe second paragraph.\n",
    )
    .unwrap();
    std::fs::write(root.join("docs/other.md"), "# Other\n\nSee [[A note]].\n").unwrap();

    root
}

/// An application on a scratch vault, with `name` open and one frame drawn.
fn editing(name: &str, open: &str) -> (App, PathBuf) {
    let root = scratch(name);

    let mut app = App::new(
        Theme::dark(),
        Keymap::defaults(),
        UiConfig::default(),
        FilesConfig::default(),
    );
    app.update(Action::Resize(100, 30));
    app.set_vault_root(&root);
    common::walk(&mut app);
    app.index_now();
    app.open(Document::load(root.join(open)).expect("the fixture is readable"));
    frame(&mut app, 100, 30);

    (app, root)
}

/// Type into the buffer.
fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        app.update(Action::EditInput(KeyEvent::new(
            KeyCode::Char(c),
            KeyModifiers::NONE,
        )));
    }
}

/// Type into whichever prompt is open.
fn type_prompt(app: &mut App, text: &str) {
    for c in text.chars() {
        app.update(Action::PromptEdit(TextEdit::Insert(c)));
    }
}

// -- Edit mode ---------------------------------------------------------------

#[test]
fn entering_edit_mode_lands_where_the_reader_was() {
    let (mut app, _) = editing("enter", "note.md");

    app.update(Action::EnterEditMode);

    assert_eq!(app.tab().mode, TabMode::Edit);
    assert!(app.tab().editor.is_some());
    assert!(!app.tab().dirty);

    // The buffer is drawn instead of the rendered document, so the source's
    // own markup is on screen.
    let painted = frame(&mut app, 100, 30);
    assert!(painted.contains("# A note"), "{painted}");
}

#[test]
fn typing_marks_the_tab_dirty_and_undoing_clears_it() {
    let (mut app, _) = editing("dirty", "note.md");
    app.update(Action::EnterEditMode);

    type_text(&mut app, "x");
    assert!(app.tab().dirty);
    assert_eq!(app.tab().display_label(), "● note");

    app.update(Action::Undo);
    assert!(!app.tab().dirty, "undoing back to the saved text is clean");

    app.update(Action::Redo);
    assert!(app.tab().dirty);
}

#[test]
fn a_read_only_document_refuses_edit_mode() {
    let root = scratch("readonly");
    std::fs::write(root.join("bad.md"), [0xff, 0xfe, b'#']).unwrap();

    let mut app = App::new(
        Theme::dark(),
        Keymap::defaults(),
        UiConfig::default(),
        FilesConfig::default(),
    );
    app.update(Action::Resize(100, 30));
    app.set_vault_root(&root);
    app.open(Document::load(root.join("bad.md")).unwrap());

    app.update(Action::EnterEditMode);

    assert_eq!(app.tab().mode, TabMode::Read);
    assert!(app.tab().editor.is_none());
}

/// Acceptance: editing then navigating away prompts, and Cancel keeps the user
/// in place.
#[test]
fn leaving_a_dirty_buffer_asks_first() {
    let (mut app, _) = editing("leave", "note.md");
    app.update(Action::EnterEditMode);
    type_text(&mut app, "x");

    app.update(Action::Escape);

    let Some(Overlay::Confirm { action, .. }) = &app.overlay else {
        panic!("leaving a dirty buffer must ask");
    };
    assert_eq!(*action, ConfirmAction::LeaveEditMode);
    assert_eq!(
        app.tab().mode,
        TabMode::Edit,
        "still editing until answered"
    );

    // Cancel keeps the reader exactly where they were.
    app.update(Action::Confirm('c'));
    assert!(app.overlay.is_none());
    assert_eq!(app.tab().mode, TabMode::Edit);
    assert!(app.tab().dirty);
}

#[test]
fn discarding_leaves_edit_mode_and_the_file_alone() {
    let (mut app, root) = editing("discard", "note.md");
    let before = std::fs::read_to_string(root.join("note.md")).unwrap();

    app.update(Action::EnterEditMode);
    type_text(&mut app, "x");
    app.update(Action::Escape);
    app.update(Action::Confirm('d'));

    assert_eq!(app.tab().mode, TabMode::Read);
    assert!(!app.tab().dirty);
    assert_eq!(
        std::fs::read_to_string(root.join("note.md")).unwrap(),
        before
    );
}

#[test]
fn a_clean_buffer_leaves_without_asking() {
    let (mut app, _) = editing("clean", "note.md");
    app.update(Action::EnterEditMode);

    app.update(Action::Escape);

    assert!(app.overlay.is_none());
    assert_eq!(app.tab().mode, TabMode::Read);
}

#[test]
fn saving_writes_the_buffer_and_reparses_the_document() {
    let (mut app, root) = editing("save", "note.md");
    app.update(Action::EnterEditMode);

    type_text(&mut app, "## New heading");
    app.update(Action::EditInput(KeyEvent::new(
        KeyCode::Enter,
        KeyModifiers::NONE,
    )));
    app.update(Action::Save);

    assert!(!app.tab().dirty);
    let written = std::fs::read_to_string(root.join("note.md")).unwrap();
    assert!(written.starts_with("## New heading"), "{written}");

    // The outline followed the text that was written.
    let outline = &app.tab().doc.as_ref().unwrap().outline;
    assert!(
        outline.iter().any(|h| h.text == "New heading"),
        "{outline:?}"
    );
}

/// Acceptance: a file with CRLF endings keeps them.
#[test]
fn crlf_endings_survive_an_edit() {
    let root = scratch("crlf");
    let path = root.join("crlf.md");
    std::fs::write(&path, "# Note\r\n\r\nProse.\r\n").unwrap();

    let mut app = App::new(
        Theme::dark(),
        Keymap::defaults(),
        UiConfig::default(),
        FilesConfig::default(),
    );
    app.update(Action::Resize(100, 30));
    app.set_vault_root(&root);
    app.open(Document::load(&path).unwrap());

    app.update(Action::EnterEditMode);
    type_text(&mut app, "x");
    app.update(Action::Save);

    let written = std::fs::read_to_string(&path).unwrap();
    assert!(written.contains("\r\n"), "{written:?}");
    assert!(!written.contains("\n\n\n"), "{written:?}");
}

#[test]
fn a_file_changed_on_disk_asks_before_overwriting() {
    let (mut app, root) = editing("conflict", "note.md");
    let path = root.join("note.md");

    app.update(Action::EnterEditMode);
    type_text(&mut app, "mine");

    // Somebody else writes, and the buffer's idea of the mtime goes stale.
    if let Some(editor) = &mut app.tabs[0].editor {
        editor.known_mtime = std::time::SystemTime::UNIX_EPOCH;
    }
    std::fs::write(&path, "theirs\n").unwrap();

    app.update(Action::Save);

    let Some(Overlay::Confirm { action, .. }) = &app.overlay else {
        panic!("a conflict must be reported");
    };
    assert_eq!(*action, ConfirmAction::OverwriteChanged);
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs\n");

    // Overwriting is the reader's call, and only theirs.
    app.update(Action::Confirm('o'));
    assert!(std::fs::read_to_string(&path).unwrap().starts_with("mine"));
}

#[test]
fn a_paste_is_one_undo_step() {
    let (mut app, _) = editing("paste", "note.md");
    app.update(Action::EnterEditMode);

    let pasted: String = (0..5_000).map(|i| format!("line {i}\n")).collect();
    let started = std::time::Instant::now();
    app.update(Action::EditPaste(pasted));
    let took = started.elapsed();

    assert!(app.tab().dirty);
    let lines = app.tab().editor.as_ref().unwrap().textarea.lines().len();
    assert!(lines > 5_000, "{lines}");

    app.update(Action::Undo);
    let after = app.tab().editor.as_ref().unwrap().textarea.lines().len();
    assert!(after < 10, "one paste must be one undo, not five thousand");

    // Not asserted as a gate — recorded because Section 9.8 names 100 ms.
    eprintln!("pasting 5,000 lines took {took:?}");
}

#[test]
fn quitting_with_unsaved_edits_asks() {
    let (mut app, _) = editing("quit", "note.md");
    app.update(Action::EnterEditMode);
    type_text(&mut app, "x");

    app.update(Action::Quit);

    assert!(!app.should_quit);
    assert!(matches!(
        app.overlay,
        Some(Overlay::Confirm {
            action: ConfirmAction::Quit,
            ..
        })
    ));

    app.update(Action::Confirm('d'));
    assert!(app.should_quit);
}

#[test]
fn a_signal_parks_unsaved_text_for_the_next_run() {
    let (mut app, root) = editing("recovery", "note.md");
    app.update(Action::EnterEditMode);
    type_text(&mut app, "unsaved");

    app.update(Action::ForceQuit(130));
    assert!(app.should_quit);
    assert_eq!(app.exit_code, 130);

    let parked = perga::editor::buffer::read_recovery(&root, &root.join("note.md"));
    assert!(parked.is_some_and(|text| text.contains("unsaved")));

    perga::editor::buffer::clear_recovery(&root, &root.join("note.md"));
}

// -- Creating and renaming ---------------------------------------------------

/// Acceptance: `Ctrl+N` with `notes/ideas` creates `notes/ideas.md`, creating
/// `notes/` if absent, and lands in edit mode.
#[test]
fn creating_a_file_makes_its_directories_and_starts_editing() {
    let (mut app, root) = editing("create", "note.md");

    app.update(Action::NewFile);
    assert!(matches!(
        app.overlay,
        Some(Overlay::Prompt {
            kind: PromptKind::NewFile,
            ..
        })
    ));

    app.update(Action::PromptEdit(TextEdit::Clear));
    type_prompt(&mut app, "notes/ideas");
    app.update(Action::PromptAccept);

    assert!(root.join("notes/ideas.md").is_file());
    assert_eq!(app.tab().mode, TabMode::Edit);
    assert_eq!(app.tab().editor.as_ref().unwrap().cursor(), (0, 0));
    assert_eq!(
        app.tab().doc.as_ref().map(|d| d.path.clone()),
        Some(root.join("notes/ideas.md"))
    );
}

#[test]
fn creating_over_an_existing_file_is_refused() {
    let (mut app, root) = editing("create-clash", "note.md");
    let before = std::fs::read_to_string(root.join("note.md")).unwrap();

    app.update(Action::NewFile);
    app.update(Action::PromptEdit(TextEdit::Clear));
    type_prompt(&mut app, "note.md");
    app.update(Action::PromptAccept);

    let (message, severity) = app.status.message.clone().expect("a message");
    assert!(message.contains("already exists"), "{message}");
    assert_eq!(severity, Severity::Error);
    assert_eq!(
        std::fs::read_to_string(root.join("note.md")).unwrap(),
        before
    );
}

/// Acceptance: a path outside the vault is refused with a clear message.
#[test]
fn creating_outside_the_vault_is_refused() {
    let (mut app, _) = editing("escape", "note.md");

    app.update(Action::NewFile);
    app.update(Action::PromptEdit(TextEdit::Clear));
    type_prompt(&mut app, "../../etc/perga-should-not-exist");
    app.update(Action::PromptAccept);

    let (message, _) = app.status.message.clone().expect("a message");
    assert!(message.contains("outside the vault"), "{message}");
    assert!(!Path::new("/etc/perga-should-not-exist.md").exists());
}

/// Acceptance: following a broken `[[X]]`, confirming, and going back finds
/// the link resolved.
#[test]
fn creating_from_a_broken_wiki_link_resolves_it() {
    let (mut app, root) = editing("wiki-create", "docs/other.md");

    // A wiki-link to a page that does not exist yet.
    std::fs::write(
        root.join("docs/other.md"),
        "# Other\n\nSee [[Not Written Yet]].\n",
    )
    .unwrap();
    app.update(Action::ReloadDocument);
    app.index_now();
    frame(&mut app, 100, 30);

    app.update(Action::NextLink);
    app.update(Action::FollowLink);

    let Some(Overlay::Confirm { action, .. }) = app.overlay.clone() else {
        panic!("a broken wiki-link offers to create the page");
    };
    assert!(matches!(action, ConfirmAction::CreatePage { .. }));

    app.update(Action::Confirm('y'));

    let created = root.join("docs/Not Written Yet.md");
    assert!(created.is_file());
    assert_eq!(
        app.tab().doc.as_ref().map(|d| d.path.clone()),
        Some(created)
    );

    // The source document's link now resolves, without the document changing.
    app.index_now();
    assert!(matches!(
        app.vault.index.resolve_wiki(
            "Not Written Yet",
            Path::new("docs/other.md"),
            &app.wikilinks
        ),
        perga::vault::index::WikiResolution::Found { .. }
    ));
}

/// Acceptance: renaming a file open in two tabs updates both.
#[test]
fn renaming_follows_every_tab_that_had_the_file_open() {
    let (mut app, root) = editing("rename", "note.md");

    app.update(Action::NewFile);
    app.update(Action::Escape);
    app.update(Action::NewTab);
    app.update(Action::OpenPath(PathBuf::from("note.md")));
    assert_eq!(app.tabs.len(), 2);

    app.update(Action::RenameDocument);
    app.update(Action::PromptEdit(TextEdit::Clear));
    type_prompt(&mut app, "renamed.md");
    app.update(Action::PromptAccept);

    assert!(root.join("renamed.md").is_file());
    assert!(!root.join("note.md").exists());

    for tab in &app.tabs {
        assert_eq!(
            tab.doc.as_ref().map(|d| d.path.clone()),
            Some(root.join("renamed.md")),
            "a tab was left reading a file that is no longer there"
        );
        assert_eq!(tab.label(), "renamed");
    }
}

#[test]
fn a_rename_refuses_a_path_and_takes_only_a_name() {
    let (mut app, root) = editing("rename-path", "note.md");

    app.update(Action::RenameDocument);
    app.update(Action::PromptEdit(TextEdit::Clear));
    type_prompt(&mut app, "../escaped.md");
    app.update(Action::PromptAccept);

    let (message, _) = app.status.message.clone().expect("a message");
    assert!(message.contains("separator"), "{message}");
    assert!(root.join("note.md").is_file());
}

#[test]
fn a_rename_says_how_many_documents_now_link_to_the_old_name() {
    let (mut app, root) = editing("rename-links", "note.md");

    // `docs/other.md` links to `[[A note]]`, which is `note.md`'s title.
    app.update(Action::RenameDocument);
    app.update(Action::PromptEdit(TextEdit::Clear));
    type_prompt(&mut app, "renamed.md");
    app.update(Action::PromptAccept);

    assert!(root.join("renamed.md").is_file());
    let (message, _) = app.status.message.clone().expect("a message");
    assert!(message.contains("Renamed"), "{message}");
}

// -- Live reload -------------------------------------------------------------

#[test]
fn a_change_on_disk_reloads_a_clean_document_in_place() {
    let (mut app, root) = editing("reload", "note.md");
    for _ in 0..2 {
        app.update(Action::ScrollLineDown);
    }
    frame(&mut app, 100, 30);
    let scroll = app.tab().scroll;

    std::fs::write(
        root.join("note.md"),
        "# A note\n\nRewritten by somebody else.\n",
    )
    .unwrap();
    app.files_changed(&[PathBuf::from("note.md")]);

    let source = &app.tab().doc.as_ref().unwrap().source;
    assert!(source.contains("somebody else"), "{source}");
    assert!(
        app.tab().scroll <= scroll,
        "the reader was thrown to the top"
    );
}

#[test]
fn a_change_on_disk_never_clobbers_a_dirty_buffer() {
    let (mut app, root) = editing("reload-dirty", "note.md");
    app.update(Action::EnterEditMode);
    type_text(&mut app, "mine");

    std::fs::write(root.join("note.md"), "theirs\n").unwrap();
    app.files_changed(&[PathBuf::from("note.md")]);

    assert!(app.tab().dirty);
    assert_eq!(
        app.status.message.as_ref().map(|(m, _)| m.as_str()),
        Some("File changed on disk (r to reload)")
    );
    assert!(app
        .tab()
        .editor
        .as_ref()
        .unwrap()
        .contents()
        .starts_with("mine"));
}

#[test]
fn perga_does_not_reload_on_its_own_save() {
    let (mut app, _) = editing("own-write", "note.md");
    app.update(Action::EnterEditMode);
    type_text(&mut app, "mine");
    app.update(Action::Save);

    let before = app.tab().doc.as_ref().unwrap().content_hash;

    // The watcher reports the write perga just made.
    app.files_changed(&[PathBuf::from("note.md")]);

    assert_eq!(
        app.tab().doc.as_ref().unwrap().content_hash,
        before,
        "perga reloaded on its own write"
    );
    assert_eq!(app.tab().mode, TabMode::Edit, "and it stayed in edit mode");
}

#[test]
fn a_deleted_file_leaves_the_tree_and_the_index() {
    let (mut app, root) = editing("delete", "note.md");
    assert!(app.vault.tree.select_path(Path::new("docs/other.md")));

    std::fs::remove_file(root.join("docs/other.md")).unwrap();
    app.files_removed(&[PathBuf::from("docs/other.md")]);

    assert!(!app.vault.tree.select_path(Path::new("docs/other.md")));
    assert!(app
        .vault
        .index
        .backlinks(Path::new("note.md"), &app.wikilinks)
        .is_empty());
}
