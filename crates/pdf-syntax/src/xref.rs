//! File structure: header, cross-reference tables and streams, trailers and
//! incremental updates (ISO 32000-2:2020, 7.5), and objects compressed in
//! object streams (7.5.7).
//!
//! Strict (ADR-0002): the header must start the file; every offset given by
//! `startxref`, `/Prev`, `/XRefStm` or an entry must point exactly at its
//! keyword or object (no white space or comment skipped); classic tables are
//! read on raw bytes (`xref` EOL, `first count` EOL, 20-byte entries,
//! `trailer`); `/Prev` chains must not loop. No reconstruction is attempted.
//! Encryption is refused as unsupported.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, Mutex};

use crate::lexer::{Lexer, Token, is_regular, is_whitespace};
use crate::limits::{Budget, Capability, Error, MalformedKind};
use crate::object::{Dictionary, IndirectObject, IndirectValue, Object, ObjectId, Stream};
use crate::objstm::{ObjectStream, ObjectStreamCache};
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
#[non_exhaustive]
pub enum XrefEntry {
    /// `f` (type 0): object number of the next free object, and generation
    /// for reuse.
    Free { next: u32, generation: u16 },
    /// `n` (type 1): byte offset of `number generation obj` in the file.
    InUse { offset: u64, generation: u16 },
    /// Type 2: object `index` of the object stream `stream`; generation 0.
    Compressed { stream: u32, index: u32 },
}

/// How a cross-reference section is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SectionKind {
    /// Classic `xref` table and `trailer`.
    Table,
    /// Cross-reference stream (`/Type /XRef`, PDF 1.5).
    Stream,
    /// Stream named by `/XRefStm` in a classic trailer (hybrid file); part of
    /// the revision of that table, not a revision of its own.
    HybridStream,
}

/// One cross-reference section of the `/Prev` chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct XrefSection {
    /// Offset of its `xref` keyword or of its stream object.
    pub offset: u64,
    /// Number of entries it declares.
    pub entries: usize,
    pub kind: SectionKind,
}

