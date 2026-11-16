# Code column construction measurements

These results describe the compact export's code columns after they were
changed from computing `row / stride % count` for every row to writing runs
of `stride` copies of each code and then tiling the first whole period by
copying it. Columns are split into chunks that fill independently on the same
bounded rayon pool that decodes values. Baseline: `main` at `f0e5a6a`, which
already decodes values in parallel.

Measured on November 15, 2026 on Windows 11 x64 with an Intel Core i7-11800H
(8 cores, 16 hardware threads) and 16 GB RAM. Software: CPython 3.13.3,
PyArrow 26.0.0, release builds. Both builds ran on the same machine in the
same session. Each configuration ran three times. The tables give the median.
Raw records are in [code-columns.json](code-columns.json).

Input: held-out file 98100532.ivt (208,450,429 bytes, counts
`[57, 820, 5, 2, 3, 10, 9]`). It was used only for timing and for checking
that the output matches. No parser rule was tuned against it.

## `to_arrow(geography=1..40)`, 88,560,000 rows

```powershell
$env:RAYON_NUM_THREADS = 1   # only for the one-worker rows
python -m scripts.profile_arrow --path 98100532.ivt --select geography_1_40.json --rss-cap-bytes 8000000000
```

| Build | Workers | `build_dimension_columns` (runs) | Median | Speedup | Total export (median) |
| --- | ---: | --- | ---: | ---: | ---: |
| Baseline `f0e5a6a` | default (16) | 2.908, 2.936, 2.893 s | 2.908 s | 1.0x | 3.052 s |
| This change | default (16) | 0.514, 0.308, 0.298 s | 0.308 s | 9.4x | 0.422 s |
| Baseline `f0e5a6a` | 1 | 2.911, 2.821, 2.819 s | 2.821 s | 1.0x | 2.928 s |
| This change | 1 | 1.943, 1.900, 1.907 s | 1.907 s | 1.5x | 2.033 s |

`build_dimension_columns` runs at the same time as `decode_values`, so the
total export is now about as long as the longer of the two plus Arrow
wrapping. With one worker most of the remaining time is spent faulting in
about 700 MB of new column memory; extra workers fault pages in parallel.
Peak RSS was 1.48-1.58 GB for this change and 1.69 GB for the baseline with
the default pool.

## Output identity

Every `to_arrow()` and `iter_batches(batch_size=777)` result was written to
Arrow IPC by both builds and compared with `Table.equals(check_metadata=True)`:
four selections (full grid, every other member on every axis, the last member
fixed on alternate axes, and a sparse mix) for each of the 31 training-corpus
files, plus the 98100532 export above. All 249 tables were identical.
`cargo test` also compares the run fill against the per-row division for the
full grid, a fixed member and a sorted member list on every axis of a
four-axis grid, with ranges that start and end mid-run and chunk sizes from 1
row to the whole range.
