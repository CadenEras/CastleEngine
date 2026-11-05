//! Length-prefixed frames: `u32` little-endian length, then the payload.
//! The length is checked against a maximum before any allocation.

use std::fmt;
use std::io::{self, Read, Write};

/// Default maximum payload size for control messages.
pub const DEFAULT_MAX_FRAME_LEN: u32 = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    TooLarge { len: u64, max: u32 },
    Truncated,
    Io(io::ErrorKind),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge { len, max } => write!(f, "frame of {len} bytes exceeds {max}"),
            Self::Truncated => f.write_str("truncated frame"),
            Self::Io(kind) => write!(f, "I/O error: {kind}"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Reads one frame. Returns `Ok(None)` on a clean end of stream between frames.
pub fn read_frame<R: Read>(reader: &mut R, max_len: u32) -> Result<Option<Vec<u8>>, FrameError> {
    let mut header = [0u8; 4];
    let mut filled = 0usize;
    while let Some(rest) = header.get_mut(filled..) {
        if rest.is_empty() {
            break;
        }
        match reader.read(rest) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => return Err(FrameError::Truncated),
            Ok(n) => filled = filled.saturating_add(n),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(FrameError::Io(e.kind())),
        }
    }

    let len = u32::from_le_bytes(header);
    if len > max_len {
        return Err(FrameError::TooLarge {
            len: u64::from(len),
            max: max_len,
        });
    }
    let size = usize::try_from(len).map_err(|_| FrameError::TooLarge {
        len: u64::from(len),
        max: max_len,
    })?;
    let mut payload = vec![0u8; size];
    reader
        .read_exact(&mut payload)
        .map_err(|e| match e.kind() {
            io::ErrorKind::UnexpectedEof => FrameError::Truncated,
            kind => FrameError::Io(kind),
        })?;
    Ok(Some(payload))
}

pub fn write_frame<W: Write>(
    writer: &mut W,
    payload: &[u8],
    max_len: u32,
) -> Result<(), FrameError> {
    let too_large = || FrameError::TooLarge {
        len: u64::try_from(payload.len()).unwrap_or(u64::MAX),
        max: max_len,
    };
    let len = u32::try_from(payload.len()).map_err(|_| too_large())?;
    if len > max_len {
        return Err(too_large());
    }
    writer
        .write_all(&len.to_le_bytes())
        .and_then(|()| writer.write_all(payload))
        .map_err(|e| FrameError::Io(e.kind()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn round_trip_and_clean_end() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"hello", 5).unwrap();
        write_frame(&mut buf, b"", 5).unwrap();
        let mut cursor = Cursor::new(buf);
        assert_eq!(read_frame(&mut cursor, 5), Ok(Some(b"hello".to_vec())));
        assert_eq!(read_frame(&mut cursor, 5), Ok(Some(Vec::new())));
        assert_eq!(read_frame(&mut cursor, 5), Ok(None));
    }

    #[test]
    fn oversized_length_is_rejected_before_allocation() {
        // Announces 4 GiB - 1 with no payload: must fail on the bound, not allocate.
        let mut cursor = Cursor::new(u32::MAX.to_le_bytes().to_vec());
        assert_eq!(
            read_frame(&mut cursor, 16),
            Err(FrameError::TooLarge {
                len: u64::from(u32::MAX),
                max: 16
            })
        );
    }

    #[test]
    fn limit_is_inclusive_on_both_sides() {
        let mut buf = Vec::new();
        assert_eq!(write_frame(&mut buf, b"abcd", 4), Ok(()));
        assert_eq!(
            write_frame(&mut buf, b"abcde", 4),
            Err(FrameError::TooLarge { len: 5, max: 4 })
        );
        let mut cursor = Cursor::new(buf.clone());
        assert_eq!(read_frame(&mut cursor, 4), Ok(Some(b"abcd".to_vec())));
        let mut cursor = Cursor::new(buf);
        assert_eq!(
            read_frame(&mut cursor, 3),
            Err(FrameError::TooLarge { len: 4, max: 3 })
        );
    }

    #[test]
    fn truncated_header_and_payload_are_reported() {
        let mut cursor = Cursor::new(vec![5u8, 0]);
        assert_eq!(read_frame(&mut cursor, 16), Err(FrameError::Truncated));

        let mut data = 5u32.to_le_bytes().to_vec();
        data.extend_from_slice(b"abc");
        let mut cursor = Cursor::new(data);
        assert_eq!(read_frame(&mut cursor, 16), Err(FrameError::Truncated));
    }
}
