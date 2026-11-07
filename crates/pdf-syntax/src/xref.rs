//! File structure: header, classic cross-reference tables, trailers and
//! incremental updates (ISO 32000-2:2020, 7.5).
//!
//! Strict (ADR-0002): the header must start the file; every offset given by
//! `startxref`, `/Prev` or an entry must point exactly at its keyword or object
//! (no white space or comment skipped); the table layout is read on raw bytes
//! (`xref` EOL, `first count` EOL, 20-byte entries, `trailer`); `/Prev` chains
//! must not loop. No reconstruction is attempted. Cross-reference streams,
//! hybrid files (`/XRefStm`) and encryption are refused as unsupported.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use crate::lexer::{Lexer, Token, is_regular, is_whitespace};
use crate::limits::{Budget, Capability, Error, MalformedKind};
use crate::object::{Dictionary, IndirectObject, IndirectValue, Object, ObjectId, Stream};
use crate::parser::{LengthResolver, Parser};
use crate::source::Source;

/// `startxref` is searched in this many bytes at the end of the file.
const STARTXREF_WINDOW: u64 = 1024;
/// Length of one cross-reference entry, end-of-line included.
const ENTRY_LEN: u64 = 20;
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

/// One cross-reference section of the `/Prev` chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct XrefSection {
    /// Offset of its `xref` keyword.
    pub offset: u64,
    /// Number of entries it declares.
    pub entries: usize,
}

/// A PDF file opened through its cross-reference tables: entries merged from
/// all revisions (newest wins) and the newest trailer.
///
/// In P1 the reader asks the source for whole ranges, which `MemorySource`
/// serves without copying; it is meant for in-memory sources (ADR-0002).
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
pub struct PdfFile<S> {
    source: S,
    version: Version,
    startxref: u64,
    sections: Vec<XrefSection>,
    entries: BTreeMap<u32, XrefEntry>,
    trailer: Dictionary,
}

/// Never prints the document bytes: they may be private and very large.
impl<S: Source> fmt::Debug for PdfFile<S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PdfFile")
            .field("len", &self.source.len())
            .field("version", &self.version)
            .field("startxref", &self.startxref)
            .field("sections", &self.sections)
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}

impl<S: Source> PdfFile<S> {
    /// Reads the header, the `startxref` pointer and every cross-reference
    /// section of the `/Prev` chain. Objects are parsed later, on demand.
    ///
    /// Charges `budget` one object per section, subsection, entry and trailer
    /// node: give opening its own budget, separate from later tasks. A file with
    /// more entries than `max_objects_visited` is refused.
    pub fn open(source: S, budget: &mut Budget) -> Result<Self, Error> {
        let data = source.read(0..source.len())?;
        let version = parse_header(&data)?;
        let startxref = find_startxref(&data)?;
        let (entries, trailer, sections) = read_sections(&data, startxref, budget)?;
        drop(data);
        Ok(Self {
            source,
            version,
            startxref,
            sections,
            entries,
            trailer,
        })
    }

    pub fn version(&self) -> Version {
        self.version
    }

    /// Offset given by the last `startxref`: the `/Prev` of a future update.
    pub fn startxref(&self) -> u64 {
        self.startxref
    }

    /// Sections read, newest first; one per revision.
    pub fn sections(&self) -> &[XrefSection] {
        &self.sections
    }

    /// Number of revisions (1 + incremental updates).
    pub fn revisions(&self) -> usize {
        self.sections.len()
    }

    /// Trailer of the newest revision.
    pub fn trailer(&self) -> &Dictionary {
        &self.trailer
    }

