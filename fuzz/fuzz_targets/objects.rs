//! Parses arbitrary bytes as direct objects, then as an indirect object at the
//! start and after each line feed. Properties: no panic, overflow or stack
//! exhaustion; returned objects respect the depth limit; stream data always lies
//! inside the input, followed by `endstream`.
#![no_main]

use libfuzzer_sys::fuzz_target;
use pdf_syntax::{
    Budget, CancellationToken, IndirectValue, Object, ObjectId, Parser, ResourceLimits,
};

const MAX_DEPTH: u32 = 32;

fn depth(object: &Object) -> u32 {
    match object {
        Object::Array(items) => 1 + items.iter().map(depth).max().unwrap_or(0),
        Object::Dictionary(dict) => 1 + dict.iter().map(|(_, v)| depth(v)).max().unwrap_or(0),
        _ => 0,
    }
}

fn budget() -> Budget {
    let mut limits = ResourceLimits::default();
    limits.max_decoded_bytes = 1 << 20;
    limits.max_objects_visited = 100_000;
    limits.max_nesting_depth = MAX_DEPTH;
    Budget::new(limits, CancellationToken::new())
}

fuzz_target!(|data: &[u8]| {
    let mut b = budget();
    let mut parser = Parser::new(data);
    while let Ok(object) = parser.parse_object(&mut b) {
        assert!(depth(&object) <= MAX_DEPTH);
    }

    // A resolver answering from the first byte, so indirect lengths vary too.
    let answer = i64::from(data.first().copied().unwrap_or(0));
    let mut resolver = |_: ObjectId, _: &mut Budget| Ok(answer);
    let starts = std::iter::once(0).chain(
        data.iter()
            .enumerate()
            .filter(|(_, b)| **b == b'\n')
            .map(|(i, _)| i as u64 + 1),
    );
    for start in starts.take(64) {
        // A fresh budget per start: one start must not starve the others.
        let mut b = budget();
        let mut parser = Parser::new(data);
        if parser.seek(start).is_err() {
            continue;
        }
        if let Ok(object) = parser.parse_indirect_object(&mut b, &mut resolver) {
            assert!(object.id.number > 0);
            if let IndirectValue::Stream(s) = object.value {
                assert!(s.data.start <= s.data.end);
                let tail = data.get(s.data.end as usize..).unwrap_or_default();
                let tail = tail.strip_prefix(b"\r\n").or_else(|| tail.strip_prefix(b"\n"))
                    .or_else(|| tail.strip_prefix(b"\r")).unwrap_or(tail);
                assert!(tail.starts_with(b"endstream"));
            }
        }
    }
});
