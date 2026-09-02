//! Vault walking and the files sidebar, driven the way the application drives
//! them: a sequence of [`Action`]s, asserted on state.

mod common;

use std::path::{Path, PathBuf};

use common::{app, frame, vault, vault_app, walk};
use perga::action::Action;
use perga::app::{App, Focus};
use perga::ui::overlay::prompt::TextEdit;
use perga::vault::walker::Entry;

/// The names of the tree's visible rows, indented by depth.
fn rows(app: &App) -> Vec<String> {
    let tree = &app.vault.tree;
    tree.rows()
        .iter()
        .map(|row| format!("{}{}", "  ".repeat(row.depth), tree.node(row.node).name))
        .collect()
}

/// The selected entry's path.
fn selected(app: &App) -> Option<PathBuf> {
    app.vault.tree.selected().map(|node| node.path.clone())
}

/// Type a string into whatever text line is open.
fn type_into(app: &mut App, text: &str) {
    for c in text.chars() {
        app.update(Action::TreeFilterEdit(TextEdit::Insert(c)));
    }
}

#[test]
fn dotted_directories_appear_by_default() {
    let app = vault_app(120, 40);
    assert!(
        rows(&app).contains(&".github".to_string()),
        "a dotted directory is a directory of notes, not noise"
    );
}

#[test]
fn a_gitignored_directory_is_not_in_the_tree() {
    let app = vault_app(120, 40);
    assert!(!rows(&app).contains(&"notes".to_string()));
}

#[test]
fn non_markdown_files_appear_only_when_asked_for() {
    let mut app = vault_app(120, 40);
    app.update(Action::FocusNext);
    app.update(Action::TreeExpandOrOpen); // .github

    // The fixture's `.gitignore` is not Markdown.
    assert!(!rows(&app).contains(&".gitignore".to_string()));

    app.update(Action::TreeToggleAllFiles);
    assert!(rows(&app).contains(&".gitignore".to_string()));
}

#[test]
fn hidden_entries_can_be_toggled_off_and_back_on() {
    let mut app = vault_app(120, 40);

    app.update(Action::TreeToggleHidden);
    assert!(!rows(&app).contains(&".github".to_string()));

    app.update(Action::TreeToggleHidden);
    assert!(rows(&app).contains(&".github".to_string()));
}

#[test]
fn walking_into_a_directory_and_opening_a_file() {
    let mut app = vault_app(120, 40);
    app.update(Action::FocusNext);
    assert_eq!(app.focus, Focus::Sidebar);

    // Down to `docs`, into it, and on to `api/auth.md`.
    app.update(Action::TreeDown); // .github
    app.update(Action::TreeDown); // docs
    app.update(Action::TreeExpandOrOpen);
    assert_eq!(selected(&app), Some(PathBuf::from("docs")));

    app.update(Action::TreeExpandOrOpen); // steps into the open directory
    assert_eq!(selected(&app), Some(PathBuf::from("docs/api")));

    app.update(Action::TreeExpandOrOpen); // expands `api`
    app.update(Action::TreeExpandOrOpen); // steps to its first child
    assert_eq!(selected(&app), Some(PathBuf::from("docs/api/ambiguous.md")));

    app.update(Action::TreeDown);
    app.update(Action::TreeExpandOrOpen);

    let doc = app.tab().doc.as_ref().expect("a document is open");
    assert_eq!(doc.path, vault().join("docs/api/auth.md"));
}

#[test]
fn collapsing_walks_back_out_of_a_directory() {
    let mut app = vault_app(120, 40);
    app.vault.tree.reveal(Path::new("docs/api/auth.md"));

    app.update(Action::TreeCollapseOrParent);
    assert_eq!(selected(&app), Some(PathBuf::from("docs/api")));

    app.update(Action::TreeCollapseOrParent); // closes `api`
    app.update(Action::TreeCollapseOrParent); // up to `docs`
    assert_eq!(selected(&app), Some(PathBuf::from("docs")));
}

#[test]
fn opening_a_document_expands_the_path_to_it() {
    let mut app = vault_app(120, 40);
    app.update(Action::OpenPath(PathBuf::from("docs/guides/setup.md")));

    assert_eq!(selected(&app), Some(PathBuf::from("docs/guides/setup.md")));
    let visible = rows(&app);
    assert!(visible.contains(&"docs".to_string()));
    assert!(visible.contains(&"  guides".to_string()));
    assert!(visible.contains(&"    setup.md".to_string()));
}

