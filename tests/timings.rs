//! The performance targets in Section 14, measured.
//!
//! Every test here is `#[ignore]`d and prints rather than asserts. Shared CI
//! runners have unpredictable performance; gating `main` on a wall-clock number
//! produces flaky failures, and a flaky failure gets "fixed" by loosening the
//! assertion until it proves nothing. The nightly `bench.yml` workflow runs
//! these with `--ignored` and uploads what they print.
//!
//! Run them by hand with:
//!
//! ```sh
//! cargo test --release --test timings -- --ignored --nocapture
//! ```

mod common;

use std::path::{Path, PathBuf};
use std::time::Instant;

use common::{app, frame, walk};

/// Report one measurement against its target.
fn report(what: &str, took: std::time::Duration, target_ms: u64) {
    let over = took.as_millis() > u128::from(target_ms);
    println!(
        "{what}: {:.1} ms (target {} ms){}",
        took.as_secs_f64() * 1000.0,
        target_ms,
        if over { "  OVER" } else { "" }
    );
}

/// A generated vault of `files` Markdown files across ten directories.
fn generated_vault(files: usize) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/vault/generated")
        .join(format!("timing-{files}"));

    let marker = root.with_extension("complete");
    if marker.exists() {
        return root;
    }

    for dir in 0..10 {
        std::fs::create_dir_all(root.join(format!("dir-{dir:02}"))).expect("a writable fixture");
    }
    for i in 0..files {
        let body = format!(
            "# Note {i}\n\nSome prose, and a link to [[Note {}]].\n\nMore prose.\n",
            (i + 1) % files.max(1)
        );
        std::fs::write(root.join(format!("dir-{:02}/note-{i:05}.md", i % 10)), body)
            .expect("a writable fixture");
    }
    std::fs::write(&marker, "").expect("a writable fixture");

    root
}

#[test]
#[ignore = "measures rather than asserts; see the module docs"]
fn cold_start_to_first_frame() {
    for (files, target) in [(1_000usize, 50u64), (10_000, 100)] {
        let root = generated_vault(files);

        let started = Instant::now();
        let mut app = app(120, 40);
        app.set_vault_root(&root);
        frame(&mut app, 120, 40);
        let took = started.elapsed();

        report(&format!("first frame, {files} files"), took, target);
        assert!(
            !app.vault.tree.complete,
            "the walk must not have to finish first"
        );
    }
}

#[test]
#[ignore = "measures rather than asserts; see the module docs"]
fn the_whole_vault_is_walked_and_indexed() {
    let root = generated_vault(10_000);

    let mut app = app(120, 40);
    app.set_vault_root(&root);

    let started = Instant::now();
    walk(&mut app);
    report("walk, 10,000 files", started.elapsed(), 3_000);

    let started = Instant::now();
    app.index_now();
    report("index, 10,000 files", started.elapsed(), 3_000);

    assert_eq!(app.vault.index.len(), 10_000);
}

#[test]
#[ignore = "measures rather than asserts; see the module docs"]
fn a_five_megabyte_document_paints_its_first_frame() {
    let path = common::large_document(50_000);
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

    let started = Instant::now();
    let mut app = app(120, 40);
    app.open(perga::doc::document::Document::load(&path).expect("the fixture loads"));
    frame(&mut app, 120, 40);
    let took = started.elapsed();

    report(
        &format!("first frame, a {:.1} MB document", size as f64 / 1e6),
        took,
        100,
    );
}

#[test]
#[ignore = "measures rather than asserts; see the module docs"]
fn scrolling_a_hundred_thousand_lines() {
    let path = common::large_document(50_000);

    let mut app = app(120, 40);
    app.open(perga::doc::document::Document::load(&path).expect("the fixture loads"));
    frame(&mut app, 120, 40);

    // Warm, the way a reader who has scrolled through it once would be.
    for _ in 0..200 {
        app.update(perga::action::Action::ScrollPageDown);
        frame(&mut app, 120, 40);
    }

    let frames = 200;
    let started = Instant::now();
    for _ in 0..frames {
        app.update(perga::action::Action::ScrollPageUp);
        frame(&mut app, 120, 40);
    }
    let per_frame = started.elapsed() / frames;

    // 60 fps is 16.6 ms per frame.
    report("one scrolled frame", per_frame, 17);
}

#[test]
#[ignore = "measures rather than asserts; see the module docs"]
fn the_first_search_results_arrive() {
    let root = generated_vault(10_000);

    let mut app = app(120, 40);
    app.set_vault_root(&root);

    let started = Instant::now();
    app.search_now("prose");
    let took = started.elapsed();

    // The synchronous path runs the *whole* search, so this is the pessimistic
    // number: the streaming one shows its first hits much sooner.
    report("a complete search, 10,000 files", took, 200);
    assert!(!app.search.hits.is_empty());
}
