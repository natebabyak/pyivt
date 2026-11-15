//! Low-level little-endian readers (0-based offsets).

#[inline]
pub fn u16(data: &[u8], off: usize) -> u16 {
    u16::from_le_bytes([data[off], data[off + 1]])
}

#[inline]
pub fn u32(data: &[u8], off: usize) -> u32 {
    u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]])
}

/// Cp1252 code points for bytes 0x80..=0x9F. Python's codec leaves 0x81,
/// 0x8D, 0x8F, 0x90 and 0x9D undefined, so they decode to U+FFFD.
#[rustfmt::skip]
const CP1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{FFFD}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{FFFD}', '\u{017D}', '\u{FFFD}',
    '\u{FFFD}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{FFFD}', '\u{017E}', '\u{0178}',
];

/// Match Python: `bytes.replace(b"\x00", b" ").decode("cp1252", errors="replace")`,
/// in one pass with one allocation.
pub fn latin1(data: &[u8]) -> String {
    // ASCII is one UTF-8 byte; 0xA0..=0xFF is two; 0x80..=0x9F is at most three.
    let high = data.iter().filter(|&&b| b >= 0x80).count();
    let mut s = String::with_capacity(data.len() + 2 * high);
    for &b in data {
        s.push(match b {
            0 => ' ',
            0x80..=0x9F => CP1252_HIGH[usize::from(b - 0x80)],
            // ASCII and 0xA0..=0xFF coincide with Latin-1 code points.
            _ => char::from(b),
        });
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

    /// The previous multi-pass implementation, kept as the output oracle.
    fn latin1_reference(data: &[u8]) -> String {
        let buf: Vec<u8> = data
            .iter()
            .map(|&b| if b == 0 { b' ' } else { b })
            .collect();
        let (cow, _, _) = encoding_rs::WINDOWS_1252.decode(&buf);
        let mut s = cow.into_owned();
        for ch in ['\u{0081}', '\u{008D}', '\u{008F}', '\u{0090}', '\u{009D}'] {
            s = s.replace(ch, "\u{FFFD}");
        }
        s
    }

    #[test]
    fn test_nextpow2() {
        assert_eq!(nextpow2(1), 1);
        assert_eq!(nextpow2(9), 16);
        assert_eq!(nextpow2(21), 32);
    }

    #[test]
    fn latin1_maps_nul_to_space() {
        assert_eq!(latin1(b"a\0b\0\0"), "a b  ");
        assert_eq!(latin1(b""), "");
    }

    #[test]
    fn latin1_undefined_cp1252_bytes_become_replacement() {
        assert_eq!(
            latin1(&[0x81, 0x8D, 0x8F, 0x90, 0x9D]),
            "\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}\u{FFFD}"
        );
        assert_eq!(latin1(&[0x80, 0x9F, 0x96]), "\u{20AC}\u{0178}\u{2013}");
    }

    #[test]
    fn latin1_accented_text() {
        // "Québec — Montréal" in cp1252.
        let bytes = b"Qu\xe9bec \x97 Montr\xe9al";
        assert_eq!(latin1(bytes), "Québec — Montréal");
        assert_eq!(latin1(b"\xc0\xff\xa0"), "\u{C0}\u{FF}\u{A0}");
    }

    #[test]
    fn latin1_matches_reference_for_every_byte() {
        let all: Vec<u8> = (0..=255).collect();
        assert_eq!(latin1(&all), latin1_reference(&all));
        for b in 0..=255u8 {
            let one = latin1(&[b]);
            assert_eq!(one, latin1_reference(&[b]), "byte 0x{b:02x}");
            assert!(one.len() <= 1 + 2 * usize::from(b >= 0x80));
        }
    }
}
