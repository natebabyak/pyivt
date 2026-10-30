# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `Ivt.to_batches()` and the Arrow C stream protocol export bounded row ranges with shared member dictionaries
- Conservative allocation estimates raise `MemoryError` and recommend streaming before unsafe materialization

### Changed

- Arrow member IDs use `int32`; text member fields, `symbol` and `status` use `dictionary<int32, utf8>`
- `to_arrow()` assembles chunked batches and caches Arrow buffers for repeated exports
- Descriptor parsing requires the native axis matrix and exposes `metadata.descriptor_path`; unverified prose scanners and demo-specific scoring are removed

### Migration

- Cast dictionary fields to `pa.string()` and member IDs to `pa.int64()` if an application requires the former physical schema
- Use `to_batches()` for large tables, or explicitly increase `max_memory_bytes` when materializing them

## [0.0.5] - 2026-10-29

### Added

- Read-only `Dimension.items` exposes the declared member count; `count` remains an alias
- Offline Arrow regressions compare all 1,155 cells of a committed paged IVT against an official CSV and verify a hand-authored fixture's complete typed output

### Changed

- Time-series `frequency` uses `Frequency`, a literal enum of `"monthly"`, `"quarterly"`, `"semi_annual"`, and `"annual"`; non-time-series and unknown rates return `None`
- Documented and validated the complete Arrow member-field projection: typed native attributes, bilingual labels, dates, missing statuses, and raw annotations

### Fixed

- Blank table and dimension notes consistently return `None`
- Keyword decoding respects stored string boundaries without interpreting framing bytes as control characters or leading spaces
- Arrow export resolves paged data directories using their declared capacity and count, including noncontiguous pages, with checked offsets and ordinal bounds
- Data records use their declared content and allocation lengths; malformed directory entries raise errors with diagnostic context

### Migration

- Replace stored-rate strings in `meta.frequency` checks with `"monthly"`, `"quarterly"`, `"semi_annual"`, or `"annual"`; handle `None` for absent or unknown frequencies
- Treat blank table and dimension notes as `None` instead of `""`

## [0.0.4] - 2026-10-27

### Changed

- Footnotes now contain both translations in `description: LocalizedText`, with one object per legend entry
- Missing-value `label` is renamed to `description`
- Dimension `type` now uses `DimensionType`, a literal enum of `"geographic"`, `"normal"`, and `"time_series"`; `type_label` is removed
- Editor and runtime documentation now includes Markdown examples; stubs describe metadata fields as read-only properties

### Fixed

- Test fixture downloads retry transient HTTP and network failures with exponential backoff
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

[Unreleased]: https://github.com/natebabyak/pyivt/compare/v0.0.5...HEAD
[0.0.5]: https://github.com/natebabyak/pyivt/compare/v0.0.4...v0.0.5
[0.0.4]: https://github.com/natebabyak/pyivt/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/natebabyak/pyivt/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/natebabyak/pyivt/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/natebabyak/pyivt/releases/tag/v0.0.1
