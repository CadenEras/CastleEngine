//! Runs the `castle-pdf` binary on the public corpus and on invalid inputs.
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers: a failed expectation is a test failure"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

fn corpus(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/corpus/public")
        .join(name)
}

fn inspect(path: &Path) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_castle-pdf"))
        .arg("inspect")
        .arg(path)
        .output()
        .expect("binary runs");
    (
        output.status.code().expect("exit code"),
        String::from_utf8(output.stdout).expect("UTF-8 report"),
        String::from_utf8(output.stderr).expect("UTF-8 errors"),
    )
}

/// Writes `bytes` to a file in Cargo's scratch directory for integration tests.
fn scratch(name: &str, bytes: &[u8]) -> PathBuf {
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    std::fs::write(&path, bytes).expect("scratch file written");
    path
}

#[test]
fn page_tree_report_is_stable() {
    let (code, out, err) = inspect(&corpus("page-tree.pdf"));
    assert_eq!(code, 0, "{err}");
    assert_eq!(err, "");
    assert_eq!(
        out,
        "size: 1109 bytes\n\
         version: 1.4\n\
         startxref: 824\n\
         revisions: 1\n  \
         section 1: xref at 824, 11 entries\n\
         objects: 10 in use, 1 free\n\
         trailer keys: /Root /Size\n\
         root: 1 0 R\n\
         size entry: 11\n\
         pages: 3\n  \
         page 1: object 4 0, media box [0 0 595 842], crop box [0 0 595 842], rotation 90\n  \
         page 2: object 5 0, media box [0 0 842 595], crop box [0 0 842 595], rotation 90\n  \
         page 3: object 6 0, media box [0 0 595 842], crop box [0 0 595 842], rotation 0\n"
    );
}

#[test]
fn incremental_report_lists_both_revisions() {
    let (code, out, _) = inspect(&corpus("incremental.pdf"));
    assert_eq!(code, 0);
    assert!(out.contains(
        "revisions: 2\n  section 1: xref at 715, 1 entries\n  section 2: xref at 425, 6 entries\n"
    ));
    assert!(out.contains("trailer keys: /Prev /Root /Size\n"));
}

#[test]
fn the_report_never_contains_document_content() {
    let (_, out, _) = inspect(&corpus("minimal.pdf"));
    assert!(!out.contains("CastlePDF minimal fixture"));
    assert!(!out.contains("Helvetica"));
}

#[test]
fn unreadable_and_malformed_inputs_exit_with_1() {
    let (code, out, err) = inspect(Path::new("this-file-does-not-exist.pdf"));
    assert_eq!(code, 1);
    assert_eq!(out, "");
    assert!(err.starts_with("error: cannot open"), "{err}");

    let (code, out, err) = inspect(&scratch("not-a-pdf.pdf", b"hello"));
    assert_eq!(code, 1);
    assert_eq!(out, "size: 5 bytes\n");
    assert!(err.contains("malformed PDF syntax at byte 0"), "{err}");
}

#[test]
fn the_structure_is_reported_before_a_page_tree_error() {
    // Valid file structure, but /Count says 2 for one page.
    // Edited in place, byte for byte: the header holds non-UTF-8 bytes and every
    // offset must stay valid.
    let mut bytes = std::fs::read(corpus("minimal.pdf")).expect("fixture");
    let at = bytes
        .windows(8)
        .position(|w| w == b"/Count 1")
        .expect("/Count 1 present");
    if let Some(digit) = bytes.get_mut(at + 7) {
        *digit = b'2';
    }
    let (code, out, err) = inspect(&scratch("bad-count.pdf", &bytes));
    assert_eq!(code, 1);
    assert!(out.contains("objects: 5 in use, 1 free\n"), "{out}");
    assert!(!out.contains("pages:"));
    assert_eq!(
        err,
        "error: invalid document structure at object 2 0: CountMismatch\n"
    );
}
