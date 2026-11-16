# Contributing to pyivt

pyivt is a Rust extension built with [maturin](https://www.maturin.rs/) and
[PyO3](https://pyo3.rs/). You need a stable Rust toolchain and
[uv](https://docs.astral.sh/uv/).

## Set up

```sh
uv sync --group dev                   # create .venv and build pyivt into it
uv run maturin develop --release      # rebuild after changing Rust code
```

Some tests read StatCan tables that are not committed. Download them into
`data/` with:

```sh
uv run python scripts/download_test_inputs.py --manifest tests/corpus/training.json
uv run python scripts/download_test_inputs.py --manifest tests/corpus/legacy.json
```

Tests that need a missing table are skipped. Set `PYIVT_REQUIRE_DATA=1` to
make them fail instead, as CI does.

## Run the tests

Python tests:

```sh
uv run pytest
```

Rust tests:

```sh
uv run python scripts/cargo_test.py
```

PyO3 enables `extension-module` only through maturin, so a plain `cargo test`
links the test binaries against libpython. It fails with
`unable to find library -lpython3.x` when the Python that cargo finds has no
shared libpython (for example a Debian system Python without
`libpython3-dev`), or at run time when that library is not on the loader path.
`scripts/cargo_test.py` sets `PYO3_PYTHON` to the venv interpreter and adds its
library directory to `PATH`, `LD_LIBRARY_PATH` or `DYLD_LIBRARY_PATH`. If the
venv's Python has no shared libpython, use a uv-managed one:

```sh
uv python install 3.12
uv run python scripts/cargo_test.py --python 3.12
```

Arguments after `--` go to `cargo test`, for example
`uv run python scripts/cargo_test.py -- --release decode::`.

To run `cargo test` by hand, set the same variables yourself (Linux shown):

```sh
export PYO3_PYTHON="$(uv python find --managed-python 3.12)"
export LD_LIBRARY_PATH="$("$PYO3_PYTHON" -c 'import sysconfig; print(sysconfig.get_config_var("LIBDIR"))')"
cargo test
```

## Lint

CI runs all of these and fails on any warning:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
uv run ruff check .
uv run ruff format --check .
uv run ty check python tests/typing
```
