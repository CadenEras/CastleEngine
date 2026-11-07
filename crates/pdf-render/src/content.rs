//! Content stream syntax (ISO 32000-2, 7.8.2): operands followed by an
//! operator, with inline images (8.9.7) skipped as one operation.

use pdf_syntax::{Budget, Lexer, Object, ObjectId, Parser, Token};

use crate::error::{ContentErrorKind, Error};

/// Most operands accepted before an operator. The largest standard operation,
/// `scn` with 32 colorants and a pattern name, takes 33.
pub const MAX_OPERANDS: usize = 64;

/// Most entries in an inline image dictionary.
const MAX_INLINE_IMAGE_ENTRIES: usize = 64;

/// One operation: its operator, where it starts, and its operands.
#[derive(Debug)]
pub(crate) struct Operation<'a> {
    pub operator: &'a [u8],
    /// Offset of the operator keyword in the decoded content.
    pub offset: u64,
    pub operands: &'a [Object],
}

pub(crate) struct ContentReader<'a> {
    data: &'a [u8],
    lexer: Lexer<'a>,
    page: ObjectId,
    operands: Vec<Object>,
}

impl<'a> ContentReader<'a> {
    pub fn new(data: &'a [u8], page: ObjectId) -> Self {
        Self {
            data,
            lexer: Lexer::new(data),
            page,
            operands: Vec::new(),
        }
    }

    pub fn error(&self, offset: u64, kind: ContentErrorKind) -> Error {
        Error::Content {
            page: self.page,
            offset,
            kind,
        }
    }

    /// Converts token-level errors, whose offsets are content offsets.
    fn syntax(&self, error: pdf_syntax::Error) -> Error {
        match error {
            pdf_syntax::Error::Malformed { offset, kind } => {
                self.error(offset, ContentErrorKind::Syntax(kind))
            }
            pdf_syntax::Error::Truncated { offset } => {
                self.error(offset, ContentErrorKind::Truncated)
            }
            other => Error::Syntax(other),
        }
    }

