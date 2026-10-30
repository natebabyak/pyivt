# Bounded Arrow export measurements

Measured on Windows 11 x64, CPython 3.12.15, PyArrow 26, Rust release builds.
Baseline: a064c84; canonical dictionary exporter: 2e0d33d. Raw records and
per-file normalized output hashes: [streaming.json](streaming.json). GB is
1,000,000,000 bytes. Both final synthetic runs had NumPy 2.5.4 and pandas 3.1.0 installed.

| Cells | Baseline table peak RSS | Baseline seconds | Stream peak RSS | Stream seconds | Stream rows / GB |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 1,000,000 | 5.810 GB | 5.585 | 0.102 GB | 0.053 | 9,844,380 |
| 10,000,000 | >8 GB, watchdog killed | ? | 0.151 GB | 0.433 | 66,384,051 |
| 100,000,000 | >8 GB, watchdog killed | ? | 0.154 GB | 4.299 | 651,371,695 |

Synthetic inputs have dimensions 1000 ? 1000 ? (1, 10, 100), eight text
attributes per dimension, deterministic repeated labels, and blank zero pages.
These measure label expansion and grid allocation, not compressed-page throughput.
Rows/GB divides total processed rows by peak RSS: streaming capacity, not rows
retained simultaneously. RSS samples include the worker process tree (Windows
virtualenv launchers spawn a child), sampled every 10 ms; the parent is excluded.
Times include open/decode/export but exclude interpreter startup and fixture
construction. Runs are single samples; short peaks can fall between RSS samples.

The final `to_arrow()` materializes 1M rows at 0.091 GB in 0.047 seconds.
Its default conservative 2 GiB estimate rejects 10M and 100M with `MemoryError`
before decoding; their streams complete. The supplied limit controls an
allocation estimate, not an OS reservation or an absolute process RSS cap.
Release each batch as it is consumed.

All 20 local IVTs have identical normalized complete-output SHA-256 hashes
against a064c84. Normalization decodes dictionaries, widens member IDs, and
copies sliced arrays into canonical buffers before serialization. Every member
field, value, null, status, annotation and row order is compared. This is a
regression check, not an independent format oracle. All 20 final exports also
convert to pandas 3.1.0. Dictionary categories are unique and non-null; a synthetic
duplicate/null map regression verifies this without tuning against held-out data.
Unique maps still share their row-key buffers across fields.

The committed official 98100018 CSV, two new held-out CSV samples, and the
hand-authored typed fixture remain independent oracles. No existing golden
values changed. Tests normalize the intentional physical Arrow schema changes.

The requested `/workspace/ivtcorpus/` is absent on this Windows host. The 66-file,
8 GB acceptance run is **not verified** by these measurements.

Reproduce from a release build with psutil installed:

```shell
python -m scripts.benchmark_memory --mode stream --cells 1000000
python -m scripts.benchmark_memory --mode stream --cells 10000000
python -m scripts.benchmark_memory --mode stream --cells 100000000
python -m scripts.benchmark_memory --mode table --cells 10000000
python -m scripts.benchmark_memory --mode table --path data/<sha256>.ivt --hash
```

## Frozen held-out results

Native parsing was frozen at 9cbaee9; hashes were committed at 8966557 before
conversion. The initial run is preserved in [heldout-initial-results.jsonl](heldout-initial-results.jsonl).
The final exporter was rerun after canonicalizing dictionary categories using
synthetic duplicate/null tests. No native parsing rules were changed against
these held-out files.

All ten final streams completed: **882,706,378 rows**, at most **856,907,776 bytes RSS**.
The eight StatCan dimension vectors match independent WDS metadata; JODI grids
match their native dimension products. Four materialized exports completed;
six raised pre-flight `MemoryError`. No crashes, caps, row failures or dimension
mismatches occurred. Final raw records: [heldout-results.jsonl](heldout-results.jsonl).

| Input | Rows | Stream peak MB | Seconds | Materialized result |
| --- | ---: | ---: | ---: | --- |
| 98100036 | 336 | 50.3 | 0.132 | converted |
| 98100042 | 93,960 | 64.4 | 0.140 | converted |
| 98100021 | 890,112 | 185.7 | 0.235 | converted |
| 98100277 | 12,261,780 | 463.3 | 2.015 | MemoryError |
| 98100532 | 126,198,000 | 656.4 | 21.678 | MemoryError |
| 98100291 | 32,345,730 | 583.9 | 5.775 | MemoryError |
| 98100592 | 372,507,660 | 773.1 | 51.084 | MemoryError |
| 98100439 | 315,705,600 | 856.9 | 47.658 | MemoryError |
| NewProcedure_World_Primary.ivt | 6,985,600 | 280.8 | 0.958 | converted |
| NewProcedure_World_Secondary.ivt | 15,717,600 | 337.3 | 2.057 | MemoryError |

These corpus runs used a sampled 8 GiB RSS watchdog on Windows and a 6 GiB
allocation estimate limit, **not a hard OS cap**. The nightly Docker workflow
adds an 8 GiB cgroup cap; it has not been executed on this Windows host.
The two new unmodified official CSV samples independently match all 94,296
coordinate/value/symbol cells, English member labels and DGUIDs. Their gzip
containers preserve the original CSV bytes and pinned decompressed hashes.
