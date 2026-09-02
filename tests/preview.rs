//! Captures the frame the social preview image is rendered from.
//!
//! Ignored by default: it writes a file rather than asserting anything, and it
//! is run by hand when the interface changes visibly. See `demo/README.md` for
//! the two commands that turn its output into `demo/social-preview.png`.

mod common;

/// Write a 120x22 frame of perga reading a fixture document.
#[test]
#[ignore = "writes demo/social-preview-frame.txt; run by hand"]
fn write_social_preview_frame() {
    let mut app = common::app_with("docs/api/auth.md", 120, 22);
    app.index_now();

    // Measured once so the tree is expanded to the open document before the
    // frame that gets captured.
    common::frame(&mut app, 120, 22);
    let painted = common::frame(&mut app, 120, 22);

    let path =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("demo/social-preview-frame.txt");
    std::fs::write(&path, painted).expect("demo/ is writable");

    eprintln!("wrote {}", path.display());
}
