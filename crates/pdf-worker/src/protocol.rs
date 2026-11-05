//! Versioned messages carried inside frames (see `frame`).
//!
//! Wire format: one tag byte, then fixed-size little-endian fields. A message
//! with trailing bytes, a missing field or an unknown tag/code is malformed.
//! The `Hello` layout (tag + `u16` version) never changes, so mismatched
//! versions are always detected.

use std::fmt;

use pdf_syntax::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    Hello {
        version: u16,
    },
    /// Synthetic job: `units` work units, sleeping `unit_delay_ms` after each.
    /// `id` must be strictly greater than every previous job id of the
    /// connection, otherwise the job fails with `InvalidJobId`.
    RunSynthetic {
        id: u64,
        units: u64,
        unit_delay_ms: u8,
    },
    Cancel {
        id: u64,
    },
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Response {
    HelloAck {
        version: u16,
    },
    JobDone {
        id: u64,
        units: u64,
    },
    JobFailed {
        id: u64,
        code: ErrorCode,
    },
    /// Fatal for the connection: last frame sent, then the worker exits.
    ProtocolError {
        code: ErrorCode,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ErrorCode {
    Cancelled = 1,
    LimitExceeded = 2,
    Busy = 3,
    MalformedMessage = 4,
    UnsupportedVersion = 5,
    FrameTooLarge = 6,
    Internal = 7,
    InvalidJobId = 8,
}

impl ErrorCode {
    fn from_u8(value: u8) -> Option<Self> {
        Some(match value {
            1 => Self::Cancelled,
            2 => Self::LimitExceeded,
            3 => Self::Busy,
            4 => Self::MalformedMessage,
            5 => Self::UnsupportedVersion,
            6 => Self::FrameTooLarge,
            7 => Self::Internal,
            8 => Self::InvalidJobId,
            _ => return None,
        })
    }
}

impl From<&Error> for ErrorCode {
    fn from(error: &Error) -> Self {
        match error {
            Error::Cancelled => Self::Cancelled,
            Error::LimitExceeded { .. } => Self::LimitExceeded,
            _ => Self::Internal,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DecodeError;

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("malformed worker message")
    }
}

impl std::error::Error for DecodeError {}

const HELLO: u8 = 0x01;
const RUN_SYNTHETIC: u8 = 0x02;
const CANCEL: u8 = 0x03;
const SHUTDOWN: u8 = 0x04;
const HELLO_ACK: u8 = 0x81;
const JOB_DONE: u8 = 0x82;
const JOB_FAILED: u8 = 0x83;
const PROTOCOL_ERROR: u8 = 0x84;

impl Request {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(18);
        match *self {
            Self::Hello { version } => {
                out.push(HELLO);
                out.extend_from_slice(&version.to_le_bytes());
            }
            Self::RunSynthetic {
                id,
                units,
                unit_delay_ms,
            } => {
                out.push(RUN_SYNTHETIC);
                out.extend_from_slice(&id.to_le_bytes());
                out.extend_from_slice(&units.to_le_bytes());
                out.push(unit_delay_ms);
            }
            Self::Cancel { id } => {
                out.push(CANCEL);
                out.extend_from_slice(&id.to_le_bytes());
            }
            Self::Shutdown => out.push(SHUTDOWN),
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader(bytes);
        let message = match r.u8()? {
            HELLO => Self::Hello { version: r.u16()? },
            RUN_SYNTHETIC => Self::RunSynthetic {
                id: r.u64()?,
                units: r.u64()?,
                unit_delay_ms: r.u8()?,
            },
            CANCEL => Self::Cancel { id: r.u64()? },
            SHUTDOWN => Self::Shutdown,
            _ => return Err(DecodeError),
        };
        r.finish()?;
        Ok(message)
    }
}

impl Response {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(17);
        match *self {
            Self::HelloAck { version } => {
                out.push(HELLO_ACK);
                out.extend_from_slice(&version.to_le_bytes());
            }
            Self::JobDone { id, units } => {
                out.push(JOB_DONE);
                out.extend_from_slice(&id.to_le_bytes());
                out.extend_from_slice(&units.to_le_bytes());
            }
            Self::JobFailed { id, code } => {
                out.push(JOB_FAILED);
                out.extend_from_slice(&id.to_le_bytes());
                out.push(code as u8);
            }
            Self::ProtocolError { code } => {
                out.push(PROTOCOL_ERROR);
                out.push(code as u8);
            }
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Self, DecodeError> {
        let mut r = Reader(bytes);
        let message = match r.u8()? {
            HELLO_ACK => Self::HelloAck { version: r.u16()? },
            JOB_DONE => Self::JobDone {
                id: r.u64()?,
                units: r.u64()?,
            },
            JOB_FAILED => Self::JobFailed {
                id: r.u64()?,
                code: r.code()?,
            },
            PROTOCOL_ERROR => Self::ProtocolError { code: r.code()? },
            _ => return Err(DecodeError),
        };
        r.finish()?;
        Ok(message)
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], DecodeError> {
        let (head, rest) = self.0.split_first_chunk::<N>().ok_or(DecodeError)?;
        self.0 = rest;
        Ok(*head)
    }

    fn u8(&mut self) -> Result<u8, DecodeError> {
        self.take::<1>().map(|[b]| b)
    }

    fn u16(&mut self) -> Result<u16, DecodeError> {
        self.take().map(u16::from_le_bytes)
    }

    fn u64(&mut self) -> Result<u64, DecodeError> {
        self.take().map(u64::from_le_bytes)
    }

    fn code(&mut self) -> Result<ErrorCode, DecodeError> {
        ErrorCode::from_u8(self.u8()?).ok_or(DecodeError)
    }

    fn finish(self) -> Result<(), DecodeError> {
        if self.0.is_empty() {
            Ok(())
        } else {
            Err(DecodeError)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const REQUESTS: [Request; 4] = [
        Request::Hello { version: 1 },
        Request::RunSynthetic {
            id: u64::MAX,
            units: 42,
            unit_delay_ms: 255,
        },
        Request::Cancel { id: 7 },
        Request::Shutdown,
    ];

    const RESPONSES: [Response; 4] = [
        Response::HelloAck { version: 1 },
        Response::JobDone { id: 3, units: 9 },
        Response::JobFailed {
            id: 3,
            code: ErrorCode::Busy,
        },
        Response::ProtocolError {
            code: ErrorCode::FrameTooLarge,
        },
    ];

    #[test]
    fn messages_round_trip() {
        for request in REQUESTS {
            assert_eq!(Request::decode(&request.encode()), Ok(request));
        }
        for response in RESPONSES {
            assert_eq!(Response::decode(&response.encode()), Ok(response));
        }
    }

    #[test]
    fn trailing_and_missing_bytes_are_rejected() {
        for request in REQUESTS {
            let mut bytes = request.encode();
            bytes.push(0);
            assert_eq!(Request::decode(&bytes), Err(DecodeError));
            bytes.truncate(bytes.len() - 2);
            if !bytes.is_empty() {
                assert_eq!(Request::decode(&bytes), Err(DecodeError));
            }
        }
        for response in RESPONSES {
            let mut bytes = response.encode();
            bytes.push(0);
            assert_eq!(Response::decode(&bytes), Err(DecodeError));
            bytes.truncate(bytes.len() - 2);
            assert_eq!(Response::decode(&bytes), Err(DecodeError));
        }
    }

    #[test]
    fn unknown_tags_and_codes_are_rejected() {
        assert_eq!(Request::decode(&[]), Err(DecodeError));
        assert_eq!(Request::decode(&[0x7f]), Err(DecodeError));
        assert_eq!(Request::decode(&[HELLO_ACK, 1, 0]), Err(DecodeError));
        assert_eq!(Response::decode(&[PROTOCOL_ERROR, 0]), Err(DecodeError));
        assert_eq!(Response::decode(&[PROTOCOL_ERROR, 9]), Err(DecodeError));
    }

    #[test]
    fn hello_layout_is_stable() {
        assert_eq!(
            Request::Hello { version: 0x0102 }.encode(),
            vec![HELLO, 0x02, 0x01]
        );
    }
}
