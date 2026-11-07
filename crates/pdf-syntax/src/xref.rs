//! File structure: header, classic cross-reference tables, trailers and
//! incremental updates (ISO 32000-2:2020, 7.5).
//!
//! Strict (ADR-0002): the header must start the file, `startxref` must lead to a
//! well-formed `xref` table, entries are exactly 20 bytes, and `/Prev` chains
//! must not loop. No reconstruction is attempted. Cross-reference streams,
//! hybrid files (`/XRefStm`) and encryption are refused as unsupported.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};

use crate::lexer::{Lexer, Spanned, Token, is_whitespace};
use crate::limits::{Budget, Capability, Error, MalformedKind};
use crate::object::{Dictionary, IndirectObject, IndirectValue, Object, ObjectId, Stream};
use crate::parser::Parser;
use crate::source::Source;

/// `startxref` is searched in this many bytes at the end of the file.
const STARTXREF_WINDOW: u64 = 1024;
/// Length of one cross-reference entry, end-of-line included.
const ENTRY_LEN: usize = 20;
/// Cancellation is checked every this many entries.
const CANCEL_CHECK_ENTRIES: u64 = 4096;

/// PDF version from the file header (`%PDF-1.7`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version {
    pub major: u8,
    pub minor: u8,
}

/// One cross-reference entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum XrefEntry {
    /// `f`: object number of the next free object, and generation for reuse.
    Free { next: u32, generation: u16 },
    /// `n`: byte offset of `number generation obj` in the file.
    InUse { offset: u64, generation: u16 },
}

/// A PDF file opened through its cross-reference tables: entries merged from
/// all revisions (newest wins) and the newest trailer.
///
/// ```
/// use pdf_syntax::{Budget, CancellationToken, MemorySource, PdfFile, ResourceLimits};
///
/// let limits = ResourceLimits::default();
/// let bytes = b"%PDF-1.7\n1 0 obj\n<< /Type /Catalog >>\nendobj\n\
///     xref\n0 2\n0000000000 65535 f \n0000000009 00000 n \n\
///     trailer\n<< /Size 2 /Root 1 0 R >>\nstartxref\n45\n%%EOF\n";
/// let source = MemorySource::from_vec(bytes.to_vec(), &limits)?;
/// let mut budget = Budget::new(limits, CancellationToken::new());
/// let file = PdfFile::open(source, &mut budget)?;
/// assert_eq!(file.root().map(|id| id.number), Some(1));
/// # Ok::<(), pdf_syntax::Error>(())
/// ```
#[derive(Debug)]
pub struct PdfFile<S> {
    source: S,
    version: Version,
    entries: BTreeMap<u32, XrefEntry>,
    trailer: Dictionary,
    revisions: usize,
}

impl<S: Source> PdfFile<S> {
    /// Reads the header, the `startxref` pointer and every cross-reference
    /// section of the `/Prev` chain. Objects are parsed later, on demand.
    pub fn open(source: S, budget: &mut Budget) -> Result<Self, Error> {
        let data = source.read(0..source.len())?;
        let version = parse_header(&data)?;
        let start = find_startxref(&data, budget)?;
        let (entries, trailer, revisions) = read_sections(&data, start, budget)?;
        drop(data);
        Ok(Self {
            source,
            version,
            entries,
            trailer,
            revisions,
        })
    }

    pub fn version(&self) -> Version {
        self.version
    }

    /// Trailer of the newest revision.
    pub fn trailer(&self) -> &Dictionary {
        &self.trailer
    }

    /// `/Root` of the newest trailer, checked as a reference when opening.
    pub fn root(&self) -> Option<ObjectId> {
        self.trailer.get(b"Root").and_then(Object::as_reference)
    }

    /// Number of cross-reference sections read (1 + incremental updates).
    pub fn revisions(&self) -> usize {
        self.revisions
    }

    pub fn entry(&self, number: u32) -> Option<XrefEntry> {
        self.entries.get(&number).copied()
    }

