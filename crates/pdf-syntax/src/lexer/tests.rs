use super::*;
use crate::limits::{CancellationToken, LimitKind, ResourceLimits};
use proptest::prelude::*;

fn budget() -> Budget {
    Budget::new(ResourceLimits::default(), CancellationToken::new())
}

fn budget_with_decoded(max_decoded_bytes: u64) -> Budget {
    let limits = ResourceLimits {
        max_decoded_bytes,
        ..ResourceLimits::default()
    };
    Budget::new(limits, CancellationToken::new())
}

/// All tokens of `input`, or the first error.
fn lex(input: &[u8]) -> Result<Vec<Token<'_>>, Error> {
    let mut budget = budget();
    let mut lexer = Lexer::new(input);
    let mut tokens = Vec::new();
    while let Some(spanned) = lexer.next_token(&mut budget)? {
        tokens.push(spanned.token);
    }
    Ok(tokens)
}

fn one(input: &[u8]) -> Token<'_> {
    let tokens = lex(input).unwrap();
    assert_eq!(tokens.len(), 1, "{tokens:?}");
    tokens.into_iter().next().unwrap()
}

fn malformed_at(offset: u64, kind: MalformedKind) -> Error {
    Error::Malformed { offset, kind }
}

fn bytes(token: Token<'_>) -> Vec<u8> {
    match token {
        Token::LiteralString(b) | Token::HexString(b) | Token::Name(b) => b,
        other => panic!("not a byte token: {other:?}"),
    }
}

#[test]
fn whitespace_and_comments_separate_tokens() {
    let tokens = lex(b"\0\t\x0C 12%comment )(<\r\n/A%x\r% y\n\rtrue").unwrap();
    assert_eq!(
        tokens,
        [
            Token::Integer(12),
            Token::Name(b"A".to_vec()),
            Token::Keyword(b"true")
        ]
    );
    assert_eq!(lex(b"% only a comment").unwrap(), []);
    assert_eq!(lex(b"").unwrap(), []);
}

#[test]
fn integers_follow_iso_32000_examples() {
    for (text, value) in [
        (&b"123"[..], 123),
        (b"43445", 43445),
        (b"+17", 17),
        (b"-98", -98),
        (b"0", 0),
        (b"-0", 0),
        (b"007", 7),
        (b"9223372036854775807", i64::MAX),
        (b"-9223372036854775808", i64::MIN),
    ] {
        assert_eq!(one(text), Token::Integer(value), "{text:?}");
    }
}

#[test]
fn reals_follow_iso_32000_examples() {
    for (text, value) in [
        (&b"34.5"[..], 34.5),
        (b"-3.62", -3.62),
        (b"+123.6", 123.6),
        (b"4.", 4.0),
        (b"-.002", -0.002),
        (b"0.0", 0.0),
        (b".5", 0.5),
    ] {
        assert_eq!(one(text), Token::Real(value), "{text:?}");
    }
}

#[test]
fn invalid_numbers_are_reported_at_their_start() {
    for text in [
        &b"-"[..],
        b"+",
        b".",
        b"-.",
        b"1.2.3",
        b"12a",
        b"--5",
        b"1e5",
        b"6-",
    ] {
        let mut input = b"  ".to_vec();
        input.extend_from_slice(text);
        assert_eq!(
            lex(&input),
            Err(malformed_at(2, MalformedKind::InvalidNumber)),
            "{text:?}"
        );
    }
}

#[test]
fn out_of_range_numbers_are_refused() {
    for text in [&b"9223372036854775808"[..], b"-9223372036854775809"] {
        assert_eq!(
            lex(text),
            Err(malformed_at(0, MalformedKind::NumberOutOfRange))
        );
    }
    let huge = format!("{}.0", "9".repeat(400));
    assert_eq!(
        lex(huge.as_bytes()),
        Err(malformed_at(0, MalformedKind::NumberOutOfRange))
    );
}

#[test]
fn literal_strings_follow_iso_32000_examples() {
    assert_eq!(bytes(one(b"(This is a string)")), b"This is a string");
    assert_eq!(
        bytes(one(b"(Strings may contain newlines\nand such.)")),
        b"Strings may contain newlines\nand such."
    );
    assert_eq!(
        bytes(one(
            b"(Strings may contain balanced parentheses ( ) and\nspecial characters (*!&}^% and so on).)"
        )),
        b"Strings may contain balanced parentheses ( ) and\nspecial characters (*!&}^% and so on)."
    );
    assert_eq!(bytes(one(b"()")), b"");
    assert_eq!(
        bytes(one(b"(These \\\ntwo strings \\\r\nare \\\rthe same.)")),
        b"These two strings are the same."
    );
}

