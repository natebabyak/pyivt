# Release profile: LTO, codegen units and strip (NAT-256)

`Cargo.toml` had no `[profile.release]`, so wheels used Cargo's defaults. It now sets:

```toml
[profile.release]
lto = "fat"
strip = true
```

The Linux x86_64 wheel is 22% smaller: 1,794,886 bytes instead of 2,290,165. Value decoding is no slower. `codegen-units` stays at Cargo's default of 16, because `codegen-units = 1` made `decode_values` about 15% slower. `panic` stays `"unwind"`.

Measured on November 15, 2026 on one machine: Windows 11 (10.0.26300), Intel i7-11800H (8 cores, 16 hardware threads), 15.7 GB RAM, rustc 1.99.1, maturin 1.16.0. The baseline is `main` at `1a86e35`. Each build variant was set with `CARGO_PROFILE_RELEASE_*` environment variables on the same source tree.

## Linux x86_64 wheel size and build time

Every wheel was built from a clean target directory in the `ghcr.io/pyo3/maturin` container (Docker Desktop on WSL 2), with this command:

```sh
maturin build --release -i python3.10 --compatibility manylinux_2_28
```

Dependencies were fetched before the timer started, so build time is compile and link only. It is wall time for `maturin build`, with no sccache. Times in the table are for 16 CPUs. Some variants were also built with `--cpus=4`, which is about what a GitHub-hosted runner has.

| `lto` | `codegen-units` | `strip` | Wheel bytes | `.so` in wheel | Build, 16 CPUs | Build, 4 CPUs |
| --- | ---: | --- | ---: | ---: | --- | ---: |
| default (`false`) | 16 | no | **2,290,165** | 8,117,816 | 42.6, 40.1, 40.4 s | 64.2 s |
| default (`false`) | 16 | yes | 1,999,827 | 5,698,808 | 40.8 s | |
| `thin` | 16 | no | 2,224,209 | 7,752,928 | 31.7 s | |
| `thin` | 16 | yes | 1,951,363 | 5,513,424 | 31.3, 31.7 s | 45.8 s |
| `fat` | 16 | no | 1,935,029 | 5,596,736 | 53.3 s | |
| **`fat`** | **16** | **yes** | **1,794,886** | **4,647,536** | **52.6, 53.7 s** | **61.3 s** |
| default (`false`) | 1 | no | 1,940,218 | 5,761,280 | 57.2 s | |
| `thin` | 1 | no | 1,873,560 | 5,528,160 | 54.2 s | |
| `thin` | 1 | yes | 1,727,554 | 4,527,464 | 52.5, 53.6, 52.7 s | 55.7 s |
| `fat` | 1 | no | 1,823,081 | 5,092,696 | 71.1, 66.0 s | |
| `fat` | 1 | yes | 1,699,546 | 4,266,168 | 68.1, 68.0, 74.9 s | 70.4 s |

`strip = true` removes the symbol table and the standard library's debug info. The module still exports `PyInit__native` from the dynamic symbol table, and the stripped wheels import and export normally (see below). Without debug symbols, Rust panic backtraces lose function names. `RUST_BACKTRACE` output is the only thing affected.

`fat` LTO gives a wheel 8% smaller than `thin` (1.79 MB against 1.95 MB). It takes about 22 s longer on 16 CPUs and about 15 s longer on 4 CPUs, where it is close to the old default (61 s against 64 s). The release workflow builds each target in its own job, so this time is added once per job, not summed.

## `decode_values` on 98100532

Input: held-out file 98100532 (208,450,429 bytes) from `tests/corpus/heldout.json`. It was used only for timing, not to tune any parser rule.

Each run is a fresh process that does `open(path)`, then `to_arrow(geography=list(range(1, 41)))` (88,560,000 rows) with `PYIVT_PROFILE=1` and `RAYON_NUM_THREADS=8`. The `decode_values` time comes from the profile event. The file was in the page cache. Each round runs every build once, in a fixed order. Round 0 is a warm-up and is not counted. The "paired" column is the median, over rounds, of each round's time for the variant divided by the baseline time in the same round.