    /// All entries, by increasing object number.
    pub fn entries(&self) -> impl Iterator<Item = (u32, XrefEntry)> + '_ {
        self.entries.iter().map(|(n, e)| (*n, *e))
    }

    /// Parses the object `id` from its cross-reference entry.
    ///
    /// A reference to an object that is free, absent, or whose generation does
    /// not match is a reference to `null` (ISO 32000-2, 7.3.10). An object whose
    /// header does not carry `id` is malformed. An indirect `/Length` must lead
    /// to a direct integer; it is never resolved through another indirection.
    pub fn resolve(&self, id: ObjectId, budget: &mut Budget) -> Result<IndirectValue, Error> {
        let Some(offset) = self.locate(id) else {
            return Ok(IndirectValue::Object(Object::Null));
        };
        let data = self.source.read(0..self.source.len())?;
        let mut length = |length_id: ObjectId, budget: &mut Budget| {
            self.direct_length(&data, offset, length_id, budget)
        };
        let object = parse_at(&data, offset, budget, &mut length)?;
        check_id(&object, id, offset)?;
        Ok(object.value)
    }

    /// Encoded bytes of a stream, as located by `resolve`; filters are not applied.
    pub fn stream_data(&self, stream: &Stream) -> Result<Cow<'_, [u8]>, Error> {
        self.source.read(stream.data.clone())
    }

    fn locate(&self, id: ObjectId) -> Option<u64> {
        match self.entries.get(&id.number) {
            Some(XrefEntry::InUse { offset, generation }) if *generation == id.generation => {
                Some(*offset)
            }
            _ => None,
        }
    }

    fn direct_length(
        &self,
        data: &[u8],
        stream_offset: u64,
        id: ObjectId,
        budget: &mut Budget,
    ) -> Result<i64, Error> {
        let invalid = malformed(stream_offset, MalformedKind::InvalidStreamLength);
        let offset = self.locate(id).ok_or(invalid.clone())?;
        let mut refuse = |_: ObjectId, _: &mut Budget| Err(invalid.clone());
        let object = parse_at(data, offset, budget, &mut refuse)?;
        check_id(&object, id, offset)?;
        match object.value {
            IndirectValue::Object(Object::Integer(length)) => Ok(length),
            _ => Err(invalid),
        }
    }
}

fn parse_at(
    data: &[u8],
    offset: u64,
    budget: &mut Budget,
    resolve_length: &mut crate::parser::LengthResolver<'_>,
) -> Result<IndirectObject, Error> {
    let mut parser = Parser::new(data);
    parser.seek(offset)?;
    parser.parse_indirect_object(budget, resolve_length)
}

fn check_id(object: &IndirectObject, expected: ObjectId, offset: u64) -> Result<(), Error> {
    if object.id == expected {
        Ok(())
    } else {
        Err(malformed(offset, MalformedKind::ObjectIdMismatch))
    }
}

fn malformed(offset: u64, kind: MalformedKind) -> Error {
    Error::Malformed { offset, kind }
}

/// `%PDF-M.m` at offset 0, followed by white space. Versions 1.0 to 1.7 and 2.0.
fn parse_header(data: &[u8]) -> Result<Version, Error> {
    let header = |len| data.get(..len).ok_or(Error::Truncated { offset: 0 });
    let invalid = malformed(0, MalformedKind::InvalidHeader);
    if !b"%PDF-".starts_with(header(data.len().min(5))?) {
        return Err(invalid);
    }
    let (major, minor) = match header(9)? {
        [b'%', b'P', b'D', b'F', b'-', major, b'.', minor, end]
            if major.is_ascii_digit() && minor.is_ascii_digit() && is_whitespace(*end) =>
        {
            (major - b'0', minor - b'0')
        }
        _ => return Err(invalid),
    };
    let version = Version { major, minor };
    if (major == 1 && minor <= 7) || (major == 2 && minor == 0) {
        Ok(version)
    } else {
        Err(Error::Unsupported(Capability::PdfVersion))
    }
}

/// Finds the last `startxref` near the end and returns the offset it gives,
/// which must be followed by `%%EOF` and nothing but white space.
fn find_startxref(data: &[u8], budget: &mut Budget) -> Result<u64, Error> {
    let len = data.len() as u64;
    let window_start = len.saturating_sub(STARTXREF_WINDOW);
    let window = data.get(window_start as usize..).unwrap_or_default();
    let keyword = b"startxref";
    let found = window
        .windows(keyword.len())
        .rposition(|w| w == keyword)
        .ok_or(malformed(window_start, MalformedKind::MissingStartxref))?;
    let keyword_offset = window_start + found as u64;
    let mut lexer = Lexer::new(data);
    lexer.seek(keyword_offset)?;
    // The keyword must stand alone (not `xstartxref`), then the offset follows.
    if !matches!(
        lexer.next_token(budget)?,
        Some(Spanned {
            token: Token::Keyword(b"startxref"),
            ..
        })
    ) {
        return Err(malformed(keyword_offset, MalformedKind::MissingStartxref));
    }
    let offset = match lexer.next_token(budget)? {
        Some(Spanned {
            token: Token::Integer(offset),
            offset: at,
        }) => u64::try_from(offset)
            .ok()
            .filter(|o| *o < len)
            .ok_or(malformed(at, MalformedKind::MissingStartxref))?,
        Some(Spanned { offset: at, .. }) => {
            return Err(malformed(at, MalformedKind::MissingStartxref));
        }
        None => {
            return Err(Error::Truncated {
                offset: keyword_offset,
            });
        }
    };
    let tail_start = lexer.position();
    let tail = data.get(tail_start as usize..).unwrap_or_default();
    let eof_at = tail.iter().position(|b| !is_whitespace(*b));
    let rest = eof_at.and_then(|i| tail.get(i..)).unwrap_or_default();
    match rest.strip_prefix(b"%%EOF") {
        Some(after) if after.iter().all(|b| is_whitespace(*b)) => Ok(offset),
        _ if b"%%EOF".starts_with(rest) => Err(Error::Truncated {
            offset: keyword_offset,
        }),
        _ => Err(malformed(
            tail_start + eof_at.unwrap_or(0) as u64,
            MalformedKind::MissingEof,
        )),
    }
}

