use miniz_oxide::deflate::compress_to_vec_zlib;
use proptest::prelude::*;

use super::*;
use crate::limits::{CancellationToken, LimitKind, ResourceLimits};

fn budget() -> Budget {
    Budget::new(ResourceLimits::default(), CancellationToken::new())
}

fn limited(max_decoded_bytes: u64) -> Budget {
    Budget::new(
        ResourceLimits {
            max_decoded_bytes,
            ..ResourceLimits::default()
        },
        CancellationToken::new(),
    )
}

fn name(n: &[u8]) -> Object {
    Object::Name(n.to_vec())
}

fn dict(entries: Vec<(&[u8], Object)>) -> Dictionary {
    let mut d = Dictionary::new();
    for (k, v) in entries {
        d.insert(k.to_vec(), v);
    }
    d
}

fn flate() -> Dictionary {
    dict(vec![(b"Filter", name(b"FlateDecode"))])
}

fn bad(kind: MalformedKind) -> Result<Vec<u8>, Error> {
    Err(Error::Malformed { offset: 100, kind })
}

#[test]
fn unfiltered_data_is_copied_and_charged() {
    let mut b = budget();
    assert_eq!(
        decode_stream(&Dictionary::new(), b"abc", 100, &mut b),
        Ok(b"abc".to_vec())
    );
    assert_eq!(b.decoded_bytes(), 3);
    let null = dict(vec![(b"Filter", Object::Null)]);
    assert_eq!(
        decode_stream(&null, b"x", 100, &mut budget()),
        Ok(b"x".to_vec())
    );
}

#[test]
fn flate_round_trip_and_filter_chains() {
    let text = b"BT /F1 24 Tf 72 770 Td (Hello) Tj ET".repeat(100);
    let once = compress_to_vec_zlib(&text, 6);
    assert_eq!(
        decode_stream(&flate(), &once, 100, &mut budget()),
        Ok(text.clone())
    );
    // Two filters, applied in order.
    let twice = compress_to_vec_zlib(&once, 9);
    let chain = dict(vec![(
        b"Filter",
        Object::Array(vec![name(b"FlateDecode"), name(b"FlateDecode")]),
    )]);
    assert_eq!(
        decode_stream(&chain, &twice, 100, &mut budget()),
        Ok(text.clone())
    );
    // Bytes after the end of the zlib stream (an EOL counted in /Length) are ignored.
    let trailing = [once.as_slice(), b"\r\n"].concat();
    assert_eq!(
        decode_stream(&flate(), &trailing, 100, &mut budget()),
        Ok(text)
    );
    // An empty stream decodes to nothing.
    let empty = compress_to_vec_zlib(b"", 6);
    assert_eq!(
        decode_stream(&flate(), &empty, 100, &mut budget()),
        Ok(Vec::new())
    );
}

#[test]
fn decompression_bomb_stops_at_the_limit() {
    // 8 MiB of zeros compress to a few kilobytes.
    let bomb = compress_to_vec_zlib(&vec![0_u8; 8 << 20], 9);
    assert!(bomb.len() < 64 << 10);
    let exact = (8 << 20) as u64;
    let mut b = limited(exact);
    assert_eq!(
        decode_stream(&flate(), &bomb, 100, &mut b).map(|d| d.len()),
        Ok(8 << 20)
    );
    assert_eq!(b.decoded_bytes(), exact);
    let mut b = limited(exact - 1);
    assert_eq!(
        decode_stream(&flate(), &bomb, 100, &mut b),
        Err(Error::LimitExceeded {
            kind: LimitKind::DecodedBytes,
            limit: exact - 1
        })
    );
    // The refused window was not kept: at most one window short of the limit.
    assert!(b.decoded_bytes() < exact);
    assert!(b.decoded_bytes() + WINDOW as u64 >= exact - 1);
}