/// A PDF file opened through its cross-reference sections: entries merged from
/// all revisions (newest wins) and the newest trailer.
///
/// In P1 the reader asks the source for whole ranges, which `MemorySource`
/// serves without copying; it is meant for in-memory sources (ADR-0002).
/// Decoded object streams are kept in a small bounded cache.
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
    object_streams: Mutex<ObjectStreamCache>,
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
    /// node, and the decoded bytes of cross-reference streams: give opening its
    /// own budget, separate from later tasks. A file with more entries than
    /// `max_objects_visited` is refused.
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
            object_streams: Mutex::new(ObjectStreamCache::default()),
        })
    }

    pub fn version(&self) -> Version {
        self.version
    }

    /// Offset given by the last `startxref`: the `/Prev` of a future update.
    pub fn startxref(&self) -> u64 {
        self.startxref
    }

    /// Sections read, newest first; a hybrid file lists its `/XRefStm` stream
    /// right after the table of the same revision.
    pub fn sections(&self) -> &[XrefSection] {
        &self.sections
    }

    /// Number of revisions (1 + incremental updates).
    pub fn revisions(&self) -> usize {
        self.sections
            .iter()
            .filter(|s| s.kind != SectionKind::HybridStream)
            .count()
    }

    /// Trailer of the newest revision: the `trailer` dictionary, or the
    /// dictionary of the newest cross-reference stream.
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
    /// offset must point exactly at `id`'s `n g obj`. A compressed object is
    /// read from its decoded object stream (generation 0, never a stream). An
    /// indirect `/Length` must lead to a direct integer, possibly compressed;
    /// it is never resolved through another indirection.
    pub fn resolve(&self, id: ObjectId, budget: &mut Budget) -> Result<IndirectValue, Error> {
        match self.entries.get(&id.number) {
            Some(XrefEntry::InUse { offset, generation }) if *generation == id.generation => {
                let offset = *offset;
                let data = self.source.read(0..self.source.len())?;
                let mut length = |length_id: ObjectId, budget: &mut Budget| {
                    self.direct_length(&data, offset, length_id, true, budget)
                };
                let object = parse_at(&data, offset, budget, &mut length)?;
                check_id(&object, id, offset)?;
                Ok(object.value)
            }
            Some(XrefEntry::Compressed { stream, index }) if id.generation == 0 => self
                .compressed_object(id, *stream, *index, budget)
                .map(IndirectValue::Object),
            _ => Ok(IndirectValue::Object(Object::Null)),
        }
    }

    /// Encoded bytes of a stream, as located by `resolve`; filters are not applied.
    pub fn stream_data(&self, stream: &Stream) -> Result<Cow<'_, [u8]>, Error> {
        self.source.read(stream.data.clone())
    }

    /// Decoded bytes of a stream: `/Filter` and `/DecodeParms` are resolved
    /// once if they are references (a stream or a second reference is
    /// refused), then `decode_stream` applies the filters under `budget`.
    pub fn decoded_stream_data(
        &self,
        stream: &Stream,
        budget: &mut Budget,
    ) -> Result<Vec<u8>, Error> {
        self.decode_with(stream, true, budget)
    }

    /// `decoded_stream_data`, where `allow_compressed` says whether `/Filter`
    /// and `/DecodeParms` may name compressed objects. Decoding an object
    /// stream refuses them: otherwise an object stream could need itself (or
    /// another object stream needing it) to decode, without end.
    fn decode_with(
        &self,
        stream: &Stream,
        allow_compressed: bool,
        budget: &mut Budget,
    ) -> Result<Vec<u8>, Error> {
        let offset = stream.data.start;
        let invalid = || malformed(offset, MalformedKind::InvalidFilter);
        let mut filtering = Dictionary::new();
        for key in [&b"Filter"[..], b"DecodeParms"] {
            let value = match stream.dictionary.get(key) {
                None => continue,
                Some(Object::Reference(id)) => {
                    let compressed = matches!(
                        self.entries.get(&id.number),
                        Some(XrefEntry::Compressed { .. })
                    );
                    if compressed && !allow_compressed {
                        return Err(invalid());
                    }
                    match self.resolve(*id, budget)? {
                        IndirectValue::Object(Object::Reference(_)) | IndirectValue::Stream(_) => {
                            return Err(invalid());
                        }
                        IndirectValue::Object(object) => object,
                    }
                }
                Some(object) => object.clone(),
            };
            filtering.insert(key.to_vec(), value);
        }
        let encoded = self.stream_data(stream)?;
        crate::filter::decode_stream(&filtering, &encoded, offset, budget)
    }

    /// Object `index` of object stream `stream_number`, which must list `id`.
    fn compressed_object(
        &self,
        id: ObjectId,
        stream_number: u32,
        index: u32,
        budget: &mut Budget,
    ) -> Result<Object, Error> {
        let object_stream = self.object_stream(stream_number, budget)?;
        object_stream.object(id.number, index, budget)
    }

    /// The decoded object stream `number`, from the cache or decoded now (and
    /// charged to `budget`). Its `/Length` may not be compressed, so decoding
    /// one object stream never requires decoding another.
    fn object_stream(&self, number: u32, budget: &mut Budget) -> Result<Arc<ObjectStream>, Error> {
        if let Some(found) = lock(&self.object_streams).get(number) {
            return Ok(found);
        }
        // check_trailer guarantees an in-use entry for every object stream
        // named by a compressed entry; object streams have generation 0.
        let Some(XrefEntry::InUse {
            offset,
            generation: 0,
        }) = self.entries.get(&number).copied()
        else {
            return Err(malformed(0, MalformedKind::InvalidObjectStream));
        };
        let generation = 0;
        let data = self.source.read(0..self.source.len())?;
        let mut length = |length_id: ObjectId, budget: &mut Budget| {
            self.direct_length(&data, offset, length_id, false, budget)
        };
        let object = parse_at(&data, offset, budget, &mut length)?;
        check_id(&object, ObjectId { number, generation }, offset)?;
        let IndirectValue::Stream(stream) = object.value else {
            return Err(malformed(offset, MalformedKind::InvalidObjectStream));
        };
        if stream.dictionary.get(b"Type").and_then(Object::as_name) != Some(b"ObjStm") {
            return Err(malformed(offset, MalformedKind::InvalidObjectStream));
        }
        drop(data);
        let decoded = self.decode_with(&stream, false, budget)?;
        let object_stream = Arc::new(ObjectStream::parse(
            &stream.dictionary,
            decoded,
            stream.data.start,
            budget,
        )?);
        lock(&self.object_streams).insert(number, Arc::clone(&object_stream));
        Ok(object_stream)
    }

    fn direct_length(
        &self,
        data: &[u8],
        stream_offset: u64,
        id: ObjectId,
        allow_compressed: bool,
        budget: &mut Budget,
    ) -> Result<i64, Error> {
        let invalid = malformed(stream_offset, MalformedKind::InvalidStreamLength);
        let value = match self.entries.get(&id.number).copied() {
            Some(XrefEntry::InUse { offset, generation }) if generation == id.generation => {
                let mut refuse = |_: ObjectId, _: &mut Budget| Err(invalid.clone());
                let object = parse_at(data, offset, budget, &mut refuse)?;
                check_id(&object, id, offset)?;
                object.value
            }
            Some(XrefEntry::Compressed { stream, index })
                if allow_compressed && id.generation == 0 =>
            {
                IndirectValue::Object(self.compressed_object(id, stream, index, budget)?)
            }
            _ => return Err(invalid),
        };
        match value {
            IndirectValue::Object(Object::Integer(length)) => Ok(length),
            _ => Err(invalid),
        }
    }
}

