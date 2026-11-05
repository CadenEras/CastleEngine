//! Worker side of the shell/worker boundary (ADR-0001). P0 provides bounded
//! framing and a synthetic cancellable job; the worker binary and request
//! messages come in a later slice.

pub mod frame;
pub mod job;

/// Incremented on any incompatible change of the framing or messages.
pub const PROTOCOL_VERSION: u16 = 1;
