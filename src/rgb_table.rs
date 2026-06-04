use std::io::Read;

use anyhow::{Result, bail};
use flate2::read::ZlibDecoder;

use crate::model::RgbTable;

const BTT_RGB_TABLE: u32 = 1;
const RGB_TABLE_VERSION: u32 = 1;

pub fn decode_rgb_table(encoded: &str) -> Result<Vec<u8>> {
    let compressed = adobe_base85_decode(encoded);
    if compressed.len() < 5 {
        bail!("decoded payload too short");
    }

    let expected_len = u32::from_le_bytes(compressed[0..4].try_into().unwrap()) as usize;
    let mut decoder = ZlibDecoder::new(&compressed[4..]);
    let mut decoded = Vec::with_capacity(expected_len);
    decoder.read_to_end(&mut decoded)?;

    if decoded.len() != expected_len {
        bail!(
            "zlib length mismatch: expected {expected_len}, got {}",
            decoded.len()
        );
    }

    Ok(decoded)
}

pub fn parse_rgb_table(bytes: &[u8]) -> Result<RgbTable> {
    let mut r = LeReader::new(bytes);

    let table_type = r.u32()?;
    if table_type != BTT_RGB_TABLE {
        bail!("not an RGB table: type {table_type}");
    }

    let version = r.u32()?;
    if version != RGB_TABLE_VERSION {
        bail!("unsupported RGB table version {version}");
    }

    let dimensions = r.u32()?;
    let divisions = r.u32()?;
    if dimensions != 1 && dimensions != 3 {
        bail!("unsupported RGB table dimensions {dimensions}");
    }
    if divisions < 2 {
        bail!("invalid division count {divisions}");
    }

    let nop: Vec<u16> = (0..divisions)
        .map(|i| ((i * 0x0ffff + (divisions >> 1)) / (divisions - 1)) as u16)
        .collect();

    let sample_count = if dimensions == 1 {
        divisions as usize
    } else {
        (divisions as usize).pow(3)
    };
    let mut samples = Vec::with_capacity(sample_count);

    if dimensions == 1 {
        for i in 0..divisions as usize {
            let rr = r.u16()?.wrapping_add(nop[i]);
            let gg = r.u16()?.wrapping_add(nop[i]);
            let bb = r.u16()?.wrapping_add(nop[i]);
            samples.push([rr, gg, bb]);
        }
    } else {
        for ri in 0..divisions as usize {
            for gi in 0..divisions as usize {
                for bi in 0..divisions as usize {
                    let rr = r.u16()?.wrapping_add(nop[ri]);
                    let gg = r.u16()?.wrapping_add(nop[gi]);
                    let bb = r.u16()?.wrapping_add(nop[bi]);
                    samples.push([rr, gg, bb]);
                }
            }
        }
    }

    let primaries = r.u32()?;
    let gamma = r.u32()?;
    let gamut = r.u32()?;
    let min_amount = r.f64()?;
    let max_amount = r.f64()?;
    let flags = if r.remaining() >= 4 {
        Some(r.u32()?)
    } else {
        None
    };

    Ok(RgbTable {
        dimensions,
        divisions,
        samples,
        primaries,
        gamma,
        gamut,
        min_amount,
        max_amount,
        flags,
    })
}

fn adobe_base85_decode(encoded: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity((encoded.len() + 4) / 5 * 4);
    let mut phase = 0u32;
    let mut value = 0u32;

    for byte in encoded.bytes() {
        if !(32..=127).contains(&byte) {
            continue;
        }
        let digit = match adobe_digit(byte) {
            Some(d) => d as u32,
            None => continue,
        };

        phase += 1;
        match phase {
            1 => value = digit,
            2 => value += digit * 85,
            3 => value += digit * 85 * 85,
            4 => value += digit * 85 * 85 * 85,
            5 => {
                value += digit * 85 * 85 * 85 * 85;
                out.extend_from_slice(&value.to_le_bytes());
                phase = 0;
            }
            _ => unreachable!(),
        }
    }

    if phase > 1 {
        let bytes = value.to_le_bytes();
        let count = match phase {
            2 => 1,
            3 => 2,
            4 => 3,
            _ => 0,
        };
        out.extend_from_slice(&bytes[..count]);
    }

    out
}