/// The cache lock; a panic while holding it cannot leave the cache in an
/// unusable state, so a poisoned lock is recovered.
fn lock(cache: &Mutex<ObjectStreamCache>) -> std::sync::MutexGuard<'_, ObjectStreamCache> {
    cache
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
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

/// Reads the section at `start` and those reached through `/Prev`, newest
/// first. In a hybrid file, the `/XRefStm` stream of a classic section is read
/// right after its table: in-use table entries win, then stream entries (which
/// also replace the table's free entries of the same revision), then older
/// revisions (ISO 32000-2, 7.5.8.4).
fn read_sections(data: &[u8], start: u64, budget: &mut Budget) -> Result<Sections, Error> {
    let len = data.len() as u64;
    let mut merged: Merged = BTreeMap::new();
    let mut visited = BTreeSet::new();
    let mut sections = Vec::new();
    let mut newest: Option<Dictionary> = None;
    let mut next = Some(start);
    let offset_in = |object: Option<&Object>, at: u64| -> Result<Option<u64>, Error> {
        match object {
            None => Ok(None),
            Some(Object::Integer(value)) => u64::try_from(*value)
                .ok()
                .filter(|o| *o < len)
                .map(Some)
                .ok_or(malformed(at, MalformedKind::InvalidTrailer)),
            Some(_) => Err(malformed(at, MalformedKind::InvalidTrailer)),
        }
    };
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
        sections.push(XrefSection {
            offset,
            entries: section.entries.len(),
            kind: section.kind,
        });
        // Free entries this table adds: a hybrid stream of the same revision
        // may override them (writers list compressed objects as free in the
        // table for readers without cross-reference stream support).
        let mut table_free = BTreeSet::new();
        for (number, entry) in section.entries {
            if let std::collections::btree_map::Entry::Vacant(slot) = merged.entry(number) {
                if matches!(entry.0, XrefEntry::Free { .. }) {
                    table_free.insert(number);
                }
                slot.insert(entry);
            }
        }
        let hybrid = match section.kind {
            SectionKind::Table => {
                offset_in(section.trailer.get(b"XRefStm"), section.trailer_offset)?
            }
            _ => None,
        };
        if let Some(stream_offset) = hybrid {
            if !visited.insert(stream_offset) {
                return Err(malformed(stream_offset, MalformedKind::XrefPrevCycle));
            }
            budget.charge_object()?;
            if !is_xref_stream(data, stream_offset, budget) {
                return Err(malformed(stream_offset, MalformedKind::MissingXref));
            }
            let stream = read_xref_stream(data, stream_offset, budget)?;
            if stream.trailer.get(b"Encrypt").is_some() {
                return Err(Error::Unsupported(Capability::Encryption));
            }
            sections.push(XrefSection {
                offset: stream_offset,
                entries: stream.entries.len(),
                kind: SectionKind::HybridStream,
            });
            for (number, entry) in stream.entries {
                if table_free.contains(&number) {
                    merged.insert(number, entry);
                } else {
                    merged.entry(number).or_insert(entry);
                }
            }
        }
        next = offset_in(section.trailer.get(b"Prev"), section.trailer_offset)?;
        if newest.is_none() {
            newest = Some(section.trailer);
        }
    }
    let trailer = newest.ok_or(malformed(start, MalformedKind::InvalidTrailer))?;
    check_trailer(&trailer, &merged, start)?;
    let entries = merged.into_iter().map(|(n, (e, _))| (n, e)).collect();
    Ok((entries, trailer, sections))
}

