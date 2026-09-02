//! Benchmarks for the vault targets in Section 14.
//!
//! Like `benches/render.rs`, these measure and do not assert; see Section 15.6
//! for why no wall-clock number gates CI.

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use perga::config::schema::FilesConfig;
use perga::vault::tree::Tree;
use perga::vault::walker::{self, Entry, WalkEvent, WalkOptions};

/// Vault sizes the targets in Section 14 are stated for.
const SIZES: [usize; 2] = [1_000, 10_000];

/// A generated vault of `files` Markdown files spread over ten directories.
///
/// Written under `target/` rather than into the repository: a vault this size
/// is a build artifact, not a fixture worth committing.
fn generated_vault(files: usize) -> PathBuf {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("target/bench-vaults")
        .join(format!("vault-{files}"));

    let marker = root.with_extension("complete");
    if marker.exists() {
        return root;
    }

    for dir in 0..10 {
        std::fs::create_dir_all(root.join(format!("dir-{dir:02}"))).expect("a writable target dir");
    }
    for i in 0..files {
        let path = root.join(format!("dir-{:02}/note-{i:05}.md", i % 10));
        std::fs::write(&path, format!("# Note {i}\n\nSome prose.\n")).expect("a writable file");
    }
    std::fs::write(&marker, "").expect("a writable file");

    root
}

/// Walk a vault to completion, collecting what it reports.
fn collect(root: &Path) -> Vec<Entry> {
    let entries = Mutex::new(Vec::new());

    walker::walk(
        root,
        WalkOptions::default(),
        &AtomicBool::new(false),
        &|event| {
            if let WalkEvent::Entries(batch) = event {
                entries.lock().unwrap().extend(batch);
            }
        },
    );

    entries.into_inner().unwrap()
}

/// Walking the whole vault.
///
/// This runs off-thread in the application, so it is not in the first-frame
/// budget; it is the number behind "the tree is fully populated by".
fn walk_vault(c: &mut Criterion) {
    let mut group = c.benchmark_group("walk_vault");

    for files in SIZES {
        let root = generated_vault(files);

        group.throughput(Throughput::Elements(files as u64));
        group.bench_function(format!("{files}_files"), |b| {
            b.iter(|| black_box(collect(&root).len()));
        });
    }

    group.finish();
}

/// Turning a walk's output into a tree and flattening the visible rows.
///
/// This part *is* in the frame budget: it happens on the main thread as each
/// batch lands, and again for every frame that draws the sidebar.
fn first_frame(c: &mut Criterion) {
    let config = FilesConfig::default();
    let mut group = c.benchmark_group("first_frame");

    for files in SIZES {
        let entries = collect(&generated_vault(files));

        group.throughput(Throughput::Elements(files as u64));
        group.bench_function(format!("tree_{files}_files"), |b| {
            b.iter(|| {
                let mut tree = Tree::new(&config);
                tree.insert_all(entries.iter().cloned());
                // With everything collapsed, only the top level is drawn —
                // which is the point of the lazy tree.
                black_box(tree.rows().len())
            });
        });

        group.bench_function(format!("rows_{files}_files"), |b| {
            let mut tree = Tree::new(&config);
            tree.insert_all(entries.iter().cloned());
            b.iter(|| black_box(tree.rows().len()));
        });
    }

    group.finish();
}

criterion_group!(benches, walk_vault, first_frame);
criterion_main!(benches);