fn adobe_digit(byte: u8) -> Option<u8> {
    let value = match byte {
        b'0'..=b'9' => byte - b'0',
        b'a'..=b'z' => 10 + byte - b'a',
        b'A'..=b'Z' => 36 + byte - b'A',
        b'.' => 62,
        b'-' => 63,
        b':' => 64,
        b'+' => 65,
        b'=' => 66,
        b'^' => 67,
        b'!' => 68,
        b'/' => 69,
        b'*' => 70,
        b'?' => 71,
        b'`' => 72,
        b'\'' => 73,
        b'|' => 74,
        b'(' => 75,
        b')' => 76,
        b'[' => 77,
        b']' => 78,
        b'{' => 79,
        b'}' => 80,
        b'@' => 81,
        b'%' => 82,
        b'$' => 83,
        b'#' => 84,
        _ => return None,
    };
    Some(value)
}

pub(crate) fn sample_table(table: &RgbTable, r: u32, g: u32, b: u32, axis: u32) -> [u16; 3] {
    if table.dimensions == 1 {
        return [
            sample_1d(table, r, axis, 0),
            sample_1d(table, g, axis, 1),
            sample_1d(table, b, axis, 2),
        ];
    }

    let d = table.divisions as usize;
    let rf = scaled_pos(r, axis, table.divisions);
    let gf = scaled_pos(g, axis, table.divisions);
    let bf = scaled_pos(b, axis, table.divisions);

    let (r0, r1, rt) = split_pos(rf, d);
    let (g0, g1, gt) = split_pos(gf, d);
    let (b0, b1, bt) = split_pos(bf, d);

    let mut out = [0u16; 3];
    for c in 0..3 {
        let c000 = table.samples[idx3(d, r0, g0, b0)][c] as f64;
        let c100 = table.samples[idx3(d, r1, g0, b0)][c] as f64;
        let c010 = table.samples[idx3(d, r0, g1, b0)][c] as f64;
        let c110 = table.samples[idx3(d, r1, g1, b0)][c] as f64;
        let c001 = table.samples[idx3(d, r0, g0, b1)][c] as f64;
        let c101 = table.samples[idx3(d, r1, g0, b1)][c] as f64;
        let c011 = table.samples[idx3(d, r0, g1, b1)][c] as f64;
        let c111 = table.samples[idx3(d, r1, g1, b1)][c] as f64;

        let c00 = lerp(c000, c100, rt);
        let c10 = lerp(c010, c110, rt);
        let c01 = lerp(c001, c101, rt);
        let c11 = lerp(c011, c111, rt);
        let c0 = lerp(c00, c10, gt);
        let c1 = lerp(c01, c11, gt);
        out[c] = lerp(c0, c1, bt).round().clamp(0.0, 65535.0) as u16;
    }

    out
}

fn sample_1d(table: &RgbTable, input: u32, axis: u32, channel: usize) -> u16 {
    let d = table.divisions as usize;
    let pos = scaled_pos(input, axis, table.divisions);
    let (i0, i1, t) = split_pos(pos, d);
    lerp(
        table.samples[i0][channel] as f64,
        table.samples[i1][channel] as f64,
        t,
    )
    .round()
    .clamp(0.0, 65535.0) as u16
}

fn scaled_pos(input: u32, axis: u32, divisions: u32) -> f64 {
    if axis <= 1 {
        return 0.0;
    }
    input as f64 * (divisions - 1) as f64 / (axis - 1) as f64
}

fn split_pos(pos: f64, divisions: usize) -> (usize, usize, f64) {
    let lo = pos.floor().clamp(0.0, (divisions - 1) as f64) as usize;
    let hi = (lo + 1).min(divisions - 1);
    (lo, hi, pos - lo as f64)
}

fn idx3(divisions: usize, r: usize, g: usize, b: usize) -> usize {
    (r * divisions + g) * divisions + b
}

fn lerp(a: f64, b: f64, t: f64) -> f64 {
    a + (b - a) * t
}

struct LeReader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> LeReader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.pos)
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N]> {
        if self.remaining() < N {
            bail!("truncated RGB table at byte {}", self.pos);
        }
        let mut out = [0u8; N];
        out.copy_from_slice(&self.bytes[self.pos..self.pos + N]);
        self.pos += N;
        Ok(out)
    }

    fn u16(&mut self) -> Result<u16> {
        Ok(u16::from_le_bytes(self.take()?))
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_le_bytes(self.take()?))
    }

    fn f64(&mut self) -> Result<f64> {
        Ok(f64::from_le_bytes(self.take()?))
    }
}
