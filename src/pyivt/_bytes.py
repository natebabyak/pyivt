"""Low-level little-endian readers (0-based offsets)."""

from __future__ import annotations

import struct


def u16(data: bytes | memoryview, off: int) -> int:
    return struct.unpack_from("<H", data, off)[0]


def u32(data: bytes | memoryview, off: int) -> int:
    return struct.unpack_from("<I", data, off)[0]


def i16_run(data: bytes | memoryview, off: int, n: int) -> list[int]:
    if n <= 0:
        return []
    return list(struct.unpack_from("<" + "h" * n, data, off))


def i32_run(data: bytes | memoryview, off: int, n: int) -> list[int]:
    if n <= 0:
        return []
    return list(struct.unpack_from("<" + "i" * n, data, off))


def f64_run(data: bytes | memoryview, off: int, n: int) -> list[float]:
    if n <= 0:
        return []
    return list(struct.unpack_from("<" + "d" * n, data, off))


def is_label_byte(b: int) -> bool:
    return (b >= 32 and b != 127) or b in (9, 10, 13)


def latin1(data: bytes | memoryview) -> str:
    raw = bytes(data).replace(b"\x00", b" ")
    return raw.decode("cp1252", errors="replace")


def rd_pascal(data: bytes | memoryview, off: int, max_len: int = 250) -> dict | None:
    if off >= len(data):
        return None
    length = data[off]
    if length < 1 or length > max_len or off + 1 + length > len(data):
        return None
    chunk = data[off + 1 : off + 1 + length]
    if not all(is_label_byte(b) for b in chunk):
        return None
    return {"text": latin1(chunk), "len": length, "end": off + 1 + length}


def nextpow2(x: int) -> int:
    if x <= 1:
        return 1
    return 1 << (x - 1).bit_length()
