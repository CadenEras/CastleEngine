//! `castle-pdf inspect <file>`: structure report, never document content.

use std::fmt::Write as _;
use std::fs::File;
use std::io::{self, Write};
use std::path::Path;

use pdf_document::{Document, Rectangle};
use pdf_syntax::{
    Budget, CancellationToken, MemorySource, Object, PdfFile, ResourceLimits, Source, XrefEntry,
};

/// Writes the report to `out` and errors to `err`; returns the exit code.
/// The file structure is reported even when the page tree turns out invalid.
pub fn run(path: &Path, out: &mut dyn Write, err: &mut dyn Write) -> io::Result<u8> {
    let limits = ResourceLimits::default();
    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) => {
            writeln!(err, "error: cannot open {}: {}", path.display(), e.kind())?;
            return Ok(1);
        }
    };
    let size_hint = file.metadata().ok().map(|m| m.len());
    let source = match MemorySource::from_reader(file, size_hint, &limits) {
        Ok(source) => source,
        Err(e) => {
            writeln!(err, "error: {e}")?;
            return Ok(1);
        }
    };
    // One budget for the whole opening: structure, catalog and page tree.
    let mut budget = Budget::new(limits, CancellationToken::new());
    writeln!(out, "size: {} bytes", Source::len(&source))?;
    let file = match PdfFile::open(source, &mut budget) {
        Ok(file) => file,
        Err(e) => {
            writeln!(err, "error: {e}")?;
            return Ok(1);
        }
    };
    out.write_all(file_report(&file).as_bytes())?;
    match Document::from_file(file, &mut budget) {
        Ok(document) => {
            out.write_all(pages_report(&document).as_bytes())?;
            Ok(0)
        }
        Err(e) => {
            writeln!(err, "error: {e}")?;
            Ok(1)
        }
    }
}

fn file_report<S: Source>(file: &PdfFile<S>) -> String {
    let mut text = String::new();
    let version = file.version();
    let _ = writeln!(text, "version: {}.{}", version.major, version.minor);
    let _ = writeln!(text, "startxref: {}", file.startxref());
    let _ = writeln!(text, "revisions: {}", file.revisions());
    for (i, section) in file.sections().iter().enumerate() {
        let _ = writeln!(
            text,
            "  section {}: xref at {}, {} entries",
            i + 1,
            section.offset,
            section.entries
        );
    }
    let (in_use, free) = file
        .entries()
        .fold((0, 0), |(u, f), (_, entry)| match entry {
            XrefEntry::InUse { .. } => (u + 1, f),
            XrefEntry::Free { .. } => (u, f + 1),
        });
    let _ = writeln!(text, "objects: {in_use} in use, {free} free");
    let trailer = file.trailer();
    let keys: Vec<String> = trailer.iter().map(|(key, _)| name(key)).collect();
    let _ = writeln!(text, "trailer keys: {}", keys.join(" "));
    if let Some(root) = file.root() {
        let _ = writeln!(text, "root: {} {} R", root.number, root.generation);
    }
    if let Some(size) = trailer.get(b"Size").and_then(Object::as_integer) {
        let _ = writeln!(text, "size entry: {size}");
    }
    text
}

fn pages_report<S: Source>(document: &Document<S>) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "pages: {}", document.page_count());
    for (i, page) in document.pages().iter().enumerate() {
        let _ = writeln!(
            text,
            "  page {}: object {} {}, media box {}, crop box {}, rotation {}",
            i + 1,
            page.id.number,
            page.id.generation,
            rectangle(&page.media_box),
            rectangle(&page.crop_box),
            page.rotation
        );
    }
    text
}

fn rectangle(r: &Rectangle) -> String {
    format!("[{} {} {} {}]", r.llx, r.lly, r.urx, r.ury)
}

/// A name as written in PDF, with `#xx` for bytes outside printable ASCII, so
/// that the report stays one line of plain text whatever the input holds.
fn name(bytes: &[u8]) -> String {
    let mut text = String::from("/");
    for &b in bytes {
        if b.is_ascii_graphic() && b != b'#' && b != b'/' {
            text.push(char::from(b));
        } else {
            let _ = write!(text, "#{b:02X}");
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_escaped() {
        assert_eq!(name(b"Root"), "/Root");
        assert_eq!(name(b"A B#/\n\xff"), "/A#20B#23#2F#0A#FF");
    }

    #[test]
    fn rectangles_print_plain_numbers() {
        assert_eq!(
            rectangle(&Rectangle::new(0.0, 0.0, 595.0, 842.5)),
            "[0 0 595 842.5]"
        );
    }
}
