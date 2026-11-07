use proptest::prelude::*;

use super::*;
use crate::limits::{CancellationToken, LimitKind, ResourceLimits};
use crate::source::MemorySource;

/// Builds a valid classic-xref file: object n is `bodies[n - 1]`.
/// Returns the bytes and the offset of `xref`.
fn build(header: &str, bodies: &[&str], trailer_extra: &str) -> (Vec<u8>, u64) {
    let mut out = format!("{header}\n%\u{e2}\u{e3}\n").into_bytes();
    let mut offsets = Vec::new();
    for (i, body) in bodies.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len() as u64;
    let size = bodies.len() + 1;
    out.extend_from_slice(format!("xref\n0 {size}\n0000000000 65535 f \n").as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {size} /Root 1 0 R{trailer_extra} >>\nstartxref\n{xref}\n%%EOF\n"
        )
        .as_bytes(),
    );
    (out, xref)
}

fn simple(bodies: &[&str]) -> Vec<u8> {
    build("%PDF-1.7", bodies, "").0
}

/// Appends an incremental update replacing or adding `objects`.
fn update(mut base: Vec<u8>, prev: u64, objects: &[(u32, &str)], size: u32) -> (Vec<u8>, u64) {
    let mut entries = String::new();
    for (number, body) in objects {
        let offset = base.len();
        base.extend_from_slice(format!("{number} 0 obj\n{body}\nendobj\n").as_bytes());
        entries.push_str(&format!("{number} 1\n{offset:010} 00000 n \n"));
    }
    let xref = base.len() as u64;
    base.extend_from_slice(
        format!(
            "xref\n{entries}trailer\n<< /Size {size} /Root 1 0 R /Prev {prev} >>\n\
             startxref\n{xref}\n%%EOF\n"
        )
        .as_bytes(),
    );
    (base, xref)
}

fn budget() -> Budget {
    Budget::new(ResourceLimits::default(), CancellationToken::new())
}

fn open(bytes: &[u8]) -> Result<PdfFile<MemorySource>, Error> {
    open_with(bytes, &mut budget())
}

fn open_with(bytes: &[u8], budget: &mut Budget) -> Result<PdfFile<MemorySource>, Error> {
    let source = MemorySource::from_vec(bytes.to_vec(), &ResourceLimits::default())?;
    PdfFile::open(source, budget)
}

fn replace(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes).into_owned();
    assert!(text.contains(from), "{from:?} not found");
    text.replacen(from, to, 1).into_bytes()
}

fn kind(result: Result<PdfFile<MemorySource>, Error>) -> Option<MalformedKind> {
    match result {
        Err(Error::Malformed { kind, .. }) => Some(kind),
        other => panic!("expected Malformed, got {other:?}"),
    }
}

fn id(number: u32, generation: u16) -> ObjectId {
    ObjectId { number, generation }
}

fn value(file: &PdfFile<MemorySource>, number: u32) -> Result<IndirectValue, Error> {
    file.resolve(id(number, 0), &mut budget())
}

const CATALOG: &str = "<< /Type /Catalog /Pages 2 0 R >>";
const PAGES: &str = "<< /Type /Pages /Kids [] /Count 0 >>";

#[test]
fn opens_a_simple_file_and_resolves_objects() {
    let file = open(&simple(&[CATALOG, PAGES])).expect("valid file");
    assert_eq!(file.version(), Version { major: 1, minor: 7 });
    assert_eq!(file.root(), Some(id(1, 0)));
    assert_eq!(file.revisions(), 1);
    assert_eq!(file.entries().count(), 3);
    assert_eq!(
        file.entry(0),
        Some(XrefEntry::Free {
            next: 0,
            generation: 65535
        })
    );
    let Ok(IndirectValue::Object(Object::Dictionary(catalog))) = value(&file, 1) else {
        panic!("catalog expected");
    };
    assert_eq!(
        catalog.get(b"Type"),
        Some(&Object::Name(b"Catalog".to_vec()))
    );
    // Free, absent and wrong-generation references are references to null.
    for target in [id(3, 0), id(1, 1), id(99, 0)] {
        assert_eq!(
            file.resolve(target, &mut budget()),
            Ok(IndirectValue::Object(Object::Null))
        );
    }
}

