//! Differential check against qpdf (ADR-0002: oracle for tests, never shipped).
//!
//! Runs only when `CASTLE_QPDF` names a qpdf executable (set by the `oracle`
//! workflow on Linux). For every fixture of tests/corpus/public, all of which
//! are valid files, `qpdf --check` must succeed and the in-use cross-reference
//! entries reported by `qpdf --show-xref` must equal those read by `PdfFile`.
//! A disagreement is analysed, not settled in favour of the oracle.
#![allow(
    clippy::expect_used,
    clippy::panic,
    reason = "test helpers: a failed expectation is a test failure"
)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use pdf_syntax::{Budget, CancellationToken, MemorySource, PdfFile, ResourceLimits, XrefEntry};

/// (number, generation) -> offset of in-use entries.
type Entries = BTreeMap<(u32, u16), u64>;

fn ours(path: &Path) -> Entries {
    let limits = ResourceLimits::default();
    let bytes = std::fs::read(path).expect("fixture readable");
    let source = MemorySource::from_vec(bytes, &limits).expect("within limits");
    let file = PdfFile::open(source, &mut Budget::new(limits, CancellationToken::new()))
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    file.entries()
        .filter_map(|(number, entry)| match entry {
            XrefEntry::InUse { offset, generation } => Some(((number, generation), offset)),
            XrefEntry::Free { .. } => None,
        })
        .collect()
}

fn qpdf(qpdf: &str, args: &[&str], path: &Path) -> String {
    let output = Command::new(qpdf)
        .args(args)
        .arg(path)
        .output()
        .expect("qpdf runs");
    assert!(
        output.status.success(),
        "qpdf {args:?} {}: {}",
        path.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("qpdf prints UTF-8")
}

/// Parses `n/g: uncompressed; offset = X`; other entry kinds are not expected
/// in classic-xref fixtures and fail the comparison.
fn theirs(show_xref: &str) -> Entries {
    show_xref
        .lines()
        .map(|line| {
            let parsed = line
                .split_once(": uncompressed; offset = ")
                .and_then(|(id, offset)| {
                    let (number, generation) = id.split_once('/')?;
                    Some((
                        (number.parse().ok()?, generation.parse().ok()?),
                        offset.trim().parse().ok()?,
                    ))
                });
            parsed.unwrap_or_else(|| panic!("unexpected qpdf line: {line}"))
        })
        .collect()
}

#[test]
fn cross_reference_entries_match_qpdf() {
    let Ok(qpdf_path) = std::env::var("CASTLE_QPDF") else {
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
        qpdf(&qpdf_path, &["--check"], &path);
        let expected = theirs(&qpdf(&qpdf_path, &["--show-xref"], &path));
        assert_eq!(ours(&path), expected, "{}", path.display());
        compared += 1;
    }
    assert!(compared > 0, "no fixture compared");
    eprintln!("{compared} fixtures match qpdf");
}
