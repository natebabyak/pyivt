# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.0.1] - 2026-10-18

First public release: Rust/PyO3 reader for Statistics Canada Beyond 20/20 `.ivt` files.

### Added

- `pyivt.open(source)` — open from path, bytes-like buffer, or binary file object
- Typed metadata (`IvtMetadata`, `LocalizedText`, `TimeSeries`, `Dimension`, `Geographies`, `Footnote`, `MissingValue`) with lazy-loaded heavy fields
- `Ivt.to_arrow(complete=, max_cells=, labels=, language=)` — published grid as an Apache Arrow table (via pyarrow)
- PEP 561 type stubs (`py.typed`, `_native.pyi`)
- StatCan golden metadata integration tests; Beyond demo fixtures when available locally

### Known limitations

- `Dimension.ordinal` is always `None`; footnote `scope` / `dimension` / `member_id` are unused
- Metadata golden snapshots do not assert geographies or cell values (Arrow tests check row counts)
- Some StatCan fixtures lock truncated dimension names or noisy footnote bytes from current parsing
- Demo IVT tests skip in CI unless Beyond sample files are linked locally
- No CLI or catalogue download; write Parquet yourself via `pyarrow` / Polars / pandas

[0.0.1]: https://github.com/natebabyak/pyivt/releases/tag/v0.0.1
