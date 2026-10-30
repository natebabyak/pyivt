"""Deterministic native IVT cubes for memory measurements (no external data)."""

import struct


def cube(counts: tuple[int, ...], label_fields: int = 8) -> bytes:
    data = bytearray(1024)
    data[:24] = bytes.fromhex(
        "04 00 20 00 20 00 00 00 40 00 08 00 20 02 00 00 20 00 0e 00 00 10 00 00"
    )

    def append(record: bytes) -> int:
        offset = len(data)
        data.extend(record)
        return offset

    def directory(slot: int, records: list[bytes]) -> None:
        capacity = 1 << (len(records) - 1).bit_length()
        start = append(bytes(8 * capacity))
        struct.pack_into("<IIIH", data, slot, start, capacity, len(records), 0)
        for index, record in enumerate(records):
            if record:
                assert len(record) <= 65535
                offset = append(record)
                struct.pack_into(
                    "<IHH", data, start + 8 * index, offset, len(record), len(record)
                )

    matrix = bytearray(struct.pack("<BBHH", 1, 2, len(counts), 82))
    for axis, count in enumerate(counts):
        row = bytearray(82)
        struct.pack_into("<I", row, 0, count)
        row[12:18] = f"Axis {axis}".encode()
        matrix.extend(row)
        definitions = bytearray(struct.pack("<BBHH", 1, 2, label_fields, 34))
        records = [b""]
        for field in range(label_fields):
            definition = bytearray(34)
            name = f"Label {field}".encode()
            definition[0] = len(name)
            definition[1 : 1 + len(name)] = name
            struct.pack_into("<H", definition, 18, 1)
            struct.pack_into("<I", definition, 26, len(records))
            definitions.extend(definition)
            text = bytearray(struct.pack("<H", count))
            for member in range(count):
                label = f"Axis {axis} field {field} member {member:04d} with repeated text".encode()
                text.extend(bytes([len(label)]) + label + b"\0")
            records.extend(
                [
                    struct.pack("<BBHI", 4, 1, 1, len(records) + 1),
                    struct.pack("<BBH", 1, 1, len(text)) + text,
                ]
            )
        records[0] = bytes(definitions)
        directory(824 + 14 * axis, records)
    settings = bytearray(32)
    struct.pack_into("<H", settings, 2, 3)
    directory(544, [b"", bytes(matrix), struct.pack("<BBH", 1, 1, 32) + settings])

    padded = [1 << (count - 1).bit_length() for count in counts]
    inner = 1
    for axis in range(len(counts) - 1, -1, -1):
        if inner * padded[axis] > 2048 or axis == 0:
            break
        inner *= padded[axis]
    windows = (counts[axis] + 2048 // inner - 1) // (2048 // inner)
    entries = 1 << (windows - 1).bit_length()
    for count in padded[:axis]:
        entries *= count
    start = append(bytes(8 * entries))
    struct.pack_into("<IIIH", data, 558, start, entries, entries, 0)
    return bytes(data)