#[test]
fn literal_string_escapes() {
    assert_eq!(
        bytes(one(b"(\\n\\r\\t\\b\\f\\(\\)\\\\\\q)")),
        b"\n\r\t\x08\x0C()\\q"
    );
    // Octal: one to three digits, high-order overflow ignored.
    assert_eq!(bytes(one(b"(\\0053)")), b"\x053");
    assert_eq!(bytes(one(b"(\\053)")), b"+");
    assert_eq!(bytes(one(b"(\\5)")), b"\x05");
    assert_eq!(bytes(one(b"(\\777)")), b"\xFF");
    assert_eq!(bytes(one(b"(\\12)")), b"\n");
    // Escaped parentheses do not count for balancing.
    assert_eq!(bytes(one(b"(a\\)b)")), b"a)b");
}

#[test]
fn literal_string_end_of_line_is_normalised() {
    assert_eq!(bytes(one(b"(a\r\nb\rc\nd)")), b"a\nb\nc\nd");
    assert_eq!(bytes(one(b"(\\r)")), b"\r");
}

#[test]
fn unterminated_literal_strings_are_truncated() {
    for text in [&b"(abc"[..], b"(a(b)", b"(a\\", b"(a\\)"] {
        let mut input = b" ".to_vec();
        input.extend_from_slice(text);
        assert_eq!(lex(&input), Err(Error::Truncated { offset: 1 }), "{text:?}");
    }
}

#[test]
fn hex_strings_follow_iso_32000_examples() {
    assert_eq!(
        bytes(one(b"<4E6F762073686D6F7A206B6120706F702E>")),
        b"Nov shmoz ka pop."
    );
    assert_eq!(bytes(one(b"<901FA3>")), [0x90, 0x1F, 0xA3]);
    assert_eq!(bytes(one(b"<901FA>")), [0x90, 0x1F, 0xA0]);
    assert_eq!(bytes(one(b"< 9 0\n1f\ta3 >")), [0x90, 0x1F, 0xA3]);
    assert_eq!(bytes(one(b"<>")), b"");
}

#[test]
fn invalid_hex_strings() {
    assert_eq!(
        lex(b"<4G>"),
        Err(malformed_at(2, MalformedKind::InvalidHexDigit))
    );
    assert_eq!(lex(b" <4E"), Err(Error::Truncated { offset: 1 }));
    assert_eq!(lex(b"<"), Err(Error::Truncated { offset: 0 }));
}

#[test]
fn names_follow_iso_32000_examples() {
    for (text, name) in [
        (&b"/Name1"[..], &b"Name1"[..]),
        (b"/ASomewhatLongerName", b"ASomewhatLongerName"),
        (
            b"/A;Name_With-Various***Characters?",
            b"A;Name_With-Various***Characters?",
        ),
        (b"/1.2", b"1.2"),
        (b"/$$", b"$$"),
        (b"/@pattern", b"@pattern"),
        (b"/.notdef", b".notdef"),
        (b"/Lime#20Green", b"Lime Green"),
        (b"/paired#28#29parentheses", b"paired()parentheses"),
        (b"/The_Key_of_F#23_Minor", b"The_Key_of_F#_Minor"),
        (b"/A#42", b"AB"),
        (b"/", b""),
    ] {
        assert_eq!(one(text), Token::Name(name.to_vec()), "{text:?}");
    }
}

#[test]
fn invalid_name_escapes() {
    assert_eq!(
        lex(b"/A#00"),
        Err(malformed_at(2, MalformedKind::NullInName))
    );
    assert_eq!(
        lex(b"/A#4"),
        Err(malformed_at(2, MalformedKind::InvalidNameEscape))
    );
    assert_eq!(
        lex(b"/A#G1"),
        Err(malformed_at(2, MalformedKind::InvalidNameEscape))
    );
    assert_eq!(
        lex(b" /#"),
        Err(malformed_at(2, MalformedKind::InvalidNameEscape))
    );
}

