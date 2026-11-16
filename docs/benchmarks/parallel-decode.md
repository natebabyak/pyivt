# Parallel value decode measurements

These results describe `decode_selected` after it was changed to decode
page-aligned chunks in parallel and to walk each page in contiguous runs.
Baseline: `main` at `de4a2e5`, which decodes on one thread and computes each
row's coordinates with division and modulo.

Measured on November 15, 2026 on Windows 11 x64 with an Intel Core i7-11800H
(8 cores, 16 hardware threads) and 16 GB RAM. Software: CPython 3.13.3,
release builds, PyArrow 26.0.0 for the branch and 26.0.1 for the baseline. Both builds ran on
the same machine in the same session. Each configuration ran three times. The
tables give the median. Raw records are in [parallel-decode.json](parallel-decode.json).

Input: held-out file 98100532.ivt (208,450,429 bytes, counts
`[57, 820, 5, 2, 3, 10, 9]`). It was used only for timing and for checking
that the output matches. No parser rule was tuned against it.

## `to_arrow(geography=1..40)`, 88,560,000 rows

```powershell
$env:RAYON_NUM_THREADS = 8   # unset for the baseline build
python -m scripts.profile_arrow --path 98100532.ivt --select geography_1_40.json --rss-cap-bytes 8000000000
```

| Build | Workers | `decode_values` (runs) | `decode_values` median | Speedup vs 1 worker | Speedup vs baseline | Total export |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
| Baseline `de4a2e5` | 1 | 5.589, 5.792, 5.475 s | 5.589 s | n/a | 1.0x | 5.697 s |
| This change | 1 | 2.393, 2.379, 2.434 s | 2.393 s | 1.0x | 2.3x | 2.616 s |
| This change | 8 | 0.529, 0.563, 0.548 s | 0.548 s | 4.4x | 10.2x | 2.575 s |
| This change | 16 | 0.385, 0.396, 0.386 s | 0.386 s | 6.2x | 14.5x | 2.566 s |

The 8-worker row is the acceptance measurement: 4.4x faster than one worker
in the same build, and 10.2x faster than the single-threaded baseline. With one
worker, the run-based walk alone is 2.3x faster than the baseline.

Total export time hardly changes beyond one worker. `to_arrow()` decodes
values while it builds the dimension code columns, and building the columns
takes about 2.5 s. That stage is now the slowest one. Peak process RSS was
1.68-1.69 GB in every configuration.

## `iter_batches()`, full table, 126,198,000 rows

Single-file stream with the default batch size, timed in-process. Times
exclude `open()`.

| Build | Workers | Runs | Median |
| --- | ---: | --- | ---: |
| Baseline `de4a2e5` | 1 | 4.321, 4.293, 4.362 s | 4.321 s |
| This change | 1 | 2.965, 2.974, 2.954 s | 2.965 s |
| This change | 8 | 2.937, 2.933, 2.934 s | 2.934 s |

The data directory and missing-value mode are now parsed once per stream, not
once per batch. With about 1M rows per batch, the values decode faster than
the code columns are built, so extra workers do not help the stream here.

## Output identity

Every column of `to_arrow()` (values, validity bitmap and code columns) was
hashed with SHA-256 for three exports per training-corpus file: the full grid,
every other member on every axis, and a sparse mix. The same was done for the
98100532 export above. All 93 results were identical between the baseline,
one worker and eight workers. `cargo test` also checks the new decoder against
the old per-row decoder on the training corpus, with 1, 3, 64 and 1024 chunks,
ranges that start and end inside a page, and tables with remapped member
slots. `pytest` compares one worker against four workers in fresh processes.

## Thread pool

Decoding uses rayon's global pool. Set `RAYON_NUM_THREADS` to limit it, for
example to `1` in each worker of a multi-process job. When it is not set, the
pool uses one worker per hardware thread. Concurrent exports in one process
share the same pool, so they cannot oversubscribe the CPU. With one worker,
or with a range small enough to fit in one chunk, decoding runs on the calling
thread.
