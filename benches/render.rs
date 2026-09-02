//! Benchmarks for the rendering targets in Section 14.
//!
//! These measure; they do not assert. Shared CI runners have unpredictable
//! performance, and gating `main` on wall-clock numbers produces flaky
//! failures that get "fixed" by loosening the threshold until it proves
//! nothing. The nightly `bench.yml` workflow runs these and warns on a
//! regression.

use std::fmt::Write as _;
use std::hint::black_box;
use std::path::PathBuf;
use std::time::SystemTime;

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use perga::doc::document::Document;
use perga::doc::highlight::Highlighter;
use perga::doc::render::{RenderedDocument, Renderer};
use perga::theme::Theme;

/// The width the benchmarks render at: a comfortable terminal, minus the
/// sidebar and both borders.
const WIDTH: u16 = 86;
/// The viewport height the scrolling benchmark uses.
const HEIGHT: u16 = 38;

/// Build a document of roughly `lines` source lines.
fn corpus(lines: usize) -> Document {
    let mut source = String::with_capacity(lines * 48);
    source.push_str("# A large document\n\n");

    for i in 0..lines {
        match i % 25 {
            0 => {
                let _ = writeln!(source, "## Section {}\n", i / 25);
            }
            7 => {
                let _ = writeln!(
                    source,
                    "```rust\nfn function_{i}() -> usize {{ {i} }}\n```\n"
                );
            }
            13 => {
                let _ = writeln!(source, "- a list item, number {i}\n");
            }
            _ => {
                let _ = writeln!(
                    source,
                    "Paragraph {i}: prose long enough that it wraps on a narrow \
                     terminal and short enough to stay readable.\n"
                );
            }
        }
    }

    Document::from_source(
        PathBuf::from("bench.md"),
        source,
        SystemTime::UNIX_EPOCH,
        false,
        None,
    )
}

/// A renderer with syntax highlighting already loaded, so the benchmark
/// measures rendering rather than the one-off cost of loading syntect.
fn renderer() -> Renderer {
    let highlighter = Highlighter::new();
    highlighter.load_blocking();
    Renderer::new(&Theme::dark(), highlighter, WIDTH)
}

/// Parsing a document into blocks.
fn parse_document(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse_document");

    for lines in [1_000usize, 10_000, 100_000] {
        let document = corpus(lines);
        let source = document.source.clone();

        group.throughput(Throughput::Bytes(source.len() as u64));
        group.bench_function(format!("{lines}_lines"), |b| {
            b.iter(|| {
                let parsed = Document::from_source(
                    PathBuf::from("bench.md"),
                    source.clone(),
                    SystemTime::UNIX_EPOCH,
                    false,
                    None,
                );
                black_box(parsed.blocks.len())
            });
        });
    }

    group.finish();
}

/// Painting the first frame of a freshly opened document.
///
/// This is the number Section 14's "open a 5 MB document in under 100 ms"
/// target is about: only the visible blocks are rendered, so it must not grow
/// with the size of the document.
fn first_frame(c: &mut Criterion) {
    let renderer = renderer();
    let mut group = c.benchmark_group("first_frame");

    for lines in [1_000usize, 10_000, 100_000] {
        let document = corpus(lines);

        group.bench_function(format!("{lines}_lines"), |b| {
            b.iter(|| {
                let mut layout = RenderedDocument::new();
                black_box(layout.window(&document, &renderer, 0, HEIGHT).len())
            });
        });
    }

    group.finish();
}

/// Scrolling a large document a screen at a time.
///
/// Section 14 asks for a sustained 60 fps, which is 16.6 ms per frame. Each
/// iteration here is one frame's worth of work with a warm cache.
fn scroll_large_document(c: &mut Criterion) {
    let renderer = renderer();
    let document = corpus(100_000);

    let mut layout = RenderedDocument::new();
    // Warm the cache the way a reader scrolling through the document would.
    while !layout.resolve_all(&document, &renderer) {}
    let total = layout
        .total_lines(&document)
        .expect("the document is measured");

    let mut group = c.benchmark_group("scroll_large_document");
    group.throughput(Throughput::Elements(u64::from(HEIGHT)));

    group.bench_function("warm_cache", |b| {
        let mut at = 0usize;
        b.iter(|| {
            at = (at + usize::from(HEIGHT)) % total.saturating_sub(usize::from(HEIGHT)).max(1);
            black_box(layout.window(&document, &renderer, at, HEIGHT).len())
        });
    });

    group.bench_function("cold_cache", |b| {
        b.iter(|| {
            let mut cold = RenderedDocument::new();
            black_box(cold.window(&document, &renderer, 0, HEIGHT).len())
        });
    });

    group.finish();
}

criterion_group!(benches, parse_document, first_frame, scroll_large_document);
criterion_main!(benches);
