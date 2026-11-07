//! Stream filters (ISO 32000-2:2020, 7.4): `FlateDecode` with its predictors.
//!
//! Every filter and parameter is validated before any byte is decoded.
//! Decoding is incremental: inflate writes into a fixed 64 KiB window, and each
//! window is charged to `max_decoded_bytes` before it is kept, so a
//! decompression bomb stops at the limit. The output buffer never grows beyond
//! what the budget still allows; a reallocation briefly holds the old and new
//! buffers, so the transient peak is about 1.5 × the decoded size. Predictors
//! are undone in place, without a second buffer or a second charge. In a chain
//! of filters, each stage's output is charged, since both buffers are alive.
//! Other filters are reported as unsupported, never passed through undecoded.

use std::io;

use miniz_oxide::inflate::stream::{InflateState, inflate};
use miniz_oxide::{DataFormat, MZError, MZFlush, MZStatus};

use crate::limits::{Budget, Capability, Error, MalformedKind};
use crate::object::{Dictionary, Object};

/// Size of the inflate output window; also the cancellation granularity.
const WINDOW: usize = 64 * 1024;
/// Cancellation is checked every this many predicted rows.
const CANCEL_CHECK_ROWS: usize = 1024;

/// Decodes the data of a stream whose `dictionary` is given, applying every
/// filter of `/Filter` in order with its `/DecodeParms`.
///
/// `/Filter` and `/DecodeParms` must be direct objects here; `PdfFile`
/// resolves references first. `data_offset` (where the encoded data starts in
/// the file) locates errors. Every decoded byte is charged to `budget`; bytes
/// after the end of a Flate stream are ignored.
pub fn decode_stream(
    dictionary: &Dictionary,
    encoded: &[u8],
    data_offset: u64,
    budget: &mut Budget,
) -> Result<Vec<u8>, Error> {
    let stages = plan(dictionary, data_offset)?;
    if stages.is_empty() {
        return copy(encoded, budget);
    }
    let mut data: Option<Vec<u8>> = None;
    for predictor in stages {
        let input = data.as_deref().unwrap_or(encoded);
        let mut decoded = inflate_zlib(input, data_offset, budget)?;
        if let Some(predictor) = predictor {
            predictor.apply_in_place(&mut decoded, data_offset, budget)?;
        }
        data = Some(decoded);
    }
    data.ok_or(malformed(data_offset, MalformedKind::InvalidFilter))
}

