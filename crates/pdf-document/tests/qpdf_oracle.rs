//! Differential check against qpdf (ADR-0002: oracle for tests, never shipped).
//!
//! Runs only when `CASTLE_QPDF` names a qpdf executable (set by the `oracle`
//! workflow on Linux). For every fixture of tests/corpus/public, all valid,
//! the page objects listed by `qpdf --show-pages`, in order, must equal those
//! of `Document::pages`. A disagreement is analysed, not settled in favour of
//! the oracle.
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers: a failed expectation is a test failure"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use pdf_document::Document;
use pdf_syntax::{Budget, CancellationToken, MemorySource, ResourceLimits};

fn ours(path: &Path) -> Vec<(u32, u16)> {
    let limits = ResourceLimits::default();
    let bytes = std::fs::read(path).expect("fixture readable");
    let source = MemorySource::from_vec(bytes, &limits).expect("within limits");
    let doc = Document::open(source, &mut Budget::new(limits, CancellationToken::new()))
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    doc.pages()
        .iter()
        .map(|p| (p.id.number, p.id.generation))
        .collect()
}

/// Parses the `page N: n g R` lines of `qpdf --show-pages`.
fn theirs(qpdf: &str, path: &Path) -> Vec<(u32, u16)> {
    let output = Command::new(qpdf)
        .arg("--show-pages")
        .arg(path)
        .output()
        .expect("qpdf runs");
    assert!(
        output.status.success(),
        "qpdf --show-pages {}: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("qpdf prints UTF-8")
        .lines()
        .filter(|line| line.starts_with("page "))
        .map(|line| {
            let reference = line
                .split_once(": ")
                .map(|(_, r)| r)
                .unwrap_or_else(|| panic!("unexpected qpdf line: {line}"));
            let mut parts = reference.split(' ');
            match (parts.next(), parts.next(), parts.next()) {
                (Some(n), Some(g), Some("R")) => (
                    n.parse().expect("object number"),
                    g.parse().expect("generation"),
                ),
                _ => panic!("unexpected qpdf line: {line}"),
            }
        })
        .collect()
}

#[test]
fn pages_match_qpdf() {
    let Ok(qpdf) = std::env::var("CASTLE_QPDF") else {
        eprintln!("CASTLE_QPDF not set: qpdf differential test skipped");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/corpus/public");
    let mut compared = 0;
    for entry in std::fs::read_dir(&dir).expect("corpus directory") {
        let path = entry.expect("directory entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("pdf") {
            continue;
        }
        assert_eq!(ours(&path), theirs(&qpdf, &path), "{}", path.display());
        compared += 1;
    }
    assert!(compared > 0, "no fixture compared");
    eprintln!("{compared} fixtures: pages match qpdf");
}