#[test]
fn accepted_headers_and_versions() {
    for (header, version) in [
        ("%PDF-1.0", (1, 0)),
        ("%PDF-1.4", (1, 4)),
        ("%PDF-2.0", (2, 0)),
    ] {
        let file = open(&build(header, &[CATALOG], "").0).expect(header);
        assert_eq!(
            file.version(),
            Version {
                major: version.0,
                minor: version.1
            }
        );
    }
    assert_eq!(
        open(&build("%PDF-1.8", &[CATALOG], "").0).err(),
        Some(Error::Unsupported(Capability::PdfVersion))
    );
    assert_eq!(
        open(&build("%PDF-3.0", &[CATALOG], "").0).err(),
        Some(Error::Unsupported(Capability::PdfVersion))
    );
}

#[test]
fn invalid_headers() {
    let valid = simple(&[CATALOG]);
    assert_eq!(
        kind(open(&replace(&valid, "%PDF-1.7", "%PDF-1x7"))),
        Some(MalformedKind::InvalidHeader)
    );
    assert_eq!(
        kind(open(&replace(&valid, "%PDF-1.7", "%FDP-1.7"))),
        Some(MalformedKind::InvalidHeader)
    );
    // Strict: nothing before the header, not even white space.
    assert_eq!(
        kind(open(&[b" ", valid.as_slice()].concat())),
        Some(MalformedKind::InvalidHeader)
    );
    assert_eq!(open(b"").err(), Some(Error::Truncated { offset: 0 }));
    assert_eq!(open(b"%PDF-1").err(), Some(Error::Truncated { offset: 0 }));
}

#[test]
fn startxref_and_eof() {
    let (valid, xref) = build("%PDF-1.7", &[CATALOG], "");
    let tail = format!("startxref\n{xref}\n%%EOF\n");
    let with_tail = |new_tail: &str| replace(&valid, &tail, new_tail);
    assert!(open(&with_tail(&format!("startxref\r\n{xref}\r\n%%EOF"))).is_ok());
    assert_eq!(
        kind(open(&with_tail(&format!(
            "startxref\n{xref}\n%%EOF\ngarbage"
        )))),
        Some(MalformedKind::MissingEof)
    );
    assert_eq!(
        open(&with_tail(&format!("startxref\n{xref}\n%%EO"))).err(),
        Some(Error::Truncated {
            offset: valid.len() as u64 - tail.len() as u64
        })
    );
    assert_eq!(
        open(&with_tail(&format!("startxref\n{xref}\n"))).err(),
        Some(Error::Truncated {
            offset: valid.len() as u64 - tail.len() as u64
        })
    );
    assert_eq!(
        kind(open(&with_tail("startxref\n999999\n%%EOF\n"))),
        Some(MalformedKind::MissingStartxref)
    );
    assert_eq!(
        kind(open(&with_tail("startxref\n-1\n%%EOF\n"))),
        Some(MalformedKind::MissingStartxref)
    );
    assert_eq!(
        kind(open(&with_tail("%%EOF\n"))),
        Some(MalformedKind::MissingStartxref)
    );
    // `startxref` more than 1024 bytes before the end is not searched for.
    let padding = " ".repeat(1100);
    assert_eq!(
        kind(open(&with_tail(&format!(
            "startxref\n{xref}\n%%EOF\n%{padding}\n"
        )))),
        Some(MalformedKind::MissingStartxref)
    );
    let far = format!("startxref\n{xref}\n{padding}%%EOF\n");
    assert_eq!(
        kind(open(&with_tail(&far))),
        Some(MalformedKind::MissingStartxref)
    );
}

