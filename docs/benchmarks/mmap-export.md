# Metadata read planning and memory-mapped exports (NAT-253)

Measured on November 15, 2026 on one machine: Windows 11 (10.0.26300), Intel
i7-11800H, 15.7 GB RAM, local NVMe SSD, Python 3.13.3, PyArrow 26.0.0, release
build. Inputs are the held-out tables 98100532 (208,450,429 bytes) and
98100291 (164,498,755 bytes) from `tests/corpus/heldout.json`. They were used
only for these I/O and memory checks, not to tune parser rules.

The file was in the OS page cache for every run (warm cache). No network share
was available, so the network-storage benefit is inferred from the read-op
counts, not measured.

## Metadata reads at `open(path)`

`open(path)` used to issue one positional read per metadata record, each
behind a mutex, and copy every read into a 32-entry cache. It now plans reads
one pointer level at a time (header and slot table, directories, directory
pages, records), coalesces ranges less than 64 KiB apart, and reads them in a
few large reads. Positional reads use `pread` on Unix and `seek_read` on
Windows without a lock, and the cache is gone: the catalog built at open
already serves later member, legend and footnote access, so nothing re-reads
records.

Counts come from `ivt.metadata._io_metrics()` (deterministic; single runs).

| Input | Read ops before | Read ops after | Bytes before | Bytes after |
| --- | ---: | ---: | ---: | ---: |
| 98100532, `open(path)` | 206 | 4 | 170,264 | 179,575 |
| 98100291, `open(path)` | 206 | 6 | 97,114 | 97,864 |
| 98100532, first `pyivt.table_id` after open | 131 | 3 | | |

Across all 31 training inputs in `data/`, `open(path)` now takes 3 to 8 read
ops (the most was 5,351 before, on the 16 MB table). Coalescing reads the gaps
between nearby records too, so bytes read grew by 0.8 KB to 76 KB per file
(0.02% on the 16 MB table, up to 5.5% on the held-out tables; small files can
read up to a few times more than before, still under 100 KB). The metadata,
including lazy dimensions, geographies, footnotes and missing values, and
`pyivt.table_id` were identical before and after for all 33 files.
`tests/test_io_planning.py` keeps the read-op bound.

## Whole-file loads for exports: `mmap` vs `std::fs::read`

The first export from a path open loads the file for value decoding. The
before case reads the file fully with `std::fs::read`. The after case maps it
read-only with `memmap2`. Each row is the median of 3 fresh-process runs,
taken with `PYIVT_MMAP=0` (full read) and the default (map), from the same
build.

`Time` covers the export call only, after `open()`. Peak memory is measured
from just before the export: *working set* is the OS peak working set (RSS),
and *commit* is the OS peak private commit (`peak_pagefile`). Both peaks are
tracked by the OS, not sampled. Mapped file pages count toward the working set
while they are resident, but they are clean page cache that the OS can drop,
not private memory.

| Export | Rows | Read: time | Map: time | Read: peak working set | Map: peak working set | Read: peak commit | Map: peak commit |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 98100532 `to_arrow(geography=[1])` | 2,214,000 | 0.123 s | 0.025 s | 248.6 MB | 44.7 MB | 250.0 MB | 41.2 MB |
| 98100532 `iter_batches()`, all batches | 126,198,000 | 1.229 s | 1.061 s | 230.7 MB | 228.9 MB | 232.2 MB | 23.3 MB |
| 98100291 `to_arrow(geography=[1])` | 185,895 | 0.086 s | 0.009 s | 169.4 MB | 6.6 MB | 170.0 MB | 5.7 MB |
| 98100291 `to_arrow()` | 32,345,730 | 0.261 s | 0.193 s | 658.7 MB | 657.9 MB | 661.6 MB | 496.4 MB |
| 98100291 `iter_batches()`, all batches | 32,345,730 | 0.392 s | 0.375 s | 187.0 MB | 183.2 MB | 188.0 MB | 22.8 MB |

MB means decimal megabytes. `to_arrow()` on all of 98100532 is refused by the
2 GiB estimate either way, so it is streamed instead.

What the numbers show:

* Selections touch only the pages they decode. With a map, a one-geography
  export of 98100532 takes 0.025 s instead of 0.123 s and peaks at 45 MB
  instead of 249 MB.
* Full exports and streams touch every page, so the peak working set is
  about the same, and time is the same or slightly lower. Private commit drops
  by about the file size (209 MB less on the 98100532 stream). That memory is
  reclaimable page cache instead of process memory.

## Decision

Exports map the file. The data supports it: selections are 5 to 10 times
faster with much lower peak memory, and full exports are no slower and commit
less private memory. We have no cold-cache or network-share measurements, so
this note claims nothing for those cases.

The safeguards, in `src/io/mapped.rs`:

* **Length check before mapping.** The file is mapped only if its current
  length equals the length its metadata was parsed against at `open()`, and
  the mapping's length is checked again after mapping. A file already
  truncated or replaced is read with `std::fs::read` instead, so the decoder
  reports the damage as an `IvtError`, as before.
* **Fallback.** Empty files and mapping failures also fall back to
  `std::fs::read`. Setting `PYIVT_MMAP=0` always uses the full read.
* **Change-while-open risk.** The length check cannot protect against changes
  made after mapping. On Unix, truncating a file while an export or batch
  stream maps it raises `SIGBUS` and ends the process. On Windows, the OS
  refuses to truncate a mapped file, but in-place writes by another process
  become visible. Do not modify or truncate an IVT file while an export or
  stream uses it. On shares where other clients may rewrite files, set
  `PYIVT_MMAP=0`.

The map is held only as long as the existing weak file handle: it is released
once no export or stream uses it, as the full-read buffer was.
