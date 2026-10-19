# IVT byte ranges and selective metadata I/O

## Header pointers ([src/header.rs](../src/header.rs))

| Offset | Field | Role |
|--------|--------|------|
| 0 | signature | `04 00 20 00` or `02 00 20 00` |
| 32 | descriptor | Dimension-name chain |
| 40 / 48 | title FR / EN | Legacy title pointers (nonzero ⇒ `"legacy"`) |
| 544 | master directory | Identity, notes, keywords, FACET, flags |
| 552 | geo field count | |
| 558 | page directory (low) | Value-block anchoring |
| 572 | codebook | |
| 698 | status legend | Missing-value directory |
| 824 + 14×k | dimension slot | Per-axis block directory |

## Master directory

Each entry is 8 bytes: `u32 offset`, `u16 length` (plus padding). Summary text and **FACET04** title/author/category runs observed on the StatCan + Beyond demo corpus all lie inside these blocks. Prefer FACET parsing from loaded master blocks; a full-file scan is an explicit fallback (`IoMetrics.fallback_used`).

## Selective summary path

For files larger than 2 MiB, `pyivt.open(path)`:

1. Reads a header prefix and the master directory
2. Coalesces master block + descriptor ranges (default merge gap 64 KiB)
3. Parses identity, notes, keywords, FACET, and read-only from those buffers
4. Builds searchable text for time-series from those buffers (full-file Latin-1 only as fallback)

Files ≤ 2 MiB use one full read (same results as the legacy oracle; range coalescing would often cover the whole file anyway).

## Lazy extended fields

| Accessor | I/O |
|----------|-----|
| `dimensions` | Dim slots + per-dim block directories + member blocks (not geography arrays) |
| `geographies` | Geography dimension blocks only |
| `footnotes` | First 12 KiB + last 200 KiB (entire file if ≤ 200 KiB) |
| `missing_values` | Status legend at 698 → directory → index → status records |
| `to_arrow()` | Loads the full file (path opens defer this until called) |

Repeated property access uses an in-process range cache and parsed-field cache; it should not re-read the same ranges.

## Caller-side persistent cache (optional)

A safe cache key without hashing the whole file:

- canonical path
- size
- mtime (nanoseconds)
- pyivt parser/schema version

Keep such a cache outside the core library.