### Linux: the wheels above

Wheels installed into `python:3.10-slim` (Python 3.10.22, PyArrow 25.0.1). The input was copied into the container's file system.

| Session | Variant | Rounds | Median | Min | Paired vs baseline |
| --- | --- | ---: | ---: | ---: | ---: |
| 1 | baseline | 15 | 0.3357 s | 0.3132 s | |
| 1 | `thin`, cgu 1, strip | 15 | 0.3788 s | 0.3667 s | |
| 1 | `fat`, cgu 1, strip | 15 | 0.3792 s | 0.3474 s | |
| 2 | baseline | 12 | 0.3295 s | 0.3184 s | |
| 2 | default LTO, strip | 12 | 0.3184 s | 0.3093 s | |
| 2 | default LTO, cgu 1 | 12 | 0.3892 s | 0.3600 s | |
| 2 | `fat`, cgu 1 | 12 | 0.3741 s | 0.3561 s | |
| 2 | `thin` | 12 | 0.3249 s | 0.3062 s | |
| 2 | `fat` | 12 | 0.3246 s | 0.2989 s | |
| 3 | baseline | 15 | 0.3239 s | 0.3100 s | |
| 3 | **`fat`, strip (chosen)** | 15 | **0.3255 s** | 0.3124 s | **−1.9%** (slower in 5 of 15) |
| 3 | `thin`, strip | 15 | 0.3350 s | 0.3092 s | |
| 3 | `fat` | 15 | 0.3438 s | 0.3176 s | |

`codegen-units = 1` is the only setting that slows `decode_values`. Every variant with it is 13–18% slower, whatever its LTO setting. LTO alone and strip alone stay within run-to-run noise, which is a few percent here. The earlier review measured "no meaningful difference" for fat LTO with `codegen-units = 1`. That was the Rust-level `decode_selected` test, measured once per build. These repeated runs of the shipped wheel show the slowdown. The likely cause is different inlining or vectorization in the per-page decode loop when the crate compiles as one unit. That was not investigated further, because 16 units already give the size win.

### Windows x64

Windows wheels use the same profile. Two wheels were built on the host with `maturin build --release -i python3.13`: the baseline (`CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_STRIP=false`) and the new profile. Each was installed into its own venv (Python 3.13.3, PyArrow 26.0.1).

| | Baseline | New profile |
| --- | ---: | ---: |
| Wheel bytes | 1,806,329 | 1,818,398 |
| `_native.pyd` bytes | 5,190,144 | 5,090,816 |
| Build (single run, clean target) | 71 s | 83 s |
| `decode_values` median, 37 rounds | 0.3430 s | 0.3535 s |
| Paired vs baseline | | +1.1% (12 rounds), −0.3% (25 rounds, order reversed) |
| Rounds where new was slower | | 18 of 37 |

MSVC already keeps debug info out of the `.pyd` in a separate PDB, so the Windows wheel does not shrink: it is 0.7% larger, though the `.pyd` itself is 2% smaller. The two medians differ by 3%, but the paired comparison shows no consistent slowdown.

## `panic = "abort"` was not adopted

The crate relies on unwinding in two places:

- PyO3 catches a Rust panic at the Python boundary and raises `pyo3_runtime.PanicException`. With `abort`, any panic would end the Python interpreter.
- `Plan::batch` in `src/compact.rs` runs the value decoder on a `std::thread::scope` thread and turns a panic from `value_task.join()` into `IvtError::Parse("value decoder thread panicked")`. Rayon also passes worker panics back to the caller by unwinding.

## Tests

With the new profile, on Windows: `cargo test` and `cargo test --release` passed 76 tests (1 ignored). `pytest` passed 625 tests (15 skipped) against a `maturin develop --release` build. The Linux wheels above were checked by installing each one and running the 98100532 export: all returned 88,560,000 rows.
