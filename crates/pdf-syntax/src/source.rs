//! Input bytes of one document, held in memory under `max_input_bytes`.
//!
//! ADR-0002: the whole file is read once, without `mmap`. Growth is bounded by
//! the limit, never by a size announced by the input or its metadata.

use std::borrow::Cow;
use std::io::{self, Read};
use std::ops::Range;

use crate::limits::{Error, LimitKind, ResourceLimits};

const READ_CHUNK: usize = 64 * 1024;

/// Immutable document bytes, at most `ResourceLimits::max_input_bytes` long.
#[derive(Clone, PartialEq, Eq)]
pub struct MemorySource {
    data: Vec<u8>,
}

/// Prints the length only: document bytes may be private and very large.
impl std::fmt::Debug for MemorySource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MemorySource")
            .field("len", &self.data.len())
            .finish_non_exhaustive()
    }
}

impl MemorySource {
    /// Takes ownership of bytes already in memory, refusing them over the limit.
    pub fn from_vec(data: Vec<u8>, limits: &ResourceLimits) -> Result<Self, Error> {
        check_len(data.len(), limits.max_input_bytes)?;
        Ok(Self { data })
    }

    /// Reads a whole input and fails as soon as it exceeds the limit; the buffer
    /// capacity never exceeds `max_input_bytes`.
    ///
    /// `size_hint` (typically the file length from its metadata) only refuses an
    /// announced oversize input early and sizes the first allocation, capped by
    /// the limit; it is never trusted as a bound. With a correct hint the buffer is
    /// allocated once. Without it, growth by doubling can briefly hold the old and
    /// new buffers together during a reallocation (up to about 1.5 × the limit).
    pub fn from_reader<R: Read>(
        mut reader: R,
        size_hint: Option<u64>,
        limits: &ResourceLimits,
    ) -> Result<Self, Error> {
        let max = usize::try_from(limits.max_input_bytes).unwrap_or(usize::MAX);
        let mut data: Vec<u8> = Vec::new();
        if let Some(hint) = size_hint {
            let hint = usize::try_from(hint).unwrap_or(usize::MAX);
            check_len(hint, limits.max_input_bytes)?;
            // One spare byte so that reading the expected end costs no reallocation.
            data.try_reserve_exact(hint.saturating_add(1).min(max))
                .map_err(|_| Error::Io(io::ErrorKind::OutOfMemory))?;
        }
        let mut chunk = vec![0_u8; READ_CHUNK];
        loop {
            let read = match reader.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(Error::Io(e.kind())),
            };
            let new_len = data.len().saturating_add(read);
            check_len(new_len, limits.max_input_bytes)?;
            if data.capacity() < new_len {
                // Amortised doubling, capped by the limit.
                let target = data.capacity().saturating_mul(2).max(new_len).min(max);
                data.try_reserve_exact(target.saturating_sub(data.len()))
                    .map_err(|_| Error::Io(io::ErrorKind::OutOfMemory))?;
            }
            let bytes = chunk
                .get(..read)
                .ok_or(Error::Io(io::ErrorKind::InvalidData))?;
            data.extend_from_slice(bytes);
        }
        Ok(Self { data })
    }

    /// Length in bytes; file offsets are `u64` throughout the engine.
    pub fn len(&self) -> u64 {
        self.data.len() as u64
    }

    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    pub fn bytes(&self) -> &[u8] {
        &self.data
    }
}

/// Random access to the bytes of one document (ADR-0002, docs/ARCHITECTURE.md).
///
/// Offsets are `u64`; conversion to platform sizes happens inside the source.
/// The structure reader only depends on this trait. In P1 it asks for whole
/// ranges, which `MemorySource` serves without copying; a ranged source will
/// need windowed reads in the reader, not a change of this contract.
pub trait Source {
    /// Length of the input in bytes.
    fn len(&self) -> u64;

    /// Returns exactly the bytes of `range`; a range past the end is `Truncated`.
    fn read(&self, range: Range<u64>) -> Result<Cow<'_, [u8]>, Error>;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Source for MemorySource {
    fn len(&self) -> u64 {
        self.data.len() as u64
    }

