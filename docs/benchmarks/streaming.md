# Bounded Arrow export measurements

Measured on Windows 11 x64, CPython 3.12.15, PyArrow 26, Rust release builds.
Baseline: a064c84. Raw results, input hashes, and per-file normalized output
hashes are in [streaming.json](streaming.json). GB means 1,000,000,000 bytes.

| Cells | Baseline table peak RSS | Baseline seconds | Stream peak RSS | Stream seconds | Stream rows / GB |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000,000 | 5.788 GB | 5.546 | 0.091 GB | 0.050 | 10,977,053 |
| 10,000,000 | >8 GB, watchdog killed | — | 0.142 GB | 0.437 | 70,595,560 |
| 100,000,000 | >8 GB, watchdog killed | — | 0.143 GB | 4.118 | 700,346,027 |

Synthetic inputs have dimensions 1000 × 1000 × (1, 10, 100), eight text
attributes per dimension, deterministic repeated labels, and blank zero pages.
These measure label expansion and grid allocation, not compressed-page throughput.
Rows/GB divides total processed rows by peak RSS; it measures streaming capacity,
not the number of rows retained simultaneously. RSS samples include the worker's
process tree (Windows virtualenv launchers spawn a child). Sampling is every
10 ms; the parent/watchdog is excluded. Times include open/decode/export but
exclude interpreter startup and fixture construction. Runs are single samples.

The new `to_arrow()` materializes 1M rows at 0.090 GB in 0.049 seconds. Its
default conservative 2 GiB estimate rejects 10M and 100M with `MemoryError`
before decoding; their streams complete. The supplied limit controls an
allocation estimate, not an OS reservation or an absolute process RSS cap.
Keep memory headroom and release batches as they are consumed.

All 20 local IVTs produced identical normalized complete-output SHA-256 hashes
against the baseline. Normalization decodes dictionaries, widens member IDs,
and copies sliced arrays into canonical buffers before serialization. This
compares every member field, value, null, status, annotation and row order.
It is a regression check against the old path, not an independent format oracle.
The committed official 98100018 CSV and hand-authored typed fixture remain
independent oracles; neither golden data nor their literal expected values changed.
Tests normalize physical Arrow encoding because IDs now use int32 and text
uses dictionaries. Cached exports share Arrow buffers instead of cloning cells.

The requested `/workspace/ivtcorpus/` corpus is absent on this Windows host.
The 66-file, 8 GB acceptance run is **not verified** by these measurements.

Reproduce from a release build with `psutil` installed:

```shell
python -m scripts.benchmark_memory --mode stream --cells 1000000
python -m scripts.benchmark_memory --mode stream --cells 10000000
python -m scripts.benchmark_memory --mode stream --cells 100000000
python -m scripts.benchmark_memory --mode table --cells 10000000
python -m scripts.benchmark_memory --mode table --path data/<sha256>.ivt --hash
```

## First frozen held-out run

Parser frozen at 9cbaee9; input hashes committed at 8966557 before conversion.
All 10 streams completed: **882,706,378 rows**, at most **613,486,592 bytes RSS**.
The eight StatCan dimension vectors matched independent WDS metadata. Both
JODI grids matched their native dimension products. Four materialized exports
completed; six raised `MemoryError` in pre-flight. No crashes, caps, row-count
failures or dimension mismatches occurred. Raw data: [heldout-results.jsonl](heldout-results.jsonl).

| Input | Rows | Stream peak MB | Seconds | Materialized result |
| --- | ---: | ---: | ---: | --- |
| 98100036 | 336 | 39.3 | 0.042 | converted |
| 98100042 | 93,960 | 44.5 | 0.045 | converted |
| 98100021 | 890,112 | 103.7 | 0.125 | converted |
| 98100277 | 12,261,780 | 240.6 | 1.789 | MemoryError |
| 98100532 | 126,198,000 | 429.0 | 17.899 | MemoryError |
| 98100291 | 32,345,730 | 372.4 | 4.503 | MemoryError |
| 98100592 | 372,507,660 | 522.5 | 44.055 | MemoryError |
| 98100439 | 315,705,600 | 613.5 | 41.989 | MemoryError |
| NewProcedure_World_Primary.ivt | 6,985,600 | 234.2 | 0.909 | converted |
| NewProcedure_World_Secondary.ivt | 15,717,600 | 287.3 | 1.988 | MemoryError |

This run used a sampled 8 GiB RSS watchdog on Windows and a 6 GiB allocation
estimate limit. It did **not** use a hard OS cap. The committed nightly Docker
workflow adds an 8 GiB cgroup limit; it has not been executed on this Windows host.
The two new unmodified official CSV samples independently match all 94,296
coordinate/value/symbol cells, English member labels and DGUIDs. Their gzip
containers preserve the original CSV bytes and pinned decompressed hashes.