    /// The next operation, or `None` at the end of the content. Operands left
    /// without an operator at the end are an error.
    pub fn next(&mut self, budget: &mut Budget) -> Result<Option<Operation<'_>>, Error> {
        self.operands.clear();
        let mut first_operand = 0;
        loop {
            let Some(spanned) = self.lexer.next_token(budget).map_err(|e| self.syntax(e))? else {
                if self.operands.is_empty() {
                    return Ok(None);
                }
                return Err(self.error(first_operand, ContentErrorKind::Truncated));
            };
            let offset = spanned.offset;
            let operand = match spanned.token {
                Token::Keyword(b"true") => Object::Boolean(true),
                Token::Keyword(b"false") => Object::Boolean(false),
                Token::Keyword(b"null") => Object::Null,
                Token::Keyword(b"BI") => {
                    if !self.operands.is_empty() {
                        return Err(self.error(offset, ContentErrorKind::InvalidOperands));
                    }
                    self.skip_inline_image(offset, budget)?;
                    return Ok(Some(Operation {
                        operator: b"BI",
                        offset,
                        operands: &[],
                    }));
                }
                Token::Keyword(operator) => {
                    return Ok(Some(Operation {
                        operator,
                        offset,
                        operands: &self.operands,
                    }));
                }
                Token::Integer(value) => Object::Integer(value),
                Token::Real(value) => Object::Real(value),
                Token::LiteralString(bytes) | Token::HexString(bytes) => Object::String(bytes),
                Token::Name(name) => Object::Name(name),
                Token::ArrayStart | Token::DictStart => self.container(offset, budget)?,
                Token::ArrayEnd | Token::DictEnd | Token::BraceStart | Token::BraceEnd => {
                    return Err(self.error(
                        offset,
                        ContentErrorKind::Syntax(pdf_syntax::MalformedKind::UnexpectedToken),
                    ));
                }
            };
            if self.operands.is_empty() {
                first_operand = offset;
            }
            if self.operands.len() >= MAX_OPERANDS {
                return Err(self.error(offset, ContentErrorKind::TooManyOperands));
            }
            self.operands.push(operand);
        }
    }

    /// Parses an array or dictionary starting at `offset` with the object
    /// parser (charged per node, depth bounded), then resumes after it.
    fn container(&mut self, offset: u64, budget: &mut Budget) -> Result<Object, Error> {
        let mut parser = Parser::new(self.data);
        parser.seek(offset).map_err(|e| self.syntax(e))?;
        let object = parser.parse_object(budget).map_err(|e| self.syntax(e))?;
        self.lexer
            .seek(parser.position())
            .map_err(|e| self.syntax(e))?;
        Ok(object)
    }

    /// Skips `BI <key value pairs> ID <data> EI`. The data length comes from
    /// `/L` or `/Length` (PDF 2.0) when present; otherwise the data ends at the
    /// first `EI` preceded by white space and followed by white space, a
    /// delimiter or the end of the content (the usual heuristic: images are
    /// not decoded yet, only skipped).
    fn skip_inline_image(&mut self, start: u64, budget: &mut Budget) -> Result<(), Error> {
        let invalid = |reader: &Self| reader.error(start, ContentErrorKind::InvalidInlineImage);
        let mut length = None;
        let mut entries = 0usize;
        loop {
            let Some(spanned) = self.lexer.next_token(budget).map_err(|e| self.syntax(e))? else {
                return Err(invalid(self));
            };
            match spanned.token {
                Token::Keyword(b"ID") => break,
                Token::Name(key) => {
                    entries += 1;
                    if entries > MAX_INLINE_IMAGE_ENTRIES {
                        return Err(invalid(self));
                    }
                    let value = self.inline_value(budget)?;
                    if key == b"L" || key == b"Length" {
                        length = Some(value.as_integer().ok_or_else(|| invalid(self))?);
                    }
                }
                _ => return Err(invalid(self)),
            }
        }
        // Exactly one white-space byte separates `ID` from the data.
        let data_start = usize::try_from(self.lexer.position())
            .ok()
            .and_then(|p| p.checked_add(1))
            .filter(|p| *p <= self.data.len())
            .ok_or_else(|| invalid(self))?;
        let end = match length {
            Some(length) => {
                let length = usize::try_from(length).map_err(|_| invalid(self))?;
                let data_end = data_start
                    .checked_add(length)
                    .filter(|e| *e <= self.data.len())
                    .ok_or_else(|| invalid(self))?;
                self.lexer
                    .seek(data_end as u64)
                    .map_err(|e| self.syntax(e))?;
                match self.lexer.next_token(budget).map_err(|e| self.syntax(e))? {
                    Some(t) if t.token == Token::Keyword(b"EI") => return Ok(()),
                    _ => return Err(invalid(self)),
                }
            }
            None => find_end_marker(self.data, data_start).ok_or_else(|| invalid(self))?,
        };
        self.lexer.seek(end as u64).map_err(|e| self.syntax(e))
    }

    /// A value of an inline image dictionary: a scalar or a container.
    fn inline_value(&mut self, budget: &mut Budget) -> Result<Object, Error> {
        let Some(spanned) = self.lexer.next_token(budget).map_err(|e| self.syntax(e))? else {
            return Err(self.error(self.lexer.position(), ContentErrorKind::Truncated));
        };
        Ok(match spanned.token {
            Token::Integer(value) => Object::Integer(value),
            Token::Real(value) => Object::Real(value),
            Token::Name(name) => Object::Name(name),
            Token::LiteralString(bytes) | Token::HexString(bytes) => Object::String(bytes),
            Token::Keyword(b"true") => Object::Boolean(true),
            Token::Keyword(b"false") => Object::Boolean(false),
            Token::Keyword(b"null") => Object::Null,
            Token::ArrayStart | Token::DictStart => self.container(spanned.offset, budget)?,
            _ => {
                return Err(self.error(spanned.offset, ContentErrorKind::InvalidInlineImage));
            }
        })
    }
}

