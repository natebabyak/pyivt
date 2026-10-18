//! Low-level little-endian readers (0-based offsets).

use encoding_rs::WINDOWS_1252;

#[inline]
pub fn u16(data: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([data[off], data[off + 1]])
}

#[inline]
pub fn u32(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

pub fn i16_run(data: &[u8], off: usize, n: usize) -> Vec<i16> {
    if n == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(n);
    let mut o = off;
    for _ in 0..n {
        out.push(i16::from_le_bytes([data[o], data[o + 1]]));
        o += 2;
    }
    out
}

pub fn i32_run(data: &[u8], off: usize, n: usize) -> Vec<i32> {
    if n == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(n);
    let mut o = off;
    for _ in 0..n {
        out.push(i32::from_le_bytes([
            data[o],
            data[o + 1],
            data[o + 2],
            data[o + 3],
        ]));
        o += 4;
    }
    out
}

pub fn f64_run(data: &[u8], off: usize, n: usize) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(n);
    let mut o = off;
    for _ in 0..n {
        out.push(f64::from_le_bytes([
            data[o],
            data[o + 1],
            data[o + 2],
            data[o + 3],
            data[o + 4],
            data[o + 5],
            data[o + 6],
            data[o + 7],
        ]));
        o += 8;
    }
    out
}

#[inline]
pub fn is_label_byte(b: u8) -> bool {
    (b >= 32 && b != 127) || matches!(b, 9 | 10 | 13)
}

pub fn latin1(data: &[u8]) -> String {
    // Match Python: bytes.replace(b"\x00", b" ").decode("cp1252", errors="replace")
    let mut buf = Vec::with_capacity(data.len());
    for &b in data {
        buf.push(if b == 0 { b' ' } else { b });
    }
    let (cow, _, _) = WINDOWS_1252.decode(&buf);
    let mut s = cow.into_owned();
    // Python's cp1252 codec treats these bytes as undefined → U+FFFD.
    for ch in ['\u{0081}', '\u{008D}', '\u{008F}', '\u{0090}', '\u{009D}'] {
        s = s.replace(ch, "\u{FFFD}");
    }
    s
}

#[derive(Debug, Clone)]
pub struct PascalStr {
    pub text: String,
    #[allow(dead_code)]
    pub len: usize,
    pub end: usize,
}

pub fn rd_pascal(data: &[u8], off: usize, max_len: usize) -> Option<PascalStr> {
    if off >= data.len() {
        return None;
    }
    let length = data[off] as usize;
    if length < 1 || length > max_len || off + 1 + length > data.len() {
        return None;
    }
    let chunk = &data[off + 1..off + 1 + length];
    if !chunk.iter().copied().all(is_label_byte) {
        return None;
    }
    Some(PascalStr {
        text: latin1(chunk),
        len: length,
        end: off + 1 + length,
    })
}

pub fn nextpow2(x: i64) -> i64 {
    if x <= 1 {
        return 1;
    }
    let mut v = x - 1;
    let mut bits = 0;
    while v > 0 {
        v >>= 1;
        bits += 1;
    }
    1i64 << bits
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nextpow2() {
        assert_eq!(nextpow2(1), 1);
        assert_eq!(nextpow2(9), 16);
        assert_eq!(nextpow2(21), 32);
    }
}
