//! Lexes arbitrary bytes to the end or to the first error.
//! Properties: no panic or overflow (cargo-fuzz builds with overflow checks and
//! AddressSanitizer by default), offsets strictly increase and stay in bounds,
//! every decoded string or name was charged at least its length beforehand.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pdf_syntax::{Budget, CancellationToken, Lexer, ResourceLimits, Token};

fuzz_target!(|data: &[u8]| {
    let mut limits = ResourceLimits::default();
    limits.max_decoded_bytes = 1 << 20;
    let mut budget = Budget::new(limits, CancellationToken::new());
    let mut lexer = Lexer::new(data);
    let mut previous: Option<u64> = None;
    loop {
        let charged_before = budget.decoded_bytes();
        let Ok(Some(spanned)) = lexer.next_token(&mut budget) else {
            break;
        };
        assert!(previous.is_none_or(|p| spanned.offset > p));
        assert!(spanned.offset < lexer.position());
        assert!(lexer.position() <= data.len() as u64);
        if let Token::LiteralString(b) | Token::HexString(b) | Token::Name(b) = &spanned.token {
            assert!(budget.decoded_bytes() - charged_before >= b.len() as u64);
        }
        previous = Some(spanned.offset);
    }
});