/// Offset just after the `EI` that ends inline image data starting at `from`.
fn find_end_marker(data: &[u8], from: usize) -> Option<usize> {
    let tail = data.get(from..)?;
    tail.windows(2).enumerate().find_map(|(i, pair)| {
        let at = from + i;
        let before = at.checked_sub(1).and_then(|b| data.get(b)).copied();
        let after = data.get(at + 2).copied();
        let ends =
            after.is_none_or(|b| pdf_syntax::is_whitespace(b) || pdf_syntax::is_delimiter(b));
        (pair == b"EI" && before.is_some_and(pdf_syntax::is_whitespace) && ends).then_some(at + 2)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pdf_syntax::{CancellationToken, ResourceLimits};

    fn budget() -> Budget {
        Budget::new(ResourceLimits::default(), CancellationToken::new())
    }

    fn page() -> ObjectId {
        ObjectId::new(3, 0).unwrap_or_else(|| unreachable!())
    }

    fn operators(data: &[u8]) -> Result<Vec<(String, usize)>, Error> {
        let mut reader = ContentReader::new(data, page());
        let mut budget = budget();
        let mut out = Vec::new();
        while let Some(op) = reader.next(&mut budget)? {
            out.push((
                String::from_utf8_lossy(op.operator).into_owned(),
                op.operands.len(),
            ));
        }
        Ok(out)
    }

    fn kind(data: &[u8]) -> Option<(u64, ContentErrorKind)> {
        match operators(data) {
            Err(Error::Content { offset, kind, .. }) => Some((offset, kind)),
            _ => None,
        }
    }

    #[test]
    fn reads_operations_and_operands() -> Result<(), Error> {
        let ops = operators(b"q 1 0 0 1 10 20 cm\n[3 2] 0 d /GS1 gs <</MCID 0>> BDC EMC Q")?;
        let names: Vec<_> = ops.iter().map(|(o, n)| format!("{o}/{n}")).collect();
        assert_eq!(
            names,
            ["q/0", "cm/6", "d/2", "gs/1", "BDC/1", "EMC/0", "Q/0"]
        );
        assert_eq!(operators(b"")?, []);
        assert_eq!(operators(b" % comment only\n")?, []);
        Ok(())
    }

    #[test]
    fn skips_inline_images() -> Result<(), Error> {
        let ops = operators(b"q BI /W 2 /H 1 /CS /G /BPC 8 ID \x00EI\xff EI Q")?;
        assert_eq!(
            ops.iter().map(|(o, _)| o.as_str()).collect::<Vec<_>>(),
            ["q", "BI", "Q"]
        );
        // PDF 2.0 length: the data may contain a delimited `EI`.
        let ops = operators(b"BI /W 4 /H 1 /L 4 ID  EI EI Q")?;
        assert_eq!(
            ops.iter().map(|(o, _)| o.as_str()).collect::<Vec<_>>(),
            ["BI", "Q"]
        );
        let ops = operators(b"BI /W 1 /D [1 0] ID x EI")?;
        assert_eq!(ops.len(), 1);
        Ok(())
    }

    #[test]
    fn rejects_invalid_content() {
        assert_eq!(kind(b"1 2"), Some((0, ContentErrorKind::Truncated)));
        assert_eq!(kind(b"q 1 2"), Some((2, ContentErrorKind::Truncated)));
        assert_eq!(
            kind(b"q ] Q"),
            Some((
                2,
                ContentErrorKind::Syntax(pdf_syntax::MalformedKind::UnexpectedToken)
            ))
        );
        assert_eq!(
            kind(b"1 BI ID x EI"),
            Some((2, ContentErrorKind::InvalidOperands))
        );
        assert_eq!(
            kind(b"BI /W 1 ID xEI"),
            Some((0, ContentErrorKind::InvalidInlineImage))
        );
        assert_eq!(
            kind(b"BI /W 1"),
            Some((0, ContentErrorKind::InvalidInlineImage))
        );
        assert_eq!(
            kind(b"BI 1 ID x EI"),
            Some((0, ContentErrorKind::InvalidInlineImage))
        );
        assert_eq!(
            kind(b"BI /L 9 ID x EI"),
            Some((0, ContentErrorKind::InvalidInlineImage))
        );
        assert_eq!(
            kind(b"BI /L -1 ID x EI"),
            Some((0, ContentErrorKind::InvalidInlineImage))
        );
        let many = "0 ".repeat(MAX_OPERANDS + 1) + "m";
        assert_eq!(
            kind(many.as_bytes()),
            Some((2 * MAX_OPERANDS as u64, ContentErrorKind::TooManyOperands))
        );
        let exact = "0 ".repeat(MAX_OPERANDS) + "m";
        assert!(operators(exact.as_bytes()).is_ok());
        assert_eq!(kind(b"[1 2"), Some((0, ContentErrorKind::Truncated)));
    }

    #[test]
    fn cancellation_stops_reading() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let mut budget = Budget::new(ResourceLimits::default(), cancel);
        let mut reader = ContentReader::new(b"q Q", page());
        assert!(matches!(
            reader.next(&mut budget),
            Err(Error::Syntax(pdf_syntax::Error::Cancelled))
        ));
    }
}