type Sections = (BTreeMap<u32, XrefEntry>, Dictionary, usize);

/// Reads the section at `start` and those reached through `/Prev`, newest first.
fn read_sections(data: &[u8], start: u64, budget: &mut Budget) -> Result<Sections, Error> {
    let len = data.len() as u64;
    let mut entries: BTreeMap<u32, XrefEntry> = BTreeMap::new();
    let mut visited = BTreeSet::new();
    let mut newest: Option<Dictionary> = None;
    let mut next = Some(start);
    while let Some(offset) = next {
        budget.check_cancelled()?;
        if !visited.insert(offset) {
            return Err(malformed(offset, MalformedKind::XrefPrevCycle));
        }
        budget.charge_object()?;
        let (section, trailer) = read_section(data, offset, budget)?;
        if trailer.get(b"XRefStm").is_some() {
            return Err(Error::Unsupported(Capability::XrefStreams));
        }
        for (number, entry) in section {
            entries.entry(number).or_insert(entry);
        }
        next = match trailer.get(b"Prev") {
            None => None,
            Some(Object::Integer(prev)) => Some(
                u64::try_from(*prev)
                    .ok()
                    .filter(|p| *p < len)
                    .ok_or(malformed(offset, MalformedKind::InvalidTrailer))?,
            ),
            Some(_) => return Err(malformed(offset, MalformedKind::InvalidTrailer)),
        };
        if newest.is_none() {
            newest = Some(trailer);
        }
    }
    let trailer = newest.ok_or(malformed(start, MalformedKind::InvalidTrailer))?;
    check_trailer(&trailer, &entries, start, len)?;
    Ok((entries, trailer, visited.len()))
}

fn check_trailer(
    trailer: &Dictionary,
    entries: &BTreeMap<u32, XrefEntry>,
    start: u64,
    len: u64,
) -> Result<(), Error> {
    let invalid = || malformed(start, MalformedKind::InvalidTrailer);
    if trailer.get(b"Encrypt").is_some() {
        return Err(Error::Unsupported(Capability::Encryption));
    }
    let size = trailer
        .get(b"Size")
        .and_then(Object::as_integer)
        .filter(|s| *s > 0)
        .ok_or_else(invalid)?;
    trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .ok_or_else(invalid)?;
    for (number, entry) in entries {
        let in_range = i64::from(*number) < size;
        let valid = match entry {
            XrefEntry::InUse { offset, .. } => *number != 0 && *offset < len,
            XrefEntry::Free { .. } => true,
        };
        if !in_range || !valid {
            return Err(malformed(start, MalformedKind::InvalidXrefEntry));
        }
    }
    Ok(())
}

type Section = (BTreeMap<u32, XrefEntry>, Dictionary);

