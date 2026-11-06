# Compact Arrow export measurements

Measured locally on Windows x86-64, Python 3.13.3, PyArrow 26.0.0, release build,
on November 2, 2026. Timings exclude imports and synthetic fixture generation;
they include opening the source, member preparation, native construction and
Arrow import. These are single runs, not statistically stable speedup claims.

The synthetic input is a deterministic `1000 x 1000 x 80` IVT with eight text
fields per dimension and zero data pages. The results cover grid expansion and
metadata overhead on this synthetic input.

| Export | Rows | Wall time | Peak process-tree RSS | Arrow logical bytes |
| --- | ---: | ---: | ---: | ---: |
| Legacy, 1M cube | 1,000,000 | 0.0500 s | 101.5 MB | 137.0 MB |
| Compact, 1M cube | 1,000,000 | 0.0432 s | 66.9 MB | 13.1 MB |
| Legacy, 80M cube, default limit | rejected | 0.0086 s | 52.9 MB | n/a |
| Legacy, 80M cube, raised limit | incomplete; watchdog stopped | n/a | 1,502.3 MB | n/a |
| Compact, 80M cube | 80,000,000 | 1.4620 s | 1,106.9 MB | 1,040.1 MB |
| Compact, 5% selection | 4,000,000 | 0.0870 s | 105.3 MB | 48.1 MB |

MB means decimal megabytes. Arrow logical bytes can count shared dictionary-key
buffers more than once; they are not physical resident allocations. RSS is
sampled every approximately 5 ms across the worker process tree (including the
Windows virtualenv launcher), and can miss short peaks. The watchdog is a
sampled guard, not an OS-enforced limit.

The 5% selection took 5.95% of full wall time, 4.62% of logical output bytes,
and approximately 5.43% of RSS growth above each worker's post-import baseline.
Total RSS was 9.51% because the interpreter and libraries are fixed costs.
Narrowing the first axis from 1000 to 50 also reduces its key width to uint8.
Member parsing, validation, hashing and JSON identity are fixed preparation
costs, so very small selections need not scale linearly with row count.

## Stage measurements

Stage RSS below is total process RSS sampled during that stage, not memory
exclusively owned by the stage. The compact decoder and dimension-column builder
run concurrently, so their times overlap and must not be added.

| Stage | Legacy 1M time / peak RSS | Compact 80M time / peak RSS |
| --- | --- | --- |
| Prepare member arrays / typed maps | 6.60 ms / 60.3 MB | 27.81 ms / 61.5 MB |
| Decode rows / values | 17.98 ms / 86.9 MB | 1,431.44 ms / 1,106.9 MB |
| Attach row metadata / build dimension keys | 23.19 ms / 101.5 MB | 856.79 ms / 828.8 MB |
| Import Arrow table | 0.274 ms / 101.5 MB | 0.247 ms / 1,106.9 MB |

The legacy default estimate for 80M rows is 35,205,233,862 bytes, exceeding the
2 GiB default before decoding. With a 40 GB estimate budget, the legacy run
crossed the 1.5 GB watchdog after 31 completed batches (about 29.76M rows).
The completed legacy stage totals before termination were 0.602 seconds for
row decoding and 0.741 seconds for metadata attachment. These are partial
measurements; no full legacy runtime or peak-memory result is claimed.

The profiler's `compact` mode measures `to_arrow()`. Starting with v0.0.7, this measures numeric code columns and excludes the separate member export. The measurements above use v0.0.6. Its `legacy` mode
uses the private `_legacy_to_arrow()` diagnostic to reproduce the baseline
from commit `ea428cc`. Raw observations are in the adjacent
`legacy-*.json` and `compact-*.json` files.

## Profile a private file locally

```powershell
python -m scripts.profile_arrow --path 'C:\private\table.ivt' --mode compact --output compact.json
python -m scripts.profile_arrow --path 'C:\private\table.ivt' --mode compact --fraction .05 --output selected.json
python -m scripts.profile_arrow --path 'C:\private\table.ivt' --mode legacy --output legacy.json
```

For explicit codes, put a mapping such as `{"geography": [1, 3]}` in `selection.json`
and pass `--select selection.json`. Use the actual slugs from `ivt.layout.slugs`.
For legacy mode, raise `--max-memory-bytes` only when an export should exceed the allocation
estimate limit; separately set `--rss-cap-bytes` and `--timeout` for the worker
watchdog. Output contains stage times and memory, not source values or member
labels. File paths are included, so review reports before sharing.