#[test]
fn delimiters_need_no_surrounding_space() {
    assert_eq!(
        lex(b"[/A(b)<<>>{}<41>]").unwrap(),
        [
            Token::ArrayStart,
            Token::Name(b"A".to_vec()),
            Token::LiteralString(b"b".to_vec()),
            Token::DictStart,
            Token::DictEnd,
            Token::BraceStart,
            Token::BraceEnd,
            Token::HexString(b"A".to_vec()),
            Token::ArrayEnd,
        ]
    );
}

#[test]
fn unexpected_delimiters_are_reported() {
    assert_eq!(
        lex(b"1 )"),
        Err(malformed_at(2, MalformedKind::UnexpectedDelimiter))
    );
    assert_eq!(
        lex(b"> "),
        Err(malformed_at(0, MalformedKind::UnexpectedDelimiter))
    );
    assert_eq!(
        lex(b"<<>"),
        Err(malformed_at(2, MalformedKind::UnexpectedDelimiter))
    );
}

#[test]
fn keywords_and_operators() {
    assert_eq!(
        lex(b"true false null obj endobj R stream T* ' \" d0").unwrap(),
        [
            Token::Keyword(b"true"),
            Token::Keyword(b"false"),
            Token::Keyword(b"null"),
            Token::Keyword(b"obj"),
            Token::Keyword(b"endobj"),
            Token::Keyword(b"R"),
            Token::Keyword(b"stream"),
            Token::Keyword(b"T*"),
            Token::Keyword(b"'"),
            Token::Keyword(b"\""),
            Token::Keyword(b"d0"),
        ]
    );
}

#[test]
fn indirect_object_with_offsets() {
    let input = b"1 0 obj\n<< /Type /Page /MediaBox [0 0 595.5 842] >>\nendobj";
    let mut budget = budget();
    let mut lexer = Lexer::new(input);
    let mut spans = Vec::new();
    while let Some(spanned) = lexer.next_token(&mut budget).unwrap() {
        spans.push((spanned.offset, spanned.token));
    }
    assert_eq!(
        spans,
        [
            (0, Token::Integer(1)),
            (2, Token::Integer(0)),
            (4, Token::Keyword(b"obj")),
            (8, Token::DictStart),
            (11, Token::Name(b"Type".to_vec())),
            (17, Token::Name(b"Page".to_vec())),
            (23, Token::Name(b"MediaBox".to_vec())),
            (33, Token::ArrayStart),
            (34, Token::Integer(0)),
            (36, Token::Integer(0)),
            (38, Token::Real(595.5)),
            (44, Token::Integer(842)),
            (47, Token::ArrayEnd),
            (49, Token::DictEnd),
            (52, Token::Keyword(b"endobj")),
        ]
    );
    assert_eq!(lexer.position(), input.len() as u64);
}

#[test]
fn seek_moves_within_bounds_only() {
    let mut budget = budget();
    let mut lexer = Lexer::new(b"1 0 obj");
    lexer.seek(4).unwrap();
    assert_eq!(
        lexer.next_token(&mut budget).unwrap().map(|s| s.token),
        Some(Token::Keyword(b"obj"))
    );
    lexer.seek(7).unwrap();
    assert_eq!(lexer.next_token(&mut budget).unwrap(), None);
    assert_eq!(lexer.seek(8), Err(Error::Truncated { offset: 8 }));
    assert_eq!(lexer.position(), 7);
}

#[test]
fn decoded_bytes_are_charged_before_allocation() {
    let refused = Error::LimitExceeded {
        kind: LimitKind::DecodedBytes,
        limit: 4,
    };
    // Exactly at the limit: accepted.
    let mut b = budget_with_decoded(4);
    assert!(Lexer::new(b"(abcd)").next_token(&mut b).is_ok());
    assert_eq!(b.decoded_bytes(), 4);
    // Limit + 1: refused, counter unchanged.
    let mut b = budget_with_decoded(4);
    assert_eq!(
        Lexer::new(b"(abcde)").next_token(&mut b),
        Err(refused.clone())
    );
    assert_eq!(b.decoded_bytes(), 0);
    // Cumulative across tokens.
    let mut b = budget_with_decoded(4);
    let mut lexer = Lexer::new(b"/ab <6364> /e");
    assert!(lexer.next_token(&mut b).is_ok());
    assert!(lexer.next_token(&mut b).is_ok());
    assert_eq!(lexer.next_token(&mut b), Err(refused));
    assert_eq!(b.decoded_bytes(), 4);
}

