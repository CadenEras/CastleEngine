//! PDF syntax layer: resource budgets, input sources, the tokenizer and the
//! object parser, and the file structure (classic cross-reference tables).

mod filter;
mod lexer;
mod limits;
mod object;
mod objstm;
mod parser;
mod source;
mod xref;

pub use filter::decode_stream;
pub use lexer::{Lexer, Spanned, Token, is_delimiter, is_regular, is_whitespace};
pub use limits::{
    Budget, CancellationToken, Capability, Error, LimitKind, MalformedKind, ResourceLimits,
};
pub use object::{Dictionary, IndirectObject, IndirectValue, Object, ObjectId, Stream};
pub use parser::{LengthResolver, MAX_SUPPORTED_NESTING_DEPTH, Parser};
pub use source::{MemorySource, Source};
pub use xref::{PdfFile, SectionKind, Version, XrefEntry, XrefSection};