    /// `/Root` of the newest trailer, checked as a reference when opening.
    pub fn root(&self) -> Option<ObjectId> {
        self.trailer.get(b"Root").and_then(Object::as_reference)
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
    /// not match is a reference to `null` (ISO 32000-2, 7.3.10). The entry
    /// offset must point exactly at `id`'s `n g obj`. An indirect `/Length` must
    /// lead to a direct integer; it is never resolved through another indirection.
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

/// Parses `n g obj` starting exactly at `offset`.
fn parse_at(
    data: &[u8],
    offset: u64,
    budget: &mut Budget,
    resolve_length: &mut LengthResolver<'_>,
) -> Result<IndirectObject, Error> {
    if !rest(data, offset).first().is_some_and(u8::is_ascii_digit) {
        return Err(malformed(offset, MalformedKind::ObjectIdMismatch));
    }
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

/// Bytes from `offset` to the end; empty past the end.
fn rest(data: &[u8], offset: u64) -> &[u8] {
    usize::try_from(offset)
        .ok()
        .and_then(|o| data.get(o..))
        .unwrap_or_default()
}

/// True when `keyword` starts exactly at `offset` as a whole token: not
/// preceded or followed by a regular character.
fn keyword_at(data: &[u8], offset: u64, keyword: &[u8]) -> bool {
    let before = offset
        .checked_sub(1)
        .and_then(|o| rest(data, o).first().copied());
    let after = rest(data, offset);
    after.starts_with(keyword)
        && !before.is_some_and(is_regular)
        && !after.get(keyword.len()).copied().is_some_and(is_regular)
}

/// Unsigned decimal digits at `offset`: value and number of digits.
fn digits_at(data: &[u8], offset: u64) -> Option<(u64, u64)> {
    let digits: &[u8] = {
        let bytes = rest(data, offset);
        let len = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
        bytes.get(..len)?
    };
    if digits.is_empty() {
        return None;
    }
    let value = digits.iter().try_fold(0_u64, |acc, d| {
        acc.checked_mul(10)?.checked_add(u64::from(d - b'0'))
    })?;
    Some((value, digits.len() as u64))
}

/// Optional spaces, then one end-of-line; returns the offset after it.
fn line_end(data: &[u8], offset: u64) -> Option<u64> {
    let bytes = rest(data, offset);
    let spaces = bytes.iter().take_while(|b| **b == b' ').count();
    let eol = match bytes.get(spaces..)? {
        [b'\r', b'\n', ..] => 2,
        [b'\r' | b'\n', ..] => 1,
        _ => return None,
    };
    Some(offset + (spaces + eol) as u64)
}

/// `%PDF-M.m` at offset 0, followed by white space. Versions 1.0 to 1.7 and 2.0.
fn parse_header(data: &[u8]) -> Result<Version, Error> {
    let start = data.get(..data.len().min(5)).unwrap_or_default();
    if !b"%PDF-".starts_with(start) {
        return Err(malformed(0, MalformedKind::InvalidHeader));
    }
    let (major, minor) = match data.get(..9).ok_or(Error::Truncated { offset: 0 })? {
        [b'%', b'P', b'D', b'F', b'-', major, b'.', minor, end]
            if major.is_ascii_digit() && minor.is_ascii_digit() && is_whitespace(*end) =>
        {
            (major - b'0', minor - b'0')
        }
        _ => return Err(malformed(0, MalformedKind::InvalidHeader)),
    };
    if (major == 1 && minor <= 7) || (major == 2 && minor == 0) {
        Ok(Version { major, minor })
    } else {
        Err(Error::Unsupported(Capability::PdfVersion))
    }
}

/// Finds the last standalone `startxref` in the final 1024 bytes: white space,
/// unsigned digits, white space, `%%EOF`, then nothing but white space.
fn find_startxref(data: &[u8]) -> Result<u64, Error> {
    let len = data.len() as u64;
    let window_start = len.saturating_sub(STARTXREF_WINDOW);
    let keyword: &[u8] = b"startxref";
    let found = rest(data, window_start)
        .windows(keyword.len())
        .enumerate()
        .rev()
        .map(|(i, _)| window_start + i as u64)
        .find(|at| keyword_at(data, *at, keyword))
        .ok_or(malformed(window_start, MalformedKind::MissingStartxref))?;
    let after_keyword = found + keyword.len() as u64;
    let skip_ws = |from: u64| {
        from + rest(data, from)
            .iter()
            .take_while(|b| is_whitespace(**b))
            .count() as u64
    };
    let digits = skip_ws(after_keyword);
    let Some((offset, digit_count)) = digits_at(data, digits).filter(|_| digits > after_keyword)
    else {
        return Err(if rest(data, digits).is_empty() {
            Error::Truncated { offset: found }
        } else {
            malformed(digits, MalformedKind::MissingStartxref)
        });
    };
    if offset >= len {
        return Err(malformed(digits, MalformedKind::MissingStartxref));
    }
    let eof = skip_ws(digits + digit_count);
    let tail = rest(data, eof);
    match tail.strip_prefix(b"%%EOF") {
        Some(after) if eof > digits + digit_count && after.iter().all(|b| is_whitespace(*b)) => {
            Ok(offset)
        }
        _ if b"%%EOF".starts_with(tail) => Err(Error::Truncated { offset: found }),
        _ => Err(malformed(eof, MalformedKind::MissingEof)),
    }
}

type Merged = BTreeMap<u32, (XrefEntry, u64)>;
type Sections = (BTreeMap<u32, XrefEntry>, Dictionary, Vec<XrefSection>);

/// Reads the section at `start` and those reached through `/Prev`, newest first.
fn read_sections(data: &[u8], start: u64, budget: &mut Budget) -> Result<Sections, Error> {
    let len = data.len() as u64;
    let mut merged: Merged = BTreeMap::new();
    let mut visited = BTreeSet::new();
    let mut sections = Vec::new();
    let mut newest: Option<Dictionary> = None;
    let mut next = Some(start);
    while let Some(offset) = next {
        budget.check_cancelled()?;
        if !visited.insert(offset) {
            return Err(malformed(offset, MalformedKind::XrefPrevCycle));
        }
        budget.charge_object()?;
        let section = read_section(data, offset, budget)?;
        // Checked on every trailer: an older encrypted revision would otherwise
        // be read as plain text.
        if section.trailer.get(b"Encrypt").is_some() {
            return Err(Error::Unsupported(Capability::Encryption));
        }
        if section.trailer.get(b"XRefStm").is_some() {
            return Err(Error::Unsupported(Capability::XrefStreams));
        }
        sections.push(XrefSection {
            offset,
            entries: section.entries.len(),
        });
        for (number, entry) in section.entries {
            merged.entry(number).or_insert(entry);
        }
        let invalid_prev = malformed(section.trailer_offset, MalformedKind::InvalidTrailer);
        next = match section.trailer.get(b"Prev") {
            None => None,
            Some(Object::Integer(prev)) => Some(
                u64::try_from(*prev)
                    .ok()
                    .filter(|p| *p < len)
                    .ok_or(invalid_prev)?,
            ),
            Some(_) => return Err(invalid_prev),
        };
        if newest.is_none() {
            newest = Some(section.trailer);
        }
    }
    let trailer = newest.ok_or(malformed(start, MalformedKind::InvalidTrailer))?;
    check_trailer(&trailer, &merged, start)?;
    let entries = merged.into_iter().map(|(n, (e, _))| (n, e)).collect();
    Ok((entries, trailer, sections))
}

/// `/Size` and `/Root` of the newest trailer; every entry number below `/Size`.
fn check_trailer(trailer: &Dictionary, entries: &Merged, start: u64) -> Result<(), Error> {
    let invalid = || malformed(start, MalformedKind::InvalidTrailer);
    let size = trailer
        .get(b"Size")
        .and_then(Object::as_integer)
        .filter(|s| *s > 0)
        .ok_or_else(invalid)?;
    trailer
        .get(b"Root")
        .and_then(Object::as_reference)
        .ok_or_else(invalid)?;
    match entries
        .iter()
        .find(|(number, _)| i64::from(**number) >= size)
    {
        Some((_, (_, at))) => Err(malformed(*at, MalformedKind::InvalidXrefEntry)),
        None => Ok(()),
    }
}

struct SectionRead {
    /// Entries with the offset of their 20-byte record.
    entries: Merged,
    trailer: Dictionary,
    trailer_offset: u64,
}

/// One `xref` table and its `trailer` dictionary, `xref` exactly at `offset`.
fn read_section(data: &[u8], offset: u64, budget: &mut Budget) -> Result<SectionRead, Error> {
    let len = data.len() as u64;
    if !keyword_at(data, offset, b"xref") {
        return Err(if is_xref_stream(data, offset, budget) {
            Error::Unsupported(Capability::XrefStreams)
        } else {
            malformed(offset, MalformedKind::MissingXref)
        });
    }
    let bad = |at| malformed(at, MalformedKind::InvalidXrefSubsection);
    let truncated = Error::Truncated { offset };
    let mut pos = line_end(data, offset + 4).ok_or(bad(offset + 4))?;
    let mut entries: Merged = BTreeMap::new();
    loop {
        budget.check_cancelled()?;
        if keyword_at(data, pos, b"trailer") {
            break;
        }
        if b"trailer".starts_with(rest(data, pos)) {
            return Err(truncated);
        }
        // `first count` separated by one space, then an end-of-line.
        let (first, first_len) = digits_at(data, pos).ok_or(bad(pos))?;
        let space = pos + first_len;
        if rest(data, space).first() != Some(&b' ') {
            return Err(bad(space));
        }
        let (count, count_len) = digits_at(data, space + 1).ok_or(bad(space + 1))?;
        budget.charge_object()?;
        // Object numbers are u32: first + count - 1 <= u32::MAX.
        if first.saturating_add(count) > u64::from(u32::MAX) + 1 {
            return Err(bad(pos));
        }
        let entries_start = line_end(data, space + 1 + count_len).ok_or(bad(space + 1))?;
        let entries_end = count
            .checked_mul(ENTRY_LEN)
            .and_then(|n| n.checked_add(entries_start))
            .filter(|end| *end <= len)
            .ok_or(Error::Truncated { offset: pos })?;
        for i in 0..count {
            if i % CANCEL_CHECK_ENTRIES == 0 {
                budget.check_cancelled()?;
            }
            budget.charge_object()?;
            let at = entries_start + i * ENTRY_LEN;
            let invalid = malformed(at, MalformedKind::InvalidXrefEntry);
            let record = rest(data, at)
                .get(..ENTRY_LEN as usize)
                .ok_or(Error::Truncated { offset: pos })?;
            let entry = parse_entry(record).ok_or(invalid.clone())?;
            // first + i <= u32::MAX was checked above.
            let number = u32::try_from(first + i).map_err(|_| bad(pos))?;
            let valid = match entry {
                XrefEntry::InUse { offset, .. } => number != 0 && offset < len,
                XrefEntry::Free { .. } => true,
            };
            if !valid || entries.insert(number, (entry, at)).is_some() {
                return Err(invalid);
            }
        }
        pos = entries_end;
    }
    let mut parser = Parser::new(data);
    parser.seek(pos + b"trailer".len() as u64)?;
    match parser.parse_object(budget)? {
        Object::Dictionary(trailer) => Ok(SectionRead {
            entries,
            trailer,
            trailer_offset: pos,
        }),
        _ => Err(malformed(pos, MalformedKind::InvalidTrailer)),
    }
}

/// True when `offset` starts `n g obj << ... /Type /XRef ... >>`.
fn is_xref_stream(data: &[u8], offset: u64, budget: &mut Budget) -> bool {
    if !rest(data, offset).first().is_some_and(u8::is_ascii_digit) {
        return false;
    }
    let mut lexer = Lexer::new(data);
    if lexer.seek(offset).is_err() {
        return false;
    }
    let mut next = || lexer.next_token(budget).ok().flatten().map(|t| t.token);
    let header = (next(), next(), next());
    if !matches!(
        header,
        (
            Some(Token::Integer(_)),
            Some(Token::Integer(_)),
            Some(Token::Keyword(b"obj"))
        )
    ) {
        return false;
    }
    let mut parser = Parser::new(data);
    parser.seek(lexer.position()).is_ok()
        && matches!(
            parser.parse_object(budget),
            Ok(Object::Dictionary(dict)) if dict.get(b"Type").and_then(Object::as_name) == Some(b"XRef")
        )
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