#[test]
fn cancellation_stops_the_lexer() {
    let token = CancellationToken::new();
    let mut b = Budget::new(ResourceLimits::default(), token.clone());
    let mut lexer = Lexer::new(b"1 2 3");
    assert!(lexer.next_token(&mut b).unwrap().is_some());
    token.cancel();
    assert_eq!(lexer.next_token(&mut b), Err(Error::Cancelled));
}

#[test]
fn errors_do_not_advance_the_cursor() {
    let mut b = budget();
    let mut lexer = Lexer::new(b"1 /A#4 2");
    assert_eq!(
        lexer.next_token(&mut b).unwrap().map(|s| s.token),
        Some(Token::Integer(1))
    );
    let error = malformed_at(4, MalformedKind::InvalidNameEscape);
    assert_eq!(lexer.next_token(&mut b), Err(error.clone()));
    assert_eq!(lexer.position(), 2);
    assert_eq!(lexer.next_token(&mut b), Err(error));
    // The caller decides where to resume.
    lexer.seek(7).unwrap();
    assert_eq!(
        lexer.next_token(&mut b).unwrap().map(|s| s.token),
        Some(Token::Integer(2))
    );
}

#[test]
fn more_escape_and_number_edge_cases() {
    // Octal code followed by an unescaped CRLF.
    assert_eq!(bytes(one(b"(\\1\r\n)")), b"\x01\n");
    // Backslash-LF continues the line; the following CR is an unescaped EOL.
    assert_eq!(bytes(one(b"(a\\\n\rb)")), b"a\nb");
    assert_eq!(one(b"+.5"), Token::Real(0.5));
    // Bytes outside 0x21..0x7E written unescaped in a name are accepted as is.
    assert_eq!(one(b"/A\xE9\x7F"), Token::Name(b"A\xE9\x7F".to_vec()));
}

#[test]
fn deep_parenthesis_nesting_is_linear_and_bounded() {
    let depth = 512 * 1024;
    let mut input = vec![b'('; depth];
    input.extend(std::iter::repeat_n(b')', depth));
    let decoded = bytes(one(&input));
    assert_eq!(decoded.len(), 2 * depth - 2);
    // Unbalanced: one closing parenthesis missing.
    assert_eq!(
        lex(input.get(..input.len() - 1).unwrap()),
        Err(Error::Truncated { offset: 0 })
    );
}

#[test]
fn character_classes_match_iso_32000_tables() {
    let whitespace: Vec<u8> = (0..=255).filter(|b| is_whitespace(*b)).collect();
    assert_eq!(whitespace, [0, 9, 10, 12, 13, 32]);
    let delimiters: Vec<u8> = (0..=255).filter(|b| is_delimiter(*b)).collect();
    assert_eq!(delimiters, b"%()/<>[]{}");
    assert_eq!((0..=255).filter(|b| is_regular(*b)).count(), 256 - 6 - 10);
}

// ---- Properties -------------------------------------------------------------

/// Serialises tokens the way a conforming writer would, for round trips.
fn write_token(token: &Token<'_>, out: &mut Vec<u8>) {
    match token {
        Token::Integer(v) => out.extend_from_slice(v.to_string().as_bytes()),
        Token::Real(v) => out.extend_from_slice(format!("{v:?}").as_bytes()),
        Token::LiteralString(bytes) => {
            out.push(b'(');
            for &b in bytes {
                match b {
                    b'(' | b')' | b'\\' => out.extend_from_slice(&[b'\\', b]),
                    b'\r' => out.extend_from_slice(b"\\r"),
                    _ => out.push(b),
                }
            }
            out.push(b')');
        }
        Token::HexString(bytes) => {
            out.push(b'<');
            for b in bytes {
                out.extend_from_slice(format!("{b:02X}").as_bytes());
            }
            out.push(b'>');
        }
        Token::Name(bytes) => {
            out.push(b'/');
            for &b in bytes {
                if (0x21..=0x7E).contains(&b) && is_regular(b) && b != b'#' {
                    out.push(b);
                } else {
                    out.extend_from_slice(format!("#{b:02X}").as_bytes());
                }
            }
        }
        Token::ArrayStart => out.push(b'['),
        Token::ArrayEnd => out.push(b']'),
        Token::DictStart => out.extend_from_slice(b"<<"),
        Token::DictEnd => out.extend_from_slice(b">>"),
        Token::BraceStart => out.push(b'{'),
        Token::BraceEnd => out.push(b'}'),
        Token::Keyword(k) => out.extend_from_slice(k),
    }
}

