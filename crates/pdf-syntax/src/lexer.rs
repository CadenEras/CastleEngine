//! Tokenizer for PDF syntax (ISO 32000-2:2020, 7.2 and 7.3).
//!
//! Strict by design (ADR-0002): a byte sequence that is not valid PDF syntax is
//! reported with its offset instead of being guessed. Decoded strings and names
//! are charged to the `Budget` before they are allocated.

use std::io;

use crate::limits::{Budget, Error, MalformedKind};

/// One lexical token. Keywords borrow from the input; strings and names are
/// decoded (escapes resolved) into owned bytes.
#[derive(Debug, Clone, PartialEq)]
pub enum Token<'a> {
    Integer(i64),
    Real(f64),
    /// `( ... )` with escapes resolved and end-of-line markers normalised to LF.
    LiteralString(Vec<u8>),
    /// `< ... >` decoded; an odd final digit is completed by `0`.
    HexString(Vec<u8>),
    /// `/Name` without the slash, `#xx` escapes resolved.
    Name(Vec<u8>),
    ArrayStart,
    ArrayEnd,
    DictStart,
    DictEnd,
    /// `{` and `}`, used by PostScript calculator functions.
    BraceStart,
    BraceEnd,
    /// Any other run of regular characters: `obj`, `R`, `true`, `null`, operators.
    Keyword(&'a [u8]),
}

/// A token and the byte offset where it starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Spanned<'a> {
    pub offset: u64,
    pub token: Token<'a>,
}

/// White-space characters (ISO 32000-2, table 1).
pub fn is_whitespace(byte: u8) -> bool {
    matches!(byte, 0 | 9 | 10 | 12 | 13 | 32)
}

/// Delimiter characters (ISO 32000-2, table 2).
pub fn is_delimiter(byte: u8) -> bool {
    matches!(
        byte,
        b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%'
    )
}

/// Regular characters: neither white space nor delimiters.
pub fn is_regular(byte: u8) -> bool {
    !is_whitespace(byte) && !is_delimiter(byte)
}