/// One `xref` table and its `trailer` dictionary at `offset`.
fn read_section(data: &[u8], offset: u64, budget: &mut Budget) -> Result<Section, Error> {
    let mut lexer = Lexer::new(data);
    lexer.seek(offset)?;
    match lexer.next_token(budget)? {
        Some(Spanned {
            token: Token::Keyword(b"xref"),
            ..
        }) => {}
        Some(Spanned { offset: at, token }) => {
            return Err(if looks_like_object(data, at, token, budget) {
                Error::Unsupported(Capability::XrefStreams)
            } else {
                malformed(at, MalformedKind::MissingXref)
            });
        }
        None => return Err(Error::Truncated { offset }),
    }
    let mut section = BTreeMap::new();
    loop {
        let (at, first) = match lexer.next_token(budget)? {
            Some(Spanned {
                token: Token::Keyword(b"trailer"),
                ..
            }) => break,
            Some(Spanned {
                token: Token::Integer(first),
                offset: at,
            }) => (at, first),
            Some(Spanned { offset: at, .. }) => {
                return Err(malformed(at, MalformedKind::InvalidXrefSubsection));
            }
            None => return Err(Error::Truncated { offset }),
        };
        let count = match lexer.next_token(budget)? {
            Some(Spanned {
                token: Token::Integer(count),
                ..
            }) => count,
            Some(Spanned { offset: at, .. }) => {
                return Err(malformed(at, MalformedKind::InvalidXrefSubsection));
            }
            None => return Err(Error::Truncated { offset }),
        };
        let bad_subsection = malformed(at, MalformedKind::InvalidXrefSubsection);
        let first = u64::try_from(first).map_err(|_| bad_subsection.clone())?;
        let count = u64::try_from(count).map_err(|_| bad_subsection.clone())?;
        // Object numbers are u32: first + count - 1 <= u32::MAX.
        if first.saturating_add(count) > u64::from(u32::MAX) + 1 {
            return Err(bad_subsection);
        }
        let entries_start = subsection_line_end(data, lexer.position())
            .ok_or(malformed(at, MalformedKind::InvalidXrefSubsection))?;
        let entries_end = count
            .checked_mul(ENTRY_LEN as u64)
            .and_then(|n| n.checked_add(entries_start))
            .filter(|end| *end <= data.len() as u64)
            .ok_or(Error::Truncated { offset: at })?;
        for i in 0..count {
            if i % CANCEL_CHECK_ENTRIES == 0 {
                budget.check_cancelled()?;
            }
            budget.charge_object()?;
            let entry_offset = entries_start + i * ENTRY_LEN as u64;
            let record = data
                .get(entry_offset as usize..entry_offset as usize + ENTRY_LEN)
                .ok_or(Error::Truncated { offset: at })?;
            let entry = parse_entry(record)
                .ok_or(malformed(entry_offset, MalformedKind::InvalidXrefEntry))?;
            // first + i <= u32::MAX was checked above.
            let number = u32::try_from(first + i)
                .map_err(|_| malformed(at, MalformedKind::InvalidXrefSubsection))?;
            if section.insert(number, entry).is_some() {
                return Err(malformed(entry_offset, MalformedKind::InvalidXrefEntry));
            }
        }
        lexer.seek(entries_end)?;
    }
    let trailer_offset = lexer.position();
    let mut parser = Parser::new(data);
    parser.seek(trailer_offset)?;
    match parser.parse_object(budget)? {
        Object::Dictionary(trailer) => Ok((section, trailer)),
        _ => Err(malformed(trailer_offset, MalformedKind::InvalidTrailer)),
    }
}

/// True when the tokens at `offset` read `n g obj`: a cross-reference stream.
fn looks_like_object(data: &[u8], offset: u64, first: Token<'_>, budget: &mut Budget) -> bool {
    if !matches!(first, Token::Integer(_)) {
        return false;
    }
    let mut lexer = Lexer::new(data);
    if lexer.seek(offset).is_err() {
        return false;
    }
    let mut tokens = std::iter::from_fn(|| lexer.next_token(budget).ok().flatten());
    matches!(
        (
            tokens.next().map(|t| t.token),
            tokens.next().map(|t| t.token),
            tokens.next().map(|t| t.token),
        ),
        (
            Some(Token::Integer(_)),
            Some(Token::Integer(_)),
            Some(Token::Keyword(b"obj"))
        )
    )
}

/// After `first count`: optional spaces, then one end-of-line. Returns the
/// offset of the first entry.
fn subsection_line_end(data: &[u8], from: u64) -> Option<u64> {
    let mut pos = usize::try_from(from).ok()?;
    while data.get(pos) == Some(&b' ') {
        pos += 1;
    }
    let eol = match (data.get(pos), data.get(pos + 1)) {
        (Some(b'\r'), Some(b'\n')) => 2,
        (Some(b'\r' | b'\n'), _) => 1,
        _ => return None,
    };
    Some((pos + eol) as u64)
}

/// `oooooooooo ggggg n` or `f`, then a two-byte end of line (ISO 32000-2, 7.5.4).
fn parse_entry(record: &[u8]) -> Option<XrefEntry> {
    let [fields @ .., e1, e2] = record else {
        return None;
    };
    if !matches!((e1, e2), (b' ', b'\r' | b'\n') | (b'\r', b'\n')) {
        return None;
    }
    let [first @ .., b' ', kind] = fields else {
        return None;
    };
    let (first, rest) = first.split_at_checked(10)?;
    let generation = rest.strip_prefix(b" ")?;
    let first = decimal(first, 10)?;
    let generation = u16::try_from(decimal(generation, 5)?).ok()?;
    match kind {
        b'n' => Some(XrefEntry::InUse {
            offset: first,
            generation,
        }),
        b'f' => Some(XrefEntry::Free {
            next: u32::try_from(first).ok()?,
            generation,
        }),
        _ => None,
    }
}

fn decimal(digits: &[u8], len: usize) -> Option<u64> {
    if digits.len() != len {
        return None;
    }
    digits.iter().try_fold(0_u64, |acc, d| {
        d.is_ascii_digit().then(|| acc * 10 + u64::from(d - b'0'))
    })
}

#[cfg(test)]
mod tests;