/// `/Size` and `/Root` of the newest trailer; every entry number below
/// `/Size`; every compressed entry names an object stream that is in use.
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
    for (number, (entry, at)) in entries {
        let valid = i64::from(*number) < size
            && match entry {
                XrefEntry::Compressed { stream, .. } => {
                    matches!(entries.get(stream), Some((XrefEntry::InUse { .. }, _)))
                }
                _ => true,
            };
        if !valid {
            return Err(malformed(*at, MalformedKind::InvalidXrefEntry));
        }
    }
    Ok(())
}

struct SectionRead {
    /// Entries with the offset of their record (a classic 20-byte record, or
    /// the cross-reference stream object).
    entries: Merged,
    trailer: Dictionary,
    trailer_offset: u64,
    kind: SectionKind,
}

/// The section at `offset`: a classic table (`xref` exactly there) or a
/// cross-reference stream (`n g obj` exactly there, `/Type /XRef`).
fn read_section(data: &[u8], offset: u64, budget: &mut Budget) -> Result<SectionRead, Error> {
    if keyword_at(data, offset, b"xref") {
        read_table(data, offset, budget)
    } else if is_xref_stream(data, offset, budget) {
        read_xref_stream(data, offset, budget)
    } else {
        Err(malformed(offset, MalformedKind::MissingXref))
    }
}

