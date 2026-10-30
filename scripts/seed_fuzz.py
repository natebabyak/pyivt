"""Seed native fuzz targets with deterministic synthetic format records."""

import struct
from pathlib import Path

from scripts.ivt_synthetic import cube


def main():
    root = Path(__file__).resolve().parents[1] / "fuzz" / "corpus"
    ivt = cube((2, 2, 3))
    page = struct.pack("<BBH", 2, 1, 2048) + bytes(4096)
    for target, seeds in {
        "decompress_component": [page, bytes([0x82, 1, 3, 0, 0, 0xA0, 1, 0, 2, 0])],
        "metadata_directory": [ivt],
        "decode_page": [page, bytes([2, 1, 0, 0])],
        "read_members": [ivt],
    }.items():
        destination = root / target
        destination.mkdir(parents=True, exist_ok=True)
        for index, seed in enumerate(seeds):
            (destination / f"synthetic-{index}").write_bytes(seed)


if __name__ == "__main__":
    main()