    fn read(&self, range: Range<u64>) -> Result<Cow<'_, [u8]>, Error> {
        let start = usize::try_from(range.start).ok();
        let end = usize::try_from(range.end).ok();
        start
            .zip(end)
            .and_then(|(start, end)| self.data.get(start..end))
            .map(Cow::Borrowed)
            .ok_or(Error::Truncated {
                offset: range.start,
            })
    }
}

fn check_len(len: usize, max: u64) -> Result<(), Error> {
    if len as u64 > max {
        Err(Error::LimitExceeded {
            kind: LimitKind::InputBytes,
            limit: max,
        })
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limits(max_input_bytes: u64) -> ResourceLimits {
        ResourceLimits {
            max_input_bytes,
            ..ResourceLimits::default()
        }
    }

    const REFUSED: Error = Error::LimitExceeded {
        kind: LimitKind::InputBytes,
        limit: 100_000,
    };

    #[test]
    fn exact_limit_is_accepted_and_limit_plus_one_refused() {
        let l = limits(100_000);
        let exact = vec![7_u8; 100_000];
        let source = MemorySource::from_reader(exact.as_slice(), None, &l).unwrap();
        assert_eq!(source.len(), 100_000);
        assert_eq!(source.bytes(), exact.as_slice());
        assert_eq!(MemorySource::from_vec(exact, &l).unwrap().len(), 100_000);

        let over = vec![7_u8; 100_001];
        assert_eq!(
            MemorySource::from_reader(over.as_slice(), None, &l),
            Err(REFUSED)
        );
        assert_eq!(MemorySource::from_vec(over, &l), Err(REFUSED));
    }

    #[test]
    fn endless_input_stops_at_the_limit() {
        let l = limits(100_000);
        let endless = io::repeat(b'x');
        assert_eq!(MemorySource::from_reader(endless, None, &l), Err(REFUSED));
    }

    #[test]
    fn capacity_never_exceeds_the_limit() {
        let l = limits(100_000);
        let source = MemorySource::from_reader(&[1_u8; 99_999][..], None, &l).unwrap();
        assert!(source.data.capacity() <= 100_000);
    }

    #[test]
    fn interrupted_reads_are_retried_and_errors_reported() {
        struct Flaky(u8);
        impl Read for Flaky {
            fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
                self.0 += 1;
                match self.0 {
                    1 => Err(io::ErrorKind::Interrupted.into()),
                    2 => {
                        buf[0] = b'%';
                        Ok(1)
                    }
                    _ => Err(io::ErrorKind::PermissionDenied.into()),
                }
            }
        }
        assert_eq!(
            MemorySource::from_reader(Flaky(0), None, &limits(10)),
            Err(Error::Io(io::ErrorKind::PermissionDenied))
        );
    }

    #[test]
    fn oversize_hint_is_refused_before_reading() {
        struct Untouched;
        impl Read for Untouched {
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                panic!("an oversize input must be refused before any read");
            }
        }
        let l = limits(100_000);
        assert_eq!(
            MemorySource::from_reader(Untouched, Some(100_001), &l),
            Err(REFUSED)
        );
        assert_eq!(
            MemorySource::from_reader(Untouched, Some(u64::MAX), &l),
            Err(REFUSED)
        );
    }

    #[test]
    fn correct_hint_allocates_once_and_wrong_hints_stay_bounded() {
        let l = limits(100_000);
        let input = vec![3_u8; 50_000];
        let exact = MemorySource::from_reader(input.as_slice(), Some(50_000), &l).unwrap();
        assert_eq!(exact.data.capacity(), 50_001);
        assert_eq!(exact.bytes(), input.as_slice());

        // A hint below the real size neither truncates nor lifts the limit.
        let short = MemorySource::from_reader(input.as_slice(), Some(10), &l).unwrap();
        assert_eq!(short.bytes(), input.as_slice());
        assert!(short.data.capacity() <= 100_000);
        let endless = io::repeat(b'x');
        assert_eq!(
            MemorySource::from_reader(endless, Some(10), &l),
            Err(REFUSED)
        );

        // A hint above the real size only reserves up to the limit.
        let long = MemorySource::from_reader(input.as_slice(), Some(100_000), &l).unwrap();
        assert_eq!(long.len(), 50_000);
        assert_eq!(long.data.capacity(), 100_000);
    }

    #[test]
    fn empty_input_is_an_empty_source() {
        let source = MemorySource::from_reader(io::empty(), None, &limits(0)).unwrap();
        assert!(source.is_empty());
    }
}
