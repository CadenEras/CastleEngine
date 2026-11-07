//! PDF syntax layer: resource budgets, input sources, the tokenizer and the
//! object parser. Cross-reference parsing builds on these in P1 (ADR-0002).

mod lexer;
mod limits;
mod object;
mod parser;
mod source;

pub use lexer::{Lexer, Spanned, Token, is_delimiter, is_regular, is_whitespace};
pub use limits::{
    Budget, CancellationToken, Capability, Error, LimitKind, MalformedKind, ResourceLimits,
};
pub use object::{Dictionary, IndirectObject, IndirectValue, Object, ObjectId, Stream};
pub use parser::{LengthResolver, MAX_SUPPORTED_NESTING_DEPTH, Parser};
pub use source::MemorySource;