#[test]
fn a_document_opened_before_the_walk_finishes_is_revealed_when_its_row_arrives() {
    // The order the real application starts in: a file argument opens
    // immediately, and the tree catches up behind it.
    let mut app = app(120, 40);
    app.set_vault_root(vault());
    app.open(perga::doc::document::Document::load(vault().join("docs/api/auth.md")).unwrap());
    assert!(app.vault.tree.is_empty());

    walk(&mut app);

    assert_eq!(selected(&app), Some(PathBuf::from("docs/api/auth.md")));
    assert!(rows(&app).contains(&"    auth.md".to_string()));
}

#[test]
fn the_filter_narrows_the_tree_as_it_is_typed() {
    let mut app = vault_app(120, 40);

    app.update(Action::TreeFilter);
    assert!(app.sidebar.filter.is_some());
    assert_eq!(app.focus, Focus::Sidebar);

    type_into(&mut app, "auth");
    assert_eq!(rows(&app), ["docs", "  api", "    auth.md"]);

    // Accepting keeps the filter and closes the line.
    app.update(Action::TreeFilterAccept);
    assert!(app.sidebar.filter.is_none());
    assert_eq!(app.vault.tree.filter(), Some("auth"));

    // Cancelling clears it.
    app.update(Action::TreeFilter);
    app.update(Action::TreeFilterCancel);
    assert_eq!(app.vault.tree.filter(), None);
    assert!(rows(&app).contains(&"README.md".to_string()));
}

#[test]
fn escape_closes_the_filter_line() {
    let mut app = vault_app(120, 40);
    app.update(Action::TreeFilter);
    type_into(&mut app, "auth");

    app.update(Action::Escape);
    assert!(app.sidebar.filter.is_none());
    assert_eq!(app.vault.tree.filter(), None);
}

#[test]
fn an_unreadable_path_is_a_status_message_rather_than_a_crash() {
    let mut app = vault_app(120, 40);
    app.update(Action::OpenPath(PathBuf::from("does-not-exist.md")));

    let (message, severity) = app.status.message.clone().expect("a message");
    assert!(message.contains("does-not-exist.md"), "{message}");
    assert_eq!(severity, perga::app::Severity::Error);
    assert!(app.tab().doc.is_none());
}

/// M3's definition of done: with a large vault, the first frame is painted
/// before the walk finishes.
///
/// Asserted as an ordering, not a wall-clock time — a shared CI runner cannot
/// be trusted with the latter, and the ordering is what actually matters.
#[test]
fn the_first_frame_is_painted_before_the_walk_finishes() {
    let root = generated_vault(10_000);
    let mut app = app(120, 40);
    app.set_vault_root(&root);

    // The tree is empty and the walk has not finished, and a frame still
    // paints — this is exactly the state the real application starts in.
    assert!(app.vault.tree.is_empty());
    assert!(!app.vault.tree.complete);
    let first = frame(&mut app, 120, 40);
    assert!(first.contains("perga"));
    assert!(first.contains("scanning"));

    // ...and once the batches land, the tree is there.
    walk(&mut app);
    assert!(app.vault.tree.complete);
    assert_eq!(
        app.vault.tree.entries,
        10_000 + 10,
        "10,000 files in 10 directories"
    );
    assert!(rows(&app).len() > 1);
}

#[test]
fn a_batch_landing_mid_walk_is_enough_to_draw_a_tree() {
    let mut app = app(120, 40);
    app.set_vault_root(vault());

    app.update(Action::VaultEntries(vec![
        Entry {
            path: PathBuf::from("docs"),
            is_dir: true,
            mtime: None,
            size: 0,
        },
        Entry {
            path: PathBuf::from("docs/setup.md"),
            is_dir: false,
            mtime: None,
            size: 0,
        },
    ]));

    assert!(!app.vault.tree.complete);
    assert_eq!(rows(&app), ["docs"]);
    assert!(frame(&mut app, 120, 40).contains("docs"));
}

/// A vault of `files` Markdown files spread over ten directories.
///
/// Generated at test time rather than committed, for the reason in Section
/// 15.4: a vault this size in the repository costs every clone.
fn generated_vault(files: usize) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/vault/generated")
        .join(format!("vault-{files}"));

    // The marker lives beside the vault, not inside it: a file in the vault
    // would show up in the tree the tests are asserting on.
    let marker = root.with_extension("complete");
    if marker.exists() {
        return root;
    }

    for dir in 0..10 {
        std::fs::create_dir_all(root.join(format!("dir-{dir:02}")))
            .expect("the fixture directory is writable");
    }

    for i in 0..files {
        let path = root.join(format!("dir-{:02}/note-{i:05}.md", i % 10));
        std::fs::write(&path, format!("# Note {i}\n\nSome prose.\n"))
            .expect("the fixture is writable");
    }

    std::fs::write(&marker, "").expect("the fixture is writable");
    root
}
