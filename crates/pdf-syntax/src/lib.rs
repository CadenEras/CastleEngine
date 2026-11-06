//! PDF syntax layer: resource budgets, input sources and the tokenizer.
//! Objects and cross-reference parsing build on these in P1 (ADR-0002).

mod lexer;
mod limits;
mod source;

pub use lexer::{Lexer, Spanned, Token, is_delimiter, is_regular, is_whitespace};
pub use limits::{Budget, CancellationToken, Error, LimitKind, MalformedKind, ResourceLimits};
pub use source::MemorySource;
