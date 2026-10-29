# Paged IVT regression fixture

`98100018-paged.ivt` retains the data and metadata of Statistics Canada table
98-10-0018, **Population and dwelling counts: Regional county municipalities
(RCM), Quebec**, reference year 2021. Only its data-directory representation
was changed, using the native flag and address rules established by static
analysis in [the investigation](../../docs/investigations/ivt-native-directory.md).
No browser interaction is required to generate or test it.

Adapted from Statistics Canada, *Population and dwelling counts: Regional
county municipalities (RCM), Quebec*, 2021. This does not constitute an endorsement
by Statistics Canada of this product.

`98100018.csv` is the unmodified, independently published English CSV from the
same table. Source: Statistics Canada, *Population and dwelling counts: Regional
county municipalities (RCM), Quebec*, 2021. Reproduced and distributed on an
"as is" basis with the permission of Statistics Canada.

Data are supplied under the [Statistics Canada Open Licence](https://www.statcan.gc.ca/en/terms-conditions/open-licence).

## Source identities

Downloaded 2026-10-28:

- Original IVT: `https://www150.statcan.gc.ca/n1/en/tbl/b2020/98100018.zip`,
  37,009 bytes, SHA-256
  `ac8356156f994c45bf464eed0e0cf114e48ccdadf23b00bb65bea8c546ffa99b`.
- CSV archive: `https://www150.statcan.gc.ca/n1/en/tbl/csv/98100018-eng.zip`.
  Extracted `98100018.csv` SHA-256:
  `402f935ce08bde6d4ae1aac9a52770d538eb9edf5b5065ae27b8dd3d6cf23045`.
- Transformed IVT: 69,781 bytes, SHA-256
  `83b1655af6af255f1f4611907892d81caed70a30eda0fc14294dbfb1ab2de56f`.

The original IVT was already present in the local corpus. Its download URL follows
the repository's existing fixture downloader; the digest binds the exact bytes
used here rather than assuming that a future download is identical.

## Reproduce the transformation

Starting with the original IVT at `source_path`, run this ordinary Python code.
It does not import `pyivt` or generate expected values from its output.

```python
from pathlib import Path
import hashlib
import struct

source = Path(source_path).read_bytes()
assert hashlib.sha256(source).hexdigest() == (
    "ac8356156f994c45bf464eed0e0cf114e48ccdadf23b00bb65bea8c546ffa99b"
)
data = bytearray(source)
pointer, capacity, count, flags = struct.unpack_from("<IIIH", data, 558)
assert (pointer, capacity, count, flags) == (6922, 2, 1, 0)
assert struct.unpack_from("<H", data, 20)[0] == 4096
page_start = len(data)
data.extend(source[pointer : pointer + 8 * capacity])
data.extend(bytes(4096 * 8 - 8 * capacity))
index_start = len(data)
data.extend(struct.pack("<I", page_start))
struct.pack_into("<IIIH", data, 558, index_start, 4096, count, flags | 1)
assert hashlib.sha256(data).hexdigest() == (
    "83b1655af6af255f1f4611907892d81caed70a30eda0fc14294dbfb1ab2de56f"
)
Path("tests/fixtures/98100018-paged.ivt").write_bytes(data)
```

All original data/component offsets remain absolute and unchanged. The original
flat directory is retained as unused bytes. One full 32 KiB directory page is
appended at byte 37,009, followed by its `u32` pointer at byte 69,777. The data slot
declares capacity 4096, count 1, and paging flag 1. The old directory locator
expects an eight-byte flat entry at that four-byte index and rejects the file;
the new decoder follows the pointer.

## What the oracle verifies

`tests/test_paged_arrow.py` reads the raw CSV with `csv.reader`, preserving its
repeated `Symbols` headers. The `Coordinate` column gives geography ordinals
1–105; bracketed header ordinals give measures 1–11. Every one of the 1,155
coordinate/value/symbol cells is compared, along with English geography and
measure labels, DGUIDs and null counts. Float64 values match exactly, with no
rounding tolerance. CSV omits hierarchy indentation, so only label comparison
trims surrounding whitespace.

The separate 32-column schema assertion covers the entire Arrow type/name
contract. It is not an independent value oracle for the native auxiliary or
French fields absent from the CSV. `tests/test_typed_arrow.py` separately uses a
hand-authored native fixture and literal complete Arrow result to test all member
field storage types, both languages, dates, missing statuses and raw annotations.

The optional canivt 0.5.0 references lose `geo_uid` for this exact original IVT
and warn about heuristic geography recovery. The official CSV contains
`2021A000224` for Quebec and blanks for its synthetic RCM geographies. The test
uses this independent CSV for that field and continues comparing all other
canivt fields by explicit name. It does not alter parsed data to match the
incomplete canivt output.