fn token_strategy() -> impl Strategy<Value = Token<'static>> {
    let keywords: [&'static [u8]; 7] = [b"true", b"false", b"null", b"obj", b"endobj", b"R", b"T*"];
    prop_oneof![
        any::<i64>().prop_map(Token::Integer),
        (-1_000_000_i64..1_000_000, 0_u32..5)
            .prop_map(|(m, scale)| Token::Real(m as f64 / 10_f64.powi(scale as i32))),
        proptest::collection::vec(any::<u8>(), 0..64).prop_map(Token::LiteralString),
        proptest::collection::vec(any::<u8>(), 0..64).prop_map(Token::HexString),
        proptest::collection::vec(1_u8..=255, 0..32).prop_map(Token::Name),
        Just(Token::ArrayStart),
        Just(Token::ArrayEnd),
        Just(Token::DictStart),
        Just(Token::DictEnd),
        Just(Token::BraceStart),
        Just(Token::BraceEnd),
        proptest::sample::select(keywords.to_vec()).prop_map(Token::Keyword),
    ]
}

/// Bytes biased towards PDF syntax, so that escapes, octal codes, end-of-line
/// pairs and nesting are exercised far more often than by uniform bytes.
fn pdf_like_bytes(max_len: usize) -> impl Strategy<Value = Vec<u8>> {
    let alphabet: Vec<u8> = b"()<>[]{}/%#\\ \r\n\t0123456789abcdefABCDEF.+-Robj\x00\xE9".to_vec();
    proptest::collection::vec(
        prop_oneof![
            4 => proptest::sample::select(alphabet),
            1 => any::<u8>(),
        ],
        0..max_len,
    )
}

/// Bytes decoded into a string or name token.
fn decoded_len(token: &Token<'_>) -> u64 {
    match token {
        Token::LiteralString(b) | Token::HexString(b) | Token::Name(b) => b.len() as u64,
        _ => 0,
    }
}

proptest! {
    /// Arbitrary input never panics, always terminates, and offsets only grow.
    #[test]
    fn arbitrary_bytes_are_handled(input in prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..2048),
        pdf_like_bytes(2048),
    ]) {
        let mut budget = budget();
        let mut lexer = Lexer::new(&input);
        let mut previous: Option<u64> = None;
        for _ in 0..=input.len() {
            match lexer.next_token(&mut budget) {
                Ok(Some(spanned)) => {
                    prop_assert!(spanned.offset < input.len() as u64);
                    prop_assert!(previous.is_none_or(|p| spanned.offset > p));
                    prop_assert!(lexer.position() > spanned.offset);
                    previous = Some(spanned.offset);
                }
                Ok(None) | Err(_) => return Ok(()),
            }
        }
        prop_assert!(false, "lexer did not finish within one token per byte");
    }

    /// Tokens written by a conforming writer read back unchanged.
    #[test]
    fn written_tokens_read_back(tokens in proptest::collection::vec(token_strategy(), 0..32)) {
        let mut text = Vec::new();
        for token in &tokens {
            write_token(token, &mut text);
            text.push(b' ');
        }
        prop_assert_eq!(lex(&text).unwrap(), tokens);
    }

    /// Every decoded string or name was charged at least its length beforehand,
    /// and a token is refused rather than emitted when the budget runs out.
    #[test]
    fn decoded_tokens_are_charged(input in pdf_like_bytes(1024), limit in 0_u64..256) {
        let mut budget = budget_with_decoded(limit);
        let mut lexer = Lexer::new(&input);
        for _ in 0..=input.len() {
            let before = budget.decoded_bytes();
            match lexer.next_token(&mut budget) {
                Ok(Some(spanned)) => {
                    prop_assert!(budget.decoded_bytes() - before >= decoded_len(&spanned.token));
                }
                Ok(None) | Err(_) => break,
            }
        }
    }
}