/// Validates `/Filter` and `/DecodeParms` before decoding: one entry per
/// FlateDecode stage, with its predictor if any.
fn plan(dictionary: &Dictionary, offset: u64) -> Result<Vec<Option<Predictor>>, Error> {
    let invalid = || malformed(offset, MalformedKind::InvalidFilter);
    let filters: Vec<&[u8]> = match dictionary.get(b"Filter") {
        None | Some(Object::Null) => Vec::new(),
        Some(Object::Name(name)) => vec![name.as_slice()],
        Some(Object::Array(items)) => items
            .iter()
            .map(|item| item.as_name().ok_or_else(invalid))
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(invalid()),
    };
    // Without a filter, /DecodeParms has nothing to parameterise.
    if filters.is_empty() {
        return Ok(Vec::new());
    }
    let params: Vec<Option<&Dictionary>> = match dictionary.get(b"DecodeParms") {
        None | Some(Object::Null) => vec![None; filters.len()],
        Some(Object::Dictionary(params)) if filters.len() == 1 => vec![Some(params)],
        Some(Object::Array(items)) if items.len() == filters.len() => items
            .iter()
            .map(|item| match item {
                Object::Null => Ok(None),
                Object::Dictionary(params) => Ok(Some(params)),
                _ => Err(invalid()),
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(invalid()),
    };
    filters
        .into_iter()
        .zip(params)
        .map(|(filter, params)| match filter {
            b"FlateDecode" => Predictor::from_params(params, offset),
            _ => Err(Error::Unsupported(Capability::StreamFilter)),
        })
        .collect()
}

fn malformed(offset: u64, kind: MalformedKind) -> Error {
    Error::Malformed { offset, kind }
}

fn out_of_memory() -> Error {
    Error::Io(io::ErrorKind::OutOfMemory)
}

/// Charges, then copies, unfiltered data.
fn copy(data: &[u8], budget: &mut Budget) -> Result<Vec<u8>, Error> {
    budget.charge_decoded_bytes(data.len() as u64)?;
    let mut out = Vec::new();
    out.try_reserve_exact(data.len())
        .map_err(|_| out_of_memory())?;
    out.extend_from_slice(data);
    Ok(out)
}

/// Inflates zlib data (RFC 1950, checksum verified) under the budget.
fn inflate_zlib(input: &[u8], offset: u64, budget: &mut Budget) -> Result<Vec<u8>, Error> {
    let bad = || malformed(offset, MalformedKind::InvalidFlateData);
    let mut state = InflateState::new_boxed(DataFormat::Zlib);
    let mut window = vec![0_u8; WINDOW];
    let mut out: Vec<u8> = Vec::new();
    let mut rest = input;
    loop {
        budget.check_cancelled()?;
        let result = inflate(&mut state, rest, &mut window, MZFlush::None);
        rest = rest.get(result.bytes_consumed..).unwrap_or_default();
        let written = window.get(..result.bytes_written).ok_or_else(bad)?;
        // Charged before it is kept: the limit stops a decompression bomb.
        budget.charge_decoded_bytes(written.len() as u64)?;
        reserve_within_budget(&mut out, written.len(), budget)?;
        out.extend_from_slice(written);
        match result.status {
            Ok(MZStatus::StreamEnd) => return Ok(out),
            Ok(MZStatus::Ok) if result.bytes_consumed > 0 || result.bytes_written > 0 => {}
            // No progress: the input ended before the end of the stream.
            Ok(MZStatus::Ok) | Err(MZError::Buf) if rest.is_empty() => {
                return Err(Error::Truncated { offset });
            }
            _ => return Err(bad()),
        }
    }
}

/// Makes room for `additional` bytes already charged: amortised doubling, but
/// never beyond the length the remaining budget still allows.
fn reserve_within_budget(
    out: &mut Vec<u8>,
    additional: usize,
    budget: &Budget,
) -> Result<(), Error> {
    let needed = out.len().saturating_add(additional);
    if out.capacity() >= needed {
        return Ok(());
    }
    let remaining = budget
        .limits()
        .max_decoded_bytes
        .saturating_sub(budget.decoded_bytes());
    let ceiling = needed.saturating_add(usize::try_from(remaining).unwrap_or(usize::MAX));
    let target = out.capacity().saturating_mul(2).max(needed).min(ceiling);
    out.try_reserve_exact(target - out.len())
        .map_err(|_| out_of_memory())
}

/// `/Predictor` and its parameters (ISO 32000-2, table 8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Predictor {
    /// 2 for TIFF, 10 to 15 for PNG (the actual filter is chosen per row).
    kind: u8,
    /// Bytes per row of decoded samples.
    row_len: usize,
    /// Bytes per complete pixel, at least 1.
    pixel_len: usize,
}

impl Predictor {
    fn from_params(params: Option<&Dictionary>, offset: u64) -> Result<Option<Self>, Error> {
        let invalid = || malformed(offset, MalformedKind::InvalidPredictor);
        let Some(params) = params else {
            return Ok(None);
        };
        let integer = |key: &[u8], default: i64| match params.get(key) {
            None | Some(Object::Null) => Ok(default),
            Some(Object::Integer(value)) => Ok(*value),
            Some(_) => Err(invalid()),
        };
        let kind = match integer(b"Predictor", 1)? {
            1 => return Ok(None),
            kind @ (2 | 10..=15) => kind as u8,
            _ => return Err(invalid()),
        };
        let colors = integer(b"Colors", 1)?;
        let bits = integer(b"BitsPerComponent", 8)?;
        let columns = integer(b"Columns", 1)?;
        let bits_per_component: u64 = match bits {
            1 | 2 | 4 | 8 | 16 => bits as u64,
            _ => return Err(invalid()),
        };
        let colors = u64::try_from(colors)
            .ok()
            .filter(|c| *c >= 1)
            .ok_or_else(invalid)?;
        let columns = u64::try_from(columns)
            .ok()
            .filter(|c| *c >= 1)
            .ok_or_else(invalid)?;
        let pixel_bits = colors.checked_mul(bits_per_component).ok_or_else(invalid)?;
        let row_bits = pixel_bits.checked_mul(columns).ok_or_else(invalid)?;
        let row_len = usize::try_from(row_bits.div_ceil(8)).map_err(|_| invalid())?;
        let pixel_len = usize::try_from(pixel_bits.div_ceil(8))
            .map_err(|_| invalid())?
            .max(1);
        // TIFF prediction is implemented for 8-bit components only.
        if kind == 2 && bits_per_component != 8 {
            return Err(Error::Unsupported(Capability::StreamFilter));
        }
        Ok(Some(Self {
            kind,
            row_len,
            pixel_len,
        }))
    }

    /// Undoes the prediction in `data`, in place. PNG rows shrink by their
    /// filter-type byte: row `r` is written at `r * row_len`, always behind the
    /// bytes still to read (at `r * (row_len + 1) + 1 + k`), and the previous
    /// decoded row is still intact when the current one reads it.
    fn apply_in_place(
        &self,
        data: &mut Vec<u8>,
        offset: u64,
        budget: &mut Budget,
    ) -> Result<(), Error> {
        let invalid = || malformed(offset, MalformedKind::InvalidPredictor);
        let png = self.kind >= 10;
        let stride = if png {
            self.row_len.checked_add(1).ok_or_else(invalid)?
        } else {
            self.row_len
        };
        if stride == 0 || !data.len().is_multiple_of(stride) {
            return Err(invalid());
        }
        let rows = data.len() / stride;
        let (row_len, pixel_len) = (self.row_len, self.pixel_len);
        let at = |data: &Vec<u8>, i: usize| data.get(i).copied().ok_or_else(invalid);
        for row in 0..rows {
            if row % CANCEL_CHECK_ROWS == 0 {
                budget.check_cancelled()?;
            }
            let out_start = row * row_len;
            let filter = if png { at(data, row * stride)? } else { 0 };
            for k in 0..row_len {
                let w = out_start + k;
                let x = if png {
                    at(data, row * stride + 1 + k)?
                } else {
                    at(data, w)?
                };
                let left = if k >= pixel_len {
                    at(data, w - pixel_len)?
                } else {
                    0
                };
                let predicted = if !png {
                    left
                } else {
                    let up = if row > 0 { at(data, w - row_len)? } else { 0 };
                    let up_left = if row > 0 && k >= pixel_len {
                        at(data, w - row_len - pixel_len)?
                    } else {
                        0
                    };
                    match filter {
                        0 => 0,
                        1 => left,
                        2 => up,
                        3 => ((u16::from(left) + u16::from(up)) / 2) as u8,
                        4 => paeth(left, up, up_left),
                        _ => return Err(invalid()),
                    }
                };
                *data.get_mut(w).ok_or_else(invalid)? = x.wrapping_add(predicted);
            }
        }
        data.truncate(rows * row_len);
        Ok(())
    }
}

fn paeth(a: u8, b: u8, c: u8) -> u8 {
    let p = i16::from(a) + i16::from(b) - i16::from(c);
    let (pa, pb, pc) = (
        (p - i16::from(a)).abs(),
        (p - i16::from(b)).abs(),
        (p - i16::from(c)).abs(),
    );
    if pa <= pb && pa <= pc {
        a
    } else if pb <= pc {
        b
    } else {
        c
    }
}

#[cfg(test)]
mod tests;
