//! Decodes arbitrary bytes as a FlateDecode stream, with predictor parameters
//! (predictor, colors, bits per component, columns) taken from the first four
//! bytes. Properties: no panic or overflow; the decoded length never exceeds
//! what was charged, and the charge never exceeds the limit (a decompression
//! bomb stops at max_decoded_bytes).
#![no_main]

use libfuzzer_sys::fuzz_target;
use pdf_syntax::{Budget, CancellationToken, Dictionary, Object, ResourceLimits, decode_stream};

const LIMIT: u64 = 1 << 20;

fuzz_target!(|data: &[u8]| {
    let (head, body) = data.split_at(data.len().min(4));
    let mut dictionary = Dictionary::new();
    dictionary.insert(b"Filter".to_vec(), Object::Name(b"FlateDecode".to_vec()));
    if let [predictor, colors, bits, columns] = head {
        let mut params = Dictionary::new();
        let predictor = match predictor % 4 {
            0 => 1,
            1 => 2,
            2 => 10 + i64::from(predictor % 6),
            _ => i64::from(*predictor),
        };
        params.insert(b"Predictor".to_vec(), Object::Integer(predictor));
        params.insert(b"Colors".to_vec(), Object::Integer(i64::from(colors % 4) + 1));
        let bits = [1, 2, 4, 8, 16, 3][usize::from(bits % 6)];
        params.insert(b"BitsPerComponent".to_vec(), Object::Integer(bits));
        params.insert(b"Columns".to_vec(), Object::Integer(i64::from(*columns)));
        dictionary.insert(b"DecodeParms".to_vec(), Object::Dictionary(params));
    }
    let mut limits = ResourceLimits::default();
    limits.max_decoded_bytes = LIMIT;
    let mut budget = Budget::new(limits, CancellationToken::new());
    if let Ok(decoded) = decode_stream(&dictionary, body, 0, &mut budget) {
        assert!(decoded.len() as u64 <= budget.decoded_bytes());
    }
    assert!(budget.decoded_bytes() <= LIMIT);
});
