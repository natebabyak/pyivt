# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- Footnotes now contain both translations in `description: LocalizedText`, with one object per legend entry
- Missing-value `label` is renamed to `description`
- Dimension `type` now uses `DimensionType`, a literal enum of `"geographic"`, `"normal"`, and `"time_series"`; `type_label` is removed
- Editor and runtime documentation now includes Markdown examples; stubs describe metadata fields as read-only properties

### Fixed

- Keyword parsing preserves all NUL-separated or fixed-width keyword strings, including compressed records and localized language references
- Blank author and category translations return `None`

### Migration

- Replace `footnote.text` and language-specific rows with `footnote.description.en` / `.fr` (or language-key access); `footnote.language` is removed
- Replace `missing.label` with `missing.description`
- Replace numeric dimension types and `dim.type_label` checks with `dim.type == "geographic"`, `"normal"`, or `"time_series"`
- Treat blank author/category translations as `None` instead of `""`

## [0.0.3] - 2026-10-25

### Changed

- Flattened time series onto `IvtMetadata`: `frequency`, `start_date`, `end_date` replace nested `time_series` / `TimeSeries`
- Dimension `name`, `description`, and `notes` now use `LocalizedText`; `members` and `geographies.geo_label` use the new `LocalizedLabels` type, with `.en` / `.fr` and language-key access
- `Ivt.to_arrow()` is parameterless and exports the complete typed member grid, including missing cells, statuses, and raw annotations
- Tests discover local IVT inputs; benchmarks run isolated workers with an optional embedded R backend for `canivt` comparisons

### Added

- Scripts for downloading test inputs and generating metadata and Parquet references
- Local dev/test deps managed with uv (`dependency-groups`, `uv.lock`; `uv sync` installs the default `dev` group)
- Optional benchmark dependencies (`uv sync --group benchmark`)

### Fixed

- Reading paged IVT metadata directories and legacy missing-value entries
- Package, Rust crate, and runtime version identifiers now agree

### Migration

- Replace `meta.time_series.frequency` / `.start` / `.end` with `meta.frequency` / `meta.start_date` / `meta.end_date`
- Replace dimension `_en` / `_fr` fields with `dim.name.en`, `dim.description.fr`, `dim.members.en`, and `dim.notes.fr` (or language-key access)
- Replace `geographies.geo_label_en` / `geo_label_fr` with `geographies.geo_label.en` / `.fr`
- Drop use of `dim.is_geography` (use `dim.type_label == "Geographic"` or `layout.geo_dim` instead)
- `from pyivt import TimeSeries` is removed
- Remove `complete`, `max_cells`, `labels`, and `language` arguments from `to_arrow()` calls

## [0.0.2] - 2026-10-20

### Changed

- Path-based `pyivt.open` uses selective range I/O for files larger than 2 MiB so summary metadata no longer requires reading the entire IVT up front
- `dimensions` and `geographies` load independently (accessing dimensions no longer forces geography member arrays)
- Lazy metadata accessors may raise on I/O failure (`OSError` / `ValueError`) instead of assuming bytes are already in memory

### Added

- Internal positional reader, range coalescing, and I/O metrics (`IvtMetadata._io_metrics()` for benchmarks)
- [docs/ivt-ranges.md](docs/ivt-ranges.md) describing IVT pointers and which operations touch the file
- `tests/bench_metadata_io.py` harness (`PYIVT_BENCH_IVT` for an optional large network sample)

### Migration

- Prefer `pyivt.open(path)` over reading the file into Python bytes when only metadata is needed
- If you relied on `dimensions` also populating `geographies`, access `geographies` explicitly
- `to_arrow()` on path-backed objects may trigger a full file read the first time it runs

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

[Unreleased]: https://github.com/natebabyak/pyivt/compare/v0.0.3...HEAD
[0.0.3]: https://github.com/natebabyak/pyivt/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/natebabyak/pyivt/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/natebabyak/pyivt/releases/tag/v0.0.1