/// A cross-reference stream (ISO 32000-2, 7.5.8): its dictionary is the
/// trailer, its decoded data holds `/W`-sized big-endian fields for the object
/// ranges of `/Index` (default `[0 /Size]`). Every value of the dictionary must
/// be direct, `/Length` included. Entries of an unknown type are references to
/// null and are skipped.
fn read_xref_stream(data: &[u8], offset: u64, budget: &mut Budget) -> Result<SectionRead, Error> {
    let len = data.len() as u64;
    let invalid = || malformed(offset, MalformedKind::InvalidXrefStream);
    let mut refuse = |_: ObjectId, _: &mut Budget| Err(invalid());
    let object = parse_at(data, offset, budget, &mut refuse)?;
    let IndirectValue::Stream(stream) = object.value else {
        return Err(invalid());
    };
    let dictionary = stream.dictionary;
    if dictionary.get(b"Type").and_then(Object::as_name) != Some(b"XRef") {
        return Err(invalid());
    }
    let encoded = usize::try_from(stream.data.start)
        .ok()
        .zip(usize::try_from(stream.data.end).ok())
        .and_then(|(s, e)| data.get(s..e))
        .ok_or_else(invalid)?;
    let decoded = crate::filter::decode_stream(&dictionary, encoded, stream.data.start, budget)?;
    let size = dictionary
        .get(b"Size")
        .and_then(Object::as_integer)
        .and_then(|s| u64::try_from(s).ok())
        .ok_or_else(invalid)?;
    let widths: Vec<usize> = match dictionary.get(b"W").and_then(Object::as_array) {
        Some(items) if items.len() == 3 => items
            .iter()
            .map(|w| {
                w.as_integer()
                    .and_then(|w| usize::try_from(w).ok())
                    .filter(|w| *w <= 8)
                    .ok_or_else(invalid)
            })
            .collect::<Result<_, _>>()?,
        _ => return Err(invalid()),
    };
    let [w_type, w_field2, w_field3] = widths.as_slice() else {
        return Err(invalid());
    };
    let (w_type, w_field2, w_field3) = (*w_type, *w_field2, *w_field3);
    let record_len = w_type + w_field2 + w_field3;
    if w_field2 == 0 || record_len == 0 {
        return Err(invalid());
    }
    let ranges: Vec<(u64, u64)> = match dictionary.get(b"Index") {
        None => vec![(0, size)],
        Some(Object::Array(items)) if items.len() % 2 == 0 => items
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| match pair {
                [Object::Integer(first), Object::Integer(count)] => u64::try_from(*first)
                    .ok()
                    .zip(u64::try_from(*count).ok())
                    .filter(|(f, c)| f.saturating_add(*c) <= u64::from(u32::MAX) + 1)
                    .ok_or_else(invalid),
                _ => Err(invalid()),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(invalid()),
    };
    let total: u64 = ranges
        .iter()
        .try_fold(0_u64, |sum, (_, count)| sum.checked_add(*count))
        .ok_or_else(invalid)?;
    let expected = total.checked_mul(record_len as u64).ok_or_else(invalid)?;
    if decoded.len() as u64 != expected {
        return Err(invalid());
    }
    let mut entries: Merged = BTreeMap::new();
    let mut records = decoded.chunks_exact(record_len);
    for (first, count) in ranges {
        budget.charge_object()?;
        for i in 0..count {
            if i % CANCEL_CHECK_ENTRIES == 0 {
                budget.check_cancelled()?;
            }
            budget.charge_object()?;
            let record = records.next().ok_or_else(invalid)?;
            let (type_bytes, fields) = record.split_at(w_type);
            let (field2, field3) = fields.split_at(w_field2);
            // A missing type field means type 1 (ISO 32000-2, table 17).
            let kind = if w_type == 0 {
                1
            } else {
                big_endian(type_bytes)
            };
            let (field2, field3) = (big_endian(field2), big_endian(field3));
            let number = u32::try_from(first + i).map_err(|_| invalid())?;
            let bad_entry = || malformed(offset, MalformedKind::InvalidXrefEntry);
            let entry = match kind {
                0 => XrefEntry::Free {
                    next: u32::try_from(field2).map_err(|_| bad_entry())?,
                    generation: u16::try_from(field3).map_err(|_| bad_entry())?,
                },
                1 => XrefEntry::InUse {
                    offset: field2,
                    generation: u16::try_from(field3).map_err(|_| bad_entry())?,
                },
                2 => XrefEntry::Compressed {
                    stream: u32::try_from(field2)
                        .ok()
                        .filter(|s| *s > 0)
                        .ok_or_else(bad_entry)?,
                    index: u32::try_from(field3).map_err(|_| bad_entry())?,
                },
                // Other types are references to null (7.5.8.3): a free entry,
                // which also hides any older entry for the number.
                _ => XrefEntry::Free {
                    next: 0,
                    generation: 0,
                },
            };
            let valid = match entry {
                XrefEntry::InUse { offset, .. } => number != 0 && offset < len,
                XrefEntry::Compressed { stream, .. } => number != 0 && stream != number,
                _ => true,
            };
            if !valid || entries.insert(number, (entry, offset)).is_some() {
                return Err(malformed(offset, MalformedKind::InvalidXrefEntry));
            }
        }
    }
    Ok(SectionRead {
        entries,
        trailer: dictionary,
        trailer_offset: offset,
        kind: SectionKind::Stream,
    })
}

/// Unsigned big-endian value of up to 8 bytes.
fn big_endian(bytes: &[u8]) -> u64 {
    bytes
        .iter()
        .fold(0_u64, |acc, b| (acc << 8) | u64::from(*b))
}

/// One `xref` table and its `trailer` dictionary, `xref` exactly at `offset`.
fn read_table(data: &[u8], offset: u64, budget: &mut Budget) -> Result<SectionRead, Error> {
    let len = data.len() as u64;
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
                _ => true,
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
            kind: SectionKind::Table,
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

#[cfg(test)]
mod stream_tests;