#[test]
fn corrupt_and_truncated_flate_data() {
    let data = compress_to_vec_zlib(&b"0123456789".repeat(1000), 6);
    // Cut before the end: truncated.
    let cut = data.get(..data.len() / 2).expect("prefix");
    assert_eq!(
        decode_stream(&flate(), cut, 100, &mut budget()),
        Err(Error::Truncated { offset: 100 })
    );
    assert_eq!(
        decode_stream(&flate(), b"", 100, &mut budget()),
        Err(Error::Truncated { offset: 100 })
    );
    // A wrong Adler-32 checksum, and a non-zlib header.
    let mut wrong_sum = data.clone();
    if let Some(last) = wrong_sum.last_mut() {
        *last ^= 0xFF;
    }
    assert_eq!(
        decode_stream(&flate(), &wrong_sum, 100, &mut budget()),
        bad(MalformedKind::InvalidFlateData)
    );
    assert_eq!(
        decode_stream(&flate(), b"not zlib at all", 100, &mut budget()),
        bad(MalformedKind::InvalidFlateData)
    );
}

#[test]
fn filter_entries_are_validated() {
    let data = compress_to_vec_zlib(b"x", 6);
    for filter in [&b"LZWDecode"[..], b"DCTDecode", b"ASCIIHexDecode", b"Fl"] {
        assert_eq!(
            decode_stream(
                &dict(vec![(b"Filter", name(filter))]),
                &data,
                100,
                &mut budget()
            ),
            Err(Error::Unsupported(Capability::StreamFilter)),
            "{filter:?}"
        );
    }
    let invalid = |d: Dictionary| decode_stream(&d, &data, 100, &mut budget());
    assert_eq!(
        invalid(dict(vec![(b"Filter", Object::Integer(5))])),
        bad(MalformedKind::InvalidFilter)
    );
    assert_eq!(
        invalid(dict(vec![(
            b"Filter",
            Object::Array(vec![Object::Integer(1)])
        )])),
        bad(MalformedKind::InvalidFilter)
    );
    // /DecodeParms must match /Filter.
    assert_eq!(
        invalid(dict(vec![
            (b"Filter", Object::Array(vec![name(b"FlateDecode")])),
            (
                b"DecodeParms",
                Object::Array(vec![Object::Null, Object::Null])
            ),
        ])),
        bad(MalformedKind::InvalidFilter)
    );
    assert_eq!(
        invalid(dict(vec![
            (b"Filter", name(b"FlateDecode")),
            (b"DecodeParms", Object::Integer(1)),
        ])),
        bad(MalformedKind::InvalidFilter)
    );
}

/// Applies PNG filter `kind` to `rows` (the encoder side, for tests).
fn png_encode(rows: &[Vec<u8>], kind: u8, pixel_len: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut previous = vec![0_u8; rows.first().map_or(0, Vec::len)];
    for row in rows {
        out.push(kind);
        for (i, &x) in row.iter().enumerate() {
            let left = i.checked_sub(pixel_len).map_or(0, |j| row[j]);
            let up = previous[i];
            let up_left = i.checked_sub(pixel_len).map_or(0, |j| previous[j]);
            let predicted = match kind {
                0 => 0,
                1 => left,
                2 => up,
                3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                _ => paeth(left, up, up_left),
            };
            out.push(x.wrapping_sub(predicted));
        }
        previous = row.clone();
    }
    out
}

fn predicted(params: Vec<(&[u8], Object)>) -> Dictionary {
    dict(vec![
        (b"Filter", name(b"FlateDecode")),
        (b"DecodeParms", Object::Dictionary(dict(params))),
    ])
}

#[test]
fn png_predictors_round_trip() {
    // 3 rows of 4 RGB pixels (12 bytes per row).
    let rows: Vec<Vec<u8>> = (0..3_u8)
        .map(|r| {
            (0..12_u8)
                .map(|c| r.wrapping_mul(37).wrapping_add(c * 11))
                .collect()
        })
        .collect();
    let expected = rows.concat();
    let params = predicted(vec![
        (b"Predictor", Object::Integer(15)),
        (b"Colors", Object::Integer(3)),
        (b"Columns", Object::Integer(4)),
    ]);
    for kind in 0..=4 {
        let encoded = compress_to_vec_zlib(&png_encode(&rows, kind, 3), 6);
        assert_eq!(
            decode_stream(&params, &encoded, 100, &mut budget()),
            Ok(expected.clone()),
            "PNG filter {kind}"
        );
    }
    // Predictor 1 means no prediction.
    let none = predicted(vec![(b"Predictor", Object::Integer(1))]);
    let raw = compress_to_vec_zlib(&expected, 6);
    assert_eq!(decode_stream(&none, &raw, 100, &mut budget()), Ok(expected));
}