#[test]
fn startxref_must_point_at_a_classic_table() {
    let (valid, xref) = build("%PDF-1.7", &[CATALOG], "");
    let point_at = |offset: u64| {
        replace(
            &valid,
            &format!("startxref\n{xref}\n"),
            &format!("startxref\n{offset}\n"),
        )
    };
    // At the catalog object: a cross-reference stream would start like this.
    let object_offset = valid
        .windows(7)
        .position(|w| w == b"1 0 obj")
        .expect("object") as u64;
    assert_eq!(
        open(&point_at(object_offset)).err(),
        Some(Error::Unsupported(Capability::XrefStreams))
    );
    assert_eq!(
        kind(open(&point_at(object_offset + 8))),
        Some(MalformedKind::MissingXref)
    );
    assert_eq!(
        kind(open(&point_at(object_offset + 2))),
        Some(MalformedKind::MissingXref)
    );
}

#[test]
fn invalid_entries() {
    let valid = simple(&[CATALOG, PAGES]);
    let first = "0000000000 65535 f \n";
    let check = |from: &str, to: &str| kind(open(&replace(&valid, from, to)));
    // 19 bytes, a wrong EOL, a bad kind, a non-digit, a generation over 65535.
    assert_eq!(
        check(first, "0000000000 65535 f\n"),
        Some(MalformedKind::InvalidXrefEntry)
    );
    assert_eq!(
        check(first, "0000000000 65535 f\n\n"),
        Some(MalformedKind::InvalidXrefEntry)
    );
    assert_eq!(
        check(first, "0000000000 65535 x \n"),
        Some(MalformedKind::InvalidXrefEntry)
    );
    assert_eq!(
        check(first, "00000000a0 65535 f \n"),
        Some(MalformedKind::InvalidXrefEntry)
    );
    assert_eq!(
        check(first, "0000000000 70000 f \n"),
        Some(MalformedKind::InvalidXrefEntry)
    );
    // All three valid end-of-line forms.
    for eol in [" \r", " \n", "\r\n"] {
        let entry = format!("0000000000 65535 f{eol}");
        assert!(open(&replace(&valid, first, &entry)).is_ok(), "{eol:?}");
    }
    // Object 0 in use, and an in-use offset past the end of the file.
    assert_eq!(
        check(first, "0000000000 00000 n \n"),
        Some(MalformedKind::InvalidXrefEntry)
    );
}

#[test]
fn entry_offsets_and_numbers_are_checked() {
    let (valid, _) = build("%PDF-1.7", &[CATALOG, PAGES], "");
    // An offset beyond the file.
    let text = String::from_utf8_lossy(&valid).into_owned();
    let line = text
        .lines()
        .find(|l| l.ends_with(" 00000 n "))
        .expect("in-use entry")
        .to_owned();
    let far = format!("{:010} 00000 n ", valid.len() + 10);
    assert_eq!(
        kind(open(&replace(&valid, &line, &far))),
        Some(MalformedKind::InvalidXrefEntry)
    );
    // A number not below /Size.
    assert_eq!(
        kind(open(&replace(&valid, "/Size 3", "/Size 2"))),
        Some(MalformedKind::InvalidXrefEntry)
    );
    // An offset that points at another object.
    let other = text
        .lines()
        .filter(|l| l.ends_with(" 00000 n "))
        .nth(1)
        .expect("second entry")
        .to_owned();
    let swapped = replace(&valid, &line, &other);
    let file = open(&swapped).expect("structure still valid");
    assert!(matches!(
        value(&file, 1),
        Err(Error::Malformed {
            kind: MalformedKind::ObjectIdMismatch,
            ..
        })
    ));
}

