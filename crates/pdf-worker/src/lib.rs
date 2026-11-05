//! Worker side of the shell/worker boundary (ADR-0001): bounded framing,
//! versioned messages, the request loop run by the `pdf-worker` binary and a
//! client used by the shell to drive that process.

pub mod client;
pub mod frame;
pub mod job;
pub mod protocol;
pub mod server;

/// Incremented on any incompatible change of the framing or messages.
pub const PROTOCOL_VERSION: u16 = 1;