#[test]
fn tiff_predictor() {
    // Two RGB pixels per row: each byte adds the same component to its left.
    let encoded = compress_to_vec_zlib(&[10, 20, 30, 1, 2, 3], 6);
    let params = predicted(vec![
        (b"Predictor", Object::Integer(2)),
        (b"Colors", Object::Integer(3)),
        (b"Columns", Object::Integer(2)),
    ]);
    assert_eq!(
        decode_stream(&params, &encoded, 100, &mut budget()),
        Ok(vec![10, 20, 30, 11, 22, 33])
    );
    let sixteen = predicted(vec![
        (b"Predictor", Object::Integer(2)),
        (b"BitsPerComponent", Object::Integer(16)),
    ]);
    assert_eq!(
        decode_stream(&sixteen, &encoded, 100, &mut budget()),
        Err(Error::Unsupported(Capability::StreamFilter))
    );
}

#[test]
fn invalid_predictor_parameters_and_data() {
    let data = compress_to_vec_zlib(&[0, 1, 2, 3], 6);
    let with = |params| decode_stream(&predicted(params), &data, 100, &mut budget());
    let invalid = bad(MalformedKind::InvalidPredictor);
    assert_eq!(with(vec![(b"Predictor", Object::Integer(3))]), invalid);
    assert_eq!(with(vec![(b"Predictor", Object::Real(12.0))]), invalid);
    let png =
        |extra: (&'static [u8], Object)| with(vec![(b"Predictor", Object::Integer(12)), extra]);
    assert_eq!(png((b"Colors", Object::Integer(0))), invalid);
    assert_eq!(png((b"BitsPerComponent", Object::Integer(3))), invalid);
    assert_eq!(png((b"Columns", Object::Integer(-1))), invalid);
    assert_eq!(png((b"Columns", Object::Integer(i64::MAX))), invalid);
    // 4 bytes with Columns 2: rows of 1 + 2 bytes leave a partial row.
    assert_eq!(png((b"Columns", Object::Integer(2))), invalid);
    // Columns 3: one row, filter type 0 then 3 samples.
    assert_eq!(png((b"Columns", Object::Integer(3))), Ok(vec![1, 2, 3]));
    // An unknown PNG filter type in the row.
    let unknown = compress_to_vec_zlib(&[5, 1, 2, 3], 6);
    assert_eq!(
        decode_stream(
            &predicted(vec![
                (b"Predictor", Object::Integer(12)),
                (b"Columns", Object::Integer(3)),
            ]),
            &unknown,
            100,
            &mut budget()
        ),
        invalid
    );
    // Huge Columns with no data: nothing is allocated, the result is empty.
    let empty = compress_to_vec_zlib(b"", 6);
    assert_eq!(
        decode_stream(
            &predicted(vec![
                (b"Predictor", Object::Integer(12)),
                (b"Columns", Object::Integer(1 << 40)),
            ]),
            &empty,
            100,
            &mut budget()
        ),
        Ok(Vec::new())
    );
}

#[test]
fn cancellation_stops_decoding() {
    let token = CancellationToken::new();
    token.cancel();
    let mut b = Budget::new(ResourceLimits::default(), token);
    let data = compress_to_vec_zlib(b"x", 6);
    assert_eq!(
        decode_stream(&flate(), &data, 100, &mut b),
        Err(Error::Cancelled)
    );
}

proptest! {
    #[test]
    fn arbitrary_flate_input_is_handled(input in prop::collection::vec(any::<u8>(), 0..512)) {
        let mut b = limited(1 << 16);
        let _ = decode_stream(&flate(), &input, 0, &mut b);
        prop_assert!(b.decoded_bytes() <= 1 << 16);
    }

    #[test]
    fn png_round_trip(
        rows in prop::collection::vec(prop::collection::vec(any::<u8>(), 6), 1..8),
        kind in 0_u8..=4,
    ) {
        let params = predicted(vec![
            (b"Predictor", Object::Integer(15)),
            (b"Colors", Object::Integer(2)),
            (b"Columns", Object::Integer(3)),
        ]);
        let encoded = compress_to_vec_zlib(&png_encode(&rows, kind, 2), 6);
        prop_assert_eq!(decode_stream(&params, &encoded, 0, &mut budget()), Ok(rows.concat()));
    }
}

#[test]
fn everything_is_validated_before_decoding() {
    // A bomb followed by an unsupported filter, or with an invalid predictor:
    // refused before a single byte is inflated or charged.
    let bomb = compress_to_vec_zlib(&vec![0_u8; 4 << 20], 9);
    let chain = dict(vec![(
        b"Filter",
        Object::Array(vec![name(b"FlateDecode"), name(b"LZWDecode")]),
    )]);
    let mut b = budget();
    assert_eq!(
        decode_stream(&chain, &bomb, 100, &mut b),
        Err(Error::Unsupported(Capability::StreamFilter))
    );
    assert_eq!(b.decoded_bytes(), 0);
    let mut b = budget();
    assert_eq!(
        decode_stream(
            &predicted(vec![(b"Predictor", Object::Integer(3))]),
            &bomb,
            100,
            &mut b
        ),
        bad(MalformedKind::InvalidPredictor)
    );
    assert_eq!(b.decoded_bytes(), 0);
}

#[test]
fn predictors_are_charged_once_at_the_exact_limit() {
    // 64 rows of 1 filter byte + 4 samples: 320 inflated bytes, 256 decoded.
    let rows: Vec<Vec<u8>> = (0..64_u8).map(|r| vec![r, r, r, r]).collect();
    let encoded = compress_to_vec_zlib(&png_encode(&rows, 2, 1), 6);
    let params = predicted(vec![
        (b"Predictor", Object::Integer(12)),
        (b"Columns", Object::Integer(4)),
    ]);
    let mut b = limited(320);
    assert_eq!(
        decode_stream(&params, &encoded, 100, &mut b),
        Ok(rows.concat())
    );
    assert_eq!(b.decoded_bytes(), 320);
    assert_eq!(
        decode_stream(&params, &encoded, 100, &mut limited(319)),
        Err(Error::LimitExceeded {
            kind: LimitKind::DecodedBytes,
            limit: 319
        })
    );
}

#[test]
fn png_predictors_with_other_component_sizes() {
    // (Colors, BitsPerComponent, Columns) -> bytes per row and per pixel.
    for (colors, bits, columns, row_len, pixel_len) in [
        (1, 1, 5, 1, 1), // 5 bits round up to one byte
        (1, 2, 9, 3, 1), // 18 bits
        (3, 4, 3, 5, 2), // 36 bits per row, 12 per pixel
        (3, 16, 2, 12, 6),
    ] {
        let rows: Vec<Vec<u8>> = (0..4_u8)
            .map(|r| (0..row_len as u8).map(|c| r * 31 + c * 7).collect())
            .collect();
        let params = predicted(vec![
            (b"Predictor", Object::Integer(15)),
            (b"Colors", Object::Integer(colors)),
            (b"BitsPerComponent", Object::Integer(bits)),
            (b"Columns", Object::Integer(columns)),
        ]);
        for kind in 0..=4 {
            let encoded = compress_to_vec_zlib(&png_encode(&rows, kind, pixel_len), 6);
            assert_eq!(
                decode_stream(&params, &encoded, 100, &mut budget()),
                Ok(rows.concat()),
                "colors {colors}, bits {bits}, columns {columns}, filter {kind}"
            );
        }
    }
}