/// Cursor over the bytes of a document or of a decoded content stream.
///
/// ```
/// use pdf_syntax::{Budget, CancellationToken, Lexer, ResourceLimits, Token};
///
/// let mut budget = Budget::new(ResourceLimits::default(), CancellationToken::new());
/// let mut lexer = Lexer::new(b"1 0 obj << /Type /Page >>");
/// let first = lexer.next_token(&mut budget)?.map(|t| t.token);
/// assert_eq!(first, Some(Token::Integer(1)));
/// # Ok::<(), pdf_syntax::Error>(())
/// ```
#[derive(Debug, Clone)]
pub struct Lexer<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Offset of the next byte to read.
    pub fn position(&self) -> u64 {
        self.pos as u64
    }

    /// Moves the cursor; an offset past the end of the input is refused.
    pub fn seek(&mut self, offset: u64) -> Result<(), Error> {
        self.pos = usize::try_from(offset)
            .ok()
            .filter(|pos| *pos <= self.data.len())
            .ok_or(Error::Truncated { offset })?;
        Ok(())
    }

    /// Returns the next token, or `None` at the end of the input.
    ///
    /// Errors are terminal for this position: the cursor stays at the start of
    /// the faulty token, so calling again returns the same error (and charges the
    /// budget again). P1 does not resynchronise (ADR-0002); a caller must stop or
    /// `seek` elsewhere. Cancellation is checked between tokens; a single token is
    /// scanned in time linear in its length, bounded by `max_input_bytes`.
    pub fn next_token(&mut self, budget: &mut Budget) -> Result<Option<Spanned<'a>>, Error> {
        budget.check_cancelled()?;
        self.skip_whitespace_and_comments();
        let start = self.pos;
        let Some(first) = self.byte(start) else {
            return Ok(None);
        };
        let token = match first {
            b'[' => self.single(Token::ArrayStart),
            b']' => self.single(Token::ArrayEnd),
            b'{' => self.single(Token::BraceStart),
            b'}' => self.single(Token::BraceEnd),
            b'<' if self.byte(start + 1) == Some(b'<') => {
                self.pos = start + 2;
                Token::DictStart
            }
            b'<' => self.hex_string(start, budget)?,
            b'>' if self.byte(start + 1) == Some(b'>') => {
                self.pos = start + 2;
                Token::DictEnd
            }
            b'>' | b')' => return Err(malformed(start, MalformedKind::UnexpectedDelimiter)),
            b'(' => self.literal_string(start, budget)?,
            b'/' => self.name(start, budget)?,
            b'+' | b'-' | b'.' | b'0'..=b'9' => self.number(start)?,
            _ => {
                let end = self.scan_regular(start);
                self.pos = end;
                Token::Keyword(self.slice(start, end)?)
            }
        };
        Ok(Some(Spanned {
            offset: start as u64,
            token,
        }))
    }

    fn byte(&self, index: usize) -> Option<u8> {
        self.data.get(index).copied()
    }

    fn slice(&self, start: usize, end: usize) -> Result<&'a [u8], Error> {
        self.data.get(start..end).ok_or(Error::Truncated {
            offset: start as u64,
        })
    }

    fn single(&mut self, token: Token<'a>) -> Token<'a> {
        self.pos += 1;
        token
    }

    fn skip_whitespace_and_comments(&mut self) {
        while let Some(byte) = self.byte(self.pos) {
            if is_whitespace(byte) {
                self.pos += 1;
            } else if byte == b'%' {
                // A comment runs to the end of the line; the EOL is white space.
                while self
                    .byte(self.pos)
                    .is_some_and(|b| b != b'\r' && b != b'\n')
                {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn scan_regular(&self, from: usize) -> usize {
        let mut end = from;
        while self.byte(end).is_some_and(is_regular) {
            end += 1;
        }
        end
    }

    fn number(&mut self, start: usize) -> Result<Token<'a>, Error> {
        let end = self.scan_regular(start);
        let token = parse_number(self.slice(start, end)?).map_err(|kind| malformed(start, kind))?;
        self.pos = end;
        Ok(token)
    }

    fn literal_string(&mut self, start: usize, budget: &mut Budget) -> Result<Token<'a>, Error> {
        // First pass: find the matching `)`; escaped bytes never count.
        let mut depth: u64 = 0;
        let mut i = start;
        let end = loop {
            match self.byte(i) {
                None => {
                    return Err(Error::Truncated {
                        offset: start as u64,
                    });
                }
                Some(b'\\') => i += 2,
                Some(b'(') => {
                    depth += 1;
                    i += 1;
                }
                Some(b')') => {
                    depth -= 1;
                    if depth == 0 {
                        break i;
                    }
                    i += 1;
                }
                Some(_) => i += 1,
            }
        };
        // The raw content length bounds the decoded length.
        let mut out = reserve(end - start - 1, budget)?;
        let mut j = start + 1;
        while j < end {
            let byte = self.byte(j).ok_or(Error::Truncated {
                offset: start as u64,
            })?;
            j += 1;
            match byte {
                b'\\' => {
                    let escaped = self.byte(j).ok_or(Error::Truncated {
                        offset: start as u64,
                    })?;
                    j += 1;
                    match escaped {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(0x08),
                        b'f' => out.push(0x0C),
                        b'0'..=b'7' => {
                            // Up to three octal digits; high-order overflow is ignored.
                            let mut value = u32::from(escaped - b'0');
                            for _ in 0..2 {
                                match self.byte(j) {
                                    Some(d @ b'0'..=b'7') if j < end => {
                                        value = value * 8 + u32::from(d - b'0');
                                        j += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push((value & 0xFF) as u8);
                        }
                        // A backslash before an end-of-line continues the string.
                        b'\r' => {
                            if j < end && self.byte(j) == Some(b'\n') {
                                j += 1;
                            }
                        }
                        b'\n' => {}
                        // `\(`, `\)`, `\\`, and an unknown escape: the backslash is ignored.
                        other => out.push(other),
                    }
                }
                // An unescaped end-of-line (CR, LF or CRLF) reads as LF.
                b'\r' => {
                    if j < end && self.byte(j) == Some(b'\n') {
                        j += 1;
                    }
                    out.push(b'\n');
                }
                other => out.push(other),
            }
        }
        self.pos = end + 1;
        Ok(Token::LiteralString(out))
    }

    fn hex_string(&mut self, start: usize, budget: &mut Budget) -> Result<Token<'a>, Error> {
        let mut i = start + 1;
        let mut digits: usize = 0;
        let end = loop {
            match self.byte(i) {
                None => {
                    return Err(Error::Truncated {
                        offset: start as u64,
                    });
                }
                Some(b'>') => break i,
                Some(b) if hex_value(b).is_some() => digits += 1,
                Some(b) if is_whitespace(b) => {}
                Some(_) => return Err(malformed(i, MalformedKind::InvalidHexDigit)),
            }
            i += 1;
        };
        let mut out = reserve(digits.div_ceil(2), budget)?;
        let mut high: Option<u8> = None;
        for &b in self.slice(start + 1, end)? {
            if let Some(v) = hex_value(b) {
                match high.take() {
                    None => high = Some(v),
                    Some(h) => out.push((h << 4) | v),
                }
            }
        }
        if let Some(h) = high {
            out.push(h << 4);
        }
        self.pos = end + 1;
        Ok(Token::HexString(out))
    }

    fn name(&mut self, start: usize, budget: &mut Budget) -> Result<Token<'a>, Error> {
        let end = self.scan_regular(start + 1);
        let raw = self.slice(start + 1, end)?;
        let mut out = reserve(raw.len(), budget)?;
        let mut k = 0;
        while let Some(&b) = raw.get(k) {
            if b == b'#' {
                let escape = start + 1 + k;
                let value = match (raw.get(k + 1), raw.get(k + 2)) {
                    (Some(&h), Some(&l)) => {
                        hex_value(h).zip(hex_value(l)).map(|(h, l)| (h << 4) | l)
                    }
                    _ => None,
                }
                .ok_or_else(|| malformed(escape, MalformedKind::InvalidNameEscape))?;
                if value == 0 {
                    return Err(malformed(escape, MalformedKind::NullInName));
                }
                out.push(value);
                k += 3;
            } else {
                out.push(b);
                k += 1;
            }
        }
        self.pos = end;
        Ok(Token::Name(out))
    }
}

fn malformed(offset: usize, kind: MalformedKind) -> Error {
    Error::Malformed {
        offset: offset as u64,
        kind,
    }
}

/// Charges `capacity` decoded bytes, then allocates them without aborting on failure.
fn reserve(capacity: usize, budget: &mut Budget) -> Result<Vec<u8>, Error> {
    budget.charge_decoded_bytes(capacity as u64)?;
    let mut out = Vec::new();
    out.try_reserve_exact(capacity)
        .map_err(|_| Error::Io(io::ErrorKind::OutOfMemory))?;
    Ok(out)
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

/// Numbers (ISO 32000-2, 7.3.3): optional sign, digits, at most one period,
/// no exponent. Integers are `i64`; reals must be finite.
fn parse_number(text: &[u8]) -> Result<Token<'static>, MalformedKind> {
    let (negative, body) = match text.split_first() {
        Some((b'-', rest)) => (true, rest),
        Some((b'+', rest)) => (false, rest),
        _ => (false, text),
    };
    let mut periods = 0;
    let mut digits = 0;
    for &b in body {
        match b {
            b'0'..=b'9' => digits += 1,
            b'.' => periods += 1,
            _ => return Err(MalformedKind::InvalidNumber),
        }
    }
    if digits == 0 || periods > 1 {
        return Err(MalformedKind::InvalidNumber);
    }
    if periods == 0 {
        // Accumulate on the sign's side so that i64::MIN is representable.
        let mut value: i64 = 0;
        for &b in body {
            let digit = i64::from(b - b'0');
            value = value
                .checked_mul(10)
                .and_then(|v| {
                    if negative {
                        v.checked_sub(digit)
                    } else {
                        v.checked_add(digit)
                    }
                })
                .ok_or(MalformedKind::NumberOutOfRange)?;
        }
        return Ok(Token::Integer(value));
    }
    let value: f64 = std::str::from_utf8(text)
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or(MalformedKind::InvalidNumber)?;
    if value.is_finite() {
        Ok(Token::Real(value))
    } else {
        Err(MalformedKind::NumberOutOfRange)
    }
}

#[cfg(test)]
mod tests;
