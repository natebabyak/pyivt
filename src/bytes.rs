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