#[test]
fn invalid_subsections() {
    let valid = simple(&[CATALOG]);
    let check = |from: &str, to: &str| open(&replace(&valid, from, to)).err();
    let malformed_kind = |e: Option<Error>| match e {
        Some(Error::Malformed { kind, .. }) => Some(kind),
        _ => None,
    };
    assert_eq!(
        malformed_kind(check("xref\n0 2\n", "xref\n-1 2\n")),
        Some(MalformedKind::InvalidXrefSubsection)
    );
    assert_eq!(
        malformed_kind(check("xref\n0 2\n", "xref\n0 2 x\n")),
        Some(MalformedKind::InvalidXrefSubsection)
    );
    assert_eq!(
        malformed_kind(check("xref\n0 2\n", "xref\n0 /A\n")),
        Some(MalformedKind::InvalidXrefSubsection)
    );
    assert_eq!(
        malformed_kind(check("xref\n0 2\n", "xref\n4294967295 2\n")),
        Some(MalformedKind::InvalidXrefSubsection)
    );
    // More entries than bytes left in the file.
    assert!(matches!(
        check("xref\n0 2\n", "xref\n0 99999999\n"),
        Some(Error::Truncated { .. })
    ));
    // Spaces before the EOL are tolerated, a missing EOL is not.
    assert!(check("xref\n0 2\n", "xref\n0 2  \n").is_none());
    assert_eq!(
        malformed_kind(check("xref\n0 2\n0", "xref\n0 2 0")),
        Some(MalformedKind::InvalidXrefSubsection)
    );
}

#[test]
fn trailer_requirements_and_unsupported_capabilities() {
    let valid = simple(&[CATALOG]);
    assert_eq!(
        kind(open(&replace(&valid, "/Root 1 0 R", "/Root 1"))),
        Some(MalformedKind::InvalidTrailer)
    );
    assert_eq!(
        kind(open(&replace(&valid, "/Size 2", "/Size 0"))),
        Some(MalformedKind::InvalidTrailer)
    );
    assert_eq!(
        kind(open(&replace(
            &valid,
            "trailer\n<< /Size 2 /Root 1 0 R >>",
            "trailer\n(not a dictionary)"
        ))),
        Some(MalformedKind::InvalidTrailer)
    );
    assert_eq!(
        kind(open(&replace(&valid, "trailer\n", "trailor\n"))),
        Some(MalformedKind::InvalidXrefSubsection)
    );
    assert_eq!(
        open(&build("%PDF-1.7", &[CATALOG], " /Encrypt << >>").0).err(),
        Some(Error::Unsupported(Capability::Encryption))
    );
    assert_eq!(
        open(&build("%PDF-1.7", &[CATALOG], " /XRefStm 0").0).err(),
        Some(Error::Unsupported(Capability::XrefStreams))
    );
}

#[test]
fn incremental_updates_override_older_entries() {
    let (base, xref) = build("%PDF-1.7", &[CATALOG, PAGES], "");
    let (updated, _) = update(
        base,
        xref,
        &[
            (2, "<< /Type /Pages /Kids [] /Count 0 /New true >>"),
            (3, "(added)"),
        ],
        4,
    );
    let file = open(&updated).expect("valid update");
    assert_eq!(file.revisions(), 2);
    let Ok(IndirectValue::Object(Object::Dictionary(pages))) = value(&file, 2) else {
        panic!("pages expected");
    };
    assert_eq!(pages.get(b"New"), Some(&Object::Boolean(true)));
    assert_eq!(
        value(&file, 3),
        Ok(IndirectValue::Object(Object::String(b"added".to_vec())))
    );
    // Object 1 still comes from the original section.
    assert!(matches!(
        value(&file, 1),
        Ok(IndirectValue::Object(Object::Dictionary(_)))
    ));
}

#[test]
fn prev_cycles_and_invalid_prev() {
    let (base, xref) = build("%PDF-1.7", &[CATALOG], "");
    // A section whose /Prev points at itself.
    let (looped, second) = update(base.clone(), xref, &[(1, CATALOG)], 2);
    // The newest trailer follows its own table, so editing it moves no offset.
    let self_loop = replace(
        &looped,
        &format!("/Prev {xref}"),
        &format!("/Prev {second}"),
    );
    assert_eq!(
        open(&self_loop).err(),
        Some(Error::Malformed {
            offset: second,
            kind: MalformedKind::XrefPrevCycle
        })
    );
    // Two sections pointing at each other, through a fixed-width placeholder.
    let (base, xref) = build("%PDF-1.7", &[CATALOG], " /Prev 0000000000");
    let (two, newest) = update(base, xref, &[(1, CATALOG)], 2);
    let cycle = replace(&two, "/Prev 0000000000", &format!("/Prev {newest:010}"));
    assert_eq!(
        open(&cycle).err(),
        Some(Error::Malformed {
            offset: newest,
            kind: MalformedKind::XrefPrevCycle
        })
    );
    // A /Prev that is not a valid offset.
    let (updated, _) = update(simple(&[CATALOG]), xref, &[(1, CATALOG)], 2);
    assert_eq!(
        kind(open(&replace(
            &updated,
            &format!("/Prev {xref}"),
            "/Prev (x)"
        ))),
        Some(MalformedKind::InvalidTrailer)
    );
}

