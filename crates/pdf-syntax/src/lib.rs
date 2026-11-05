//! PDF syntax layer. P0 provides only the shared resource budgets and errors;
//! objects, tokenizer and cross-reference parsing arrive in P1.

mod limits;

pub use limits::{Budget, CancellationToken, Error, LimitKind, ResourceLimits};
