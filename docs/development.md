# Developing pyivt

## Build and test

From the repository root, install Rust and use uv to set up the development environment.

```sh
uv sync --group dev
uv run pytest
cargo test
```

Run `uv run maturin develop --release --skip-install` after changing the Rust extension if you need to rebuild it in the existing environment.

The test suite includes offline paged and typed IVT fixtures. See the [fixture provenance](../tests/fixtures/README.md) and [native directory investigation](investigations/ivt-native-directory.md).

For the Census corpus checks, download the inputs listed in the test manifest.

```sh
uv run python scripts/download_test_inputs.py --manifest tests/corpus/training.json
uv run pytest
```

Tests use committed JSON references and compare Parquet references when available. For local IVTs under `data`, generate JSON with Beyond 20/20 Professional Browser on Windows and Parquet with R.

```sh
uv run python scripts/generate_expected_metadata.py --beyond PATH_TO_BROWSER
uv run python scripts/generate_expected_parquet.py
```

For the optional NAICS234 regression, set `PYIVT_REGRESSION_NAICS234_PATH` and `PYIVT_REGRESSION_NAICS234_EXPECTED_PARQUET`. The Parquet file must come from an independent export and match the detailed schema and row order. Run `uv run pytest tests/test_arrow_regression.py`.

## Run benchmarks

Place IVT files under `data`. The benchmark suite also discovers files in subdirectories.

```sh
uv sync --group benchmark
uv run maturin develop --release --skip-install
uv run --group benchmark pytest benchmarks --ivt-report .cache/benchmarks/results.json
```

The `canivt` backend uses R with the `canivt` and `arrow` packages. Set `R_HOME` if R is outside the default search path. Pass `--ivt-r-library PATH` to add an R library or `--ivt-backends pyivt` to run only the Python backend.

- [Compact Arrow measurements and local profiling](benchmarks/compact-arrow.md)
- [Recorded streaming measurements](benchmarks/streaming.md)