#[test]
fn streams_and_lengths() {
    let file_bytes = simple(&[
        CATALOG,
        "<< /Length 5 >>\nstream\nhello\nendstream",
        "<< /Length 4 0 R >>\nstream\nabc\nendstream",
        "3",
        "<< /Length 2 0 R >>\nstream\nhello\nendstream",
        "<< /Length 6 0 R >>\nstream\nhello\nendstream",
        "<< /Length 9 0 R >>\nstream\nhello\nendstream",
    ]);
    let file = open(&file_bytes).expect("valid file");
    let data = |number| match value(&file, number) {
        Ok(IndirectValue::Stream(s)) => file.stream_data(&s).map(|d| d.into_owned()),
        other => panic!("stream expected, got {other:?}"),
    };
    assert_eq!(data(2), Ok(b"hello".to_vec()));
    // Indirect /Length resolved through the table.
    assert_eq!(data(3), Ok(b"abc".to_vec()));
    // /Length pointing at a stream, at itself, or at an absent object.
    for number in [5, 6, 7] {
        assert!(
            matches!(
                value(&file, number),
                Err(Error::Malformed {
                    kind: MalformedKind::InvalidStreamLength,
                    ..
                })
            ),
            "object {number}"
        );
    }
}

#[test]
fn entries_are_charged_and_cancellation_is_honoured() {
    // 3 entries + 1 section, then the trailer: 1 dictionary + 2 keys + 2 values.
    let bytes = simple(&[CATALOG, PAGES]);
    let mut exact = Budget::new(
        ResourceLimits {
            max_objects_visited: 9,
            ..ResourceLimits::default()
        },
        CancellationToken::new(),
    );
    // /Root 1 0 R is one object: 1 + 3 + (1 + 2 + 2) = 9.
    assert!(open_with(&bytes, &mut exact).is_ok());
    assert_eq!(exact.objects_visited(), 9);
    let mut short = Budget::new(
        ResourceLimits {
            max_objects_visited: 8,
            ..ResourceLimits::default()
        },
        CancellationToken::new(),
    );
    assert_eq!(
        open_with(&bytes, &mut short).err(),
        Some(Error::LimitExceeded {
            kind: LimitKind::ObjectsVisited,
            limit: 8
        })
    );
    let token = CancellationToken::new();
    token.cancel();
    let mut cancelled = Budget::new(ResourceLimits::default(), token);
    assert_eq!(
        open_with(&bytes, &mut cancelled).err(),
        Some(Error::Cancelled)
    );
}

proptest! {
    /// Corrupting any byte of a valid file never panics; whatever opens can be
    /// resolved entry by entry without leaving the file.
    #[test]
    fn corrupted_files_are_handled(position in any::<prop::sample::Index>(), byte in any::<u8>()) {
        let mut bytes = simple(&[
            CATALOG,
            PAGES,
            "<< /Length 4 0 R >>\nstream\nabc\nendstream",
            "3",
        ]);
        let index = position.index(bytes.len());
        if let Some(b) = bytes.get_mut(index) {
            *b = byte;
        }
        if let Ok(file) = open(&bytes) {
            let numbers: Vec<u32> = file.entries().map(|(n, _)| n).collect();
            for number in numbers.into_iter().filter(|n| *n > 0) {
                if let Ok(IndirectValue::Stream(s)) = file.resolve(id(number, 0), &mut budget()) {
                    prop_assert!(file.stream_data(&s).is_ok());
                }
            }
        }
    }
}
