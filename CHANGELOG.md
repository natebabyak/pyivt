# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.10] - 2026-11-10

### Added

- Open StatCan ZIP downloads from paths, bytes, or binary files. Use `member=` to select an IVT when an archive contains several.
- Read version-2 IVT tables using native axis allocations, live code slots, and date records, including historical Business Patterns tables.

### Fixed

- Read older bilingual dimension summaries at their declared directory entries and honor native code fields selected for display.
- Validate metadata records identically for path and bytes inputs, with directory context in errors for records extending past EOF.
- Bound dimension slots to the header's declared slot table so 12-axis tables cannot read into the next file region.

## [0.0.9] - 2026-11-05

### Changed

- `Ivt.to_member_table()` exports one Arrow lookup with a dictionary-encoded `dimension` column, followed by `code`, `label`, and native attributes.
- The geographic dimension uses `geography` in Arrow columns, member lookups, selection keywords, and generated types. Time dimension names follow the IVT file.
- Arrow dimension columns and member lookup keys use 1-based positions in file order. Their unsigned widths stay consistent across slices. The combined lookup uses the width of the largest dimension.
- Native `Code` values remain available as text in member lookups. The fact and lookup schemas identify the new encoding with `pyivt.dimension_encoding=one_based_index`.
- Generated enums use the same positions. The generated `NATIVE_CODES` mapping exposes the original text by dimension slug and enum value.

### Documentation

- The README explains how to install `pyarrow-stubs` for editor type hints and shows how to pivot a sliced table by industry and occupation with Polars.

### Migration

- Replace `ivt.members()` with `ivt.to_member_table()`. Filter the result by `dimension` to look up one axis, then cast `code` to the corresponding fact column's type before joining.
- Replace `geo` with `geography` in selection arguments and joins for geographic axes.
- Update selection arguments to use 1-based positions and regenerate codegen modules. Re-export fact tables and member lookups together before joining on the new codes.

## [0.0.8] - 2026-11-04

### Added

- `pyivt codegen INPUT.ivt --output OUTPUT.py` generates dimension enums and a `Dimensions` typed dictionary for one table.
- `Dimension.slug` exposes the dimension key used by Arrow and keyword selection.
- Development type checks resolve PyArrow return types through `pyarrow-stubs`.

### Migration

- Replace `Selection` and positional selection mappings with dimension keywords. Pass an integer to fix a dimension and drop its column, or a list to retain the column. Omitted dimensions retain every member and their columns.
- Replace `to_member_tables()` with `members()`. Use `members(language="fr")` for French values in the `label` column. Write lookup tables separately when migrating from `embed_members=True`.
- Use native integer codes in place of one-based positions. Axes without a native `Code` field still use one-based positions. Non-integer and duplicate native codes raise `ValueError`.
- Use `Dimension.slug` and `Dimension.items` in place of public `Layout` fields. `descriptor_path`, the always-empty dimension and footnote fields, and the `DimensionType` and `Frequency` exports were removed.
- Pass `ivt.to_arrow()` to Arrow consumers in place of the reader's Arrow stream protocol. Selection IDs and JSON round trips were removed with `Selection`.

## [0.0.7] - 2026-11-03

### Breaking changes

- `Ivt.to_arrow(selection)` returns a PyArrow table with native member codes in minimal-width unsigned columns, replacing the `(table, members)` tuple.
- `Ivt.to_member_tables(selection)` replaces `Ivt.members()` and the custom member objects and JSON sidecars. It returns one typed Arrow lookup per included dimension, with English and French labels and native attributes.
- Removed `Dimension.count`. Use `Dimension.items` for the declared member count.
- Removed the public `max_memory_bytes` argument. Compact exports and member preparation use an internal 2 GiB allocation estimate.

### Added

- Immutable, hashable `Selection` with validation, JSON round trips, and shared Arrow buffers for equivalent selections. Plain mappings remain accepted.
- `Ivt.to_arrow(embed_members=True)` embeds the lookup tables as base64-encoded Arrow IPC files in Arrow schema metadata. The fact columns stay compact.

### Fixed

- Time-series member labels use ISO 8601 dates.

### Migration

- Replace `table, members = ivt.to_arrow(sel)` with `table = ivt.to_arrow(sel)` and `members = ivt.to_member_tables(sel)`.
- Join each fact dimension column to its lookup's `code` column; use `label_en`, `label_fr`, and native attribute columns for labels and metadata.
- Save lookups as separate Arrow or Parquet tables, or use `embed_members=True` to keep them with the fact table.
- Replace the removed JSON-based `generate_member_types()` helper and member classes with the Arrow lookup tables.

## [0.0.6] - 2026-11-02

### Breaking changes

- `Ivt.to_arrow(select=None)` returns `(table, members)`. The compact table contains one label dictionary per included dimension and nullable `value`; per-row member fields, statuses and annotations are removed from this table
- Dictionary indices use the smallest unsigned type that fits the selected member count. Indices map to native numeric ordinals through the separate member maps; stored text codes retain leading zeros in member metadata
- Removed `Ivt.to_batches()`. The Arrow C stream provides detailed batches with the default batch size and memory budget.

### Added

- Selection by dimension slug and native integer ordinal, applied before row allocation and decoding. Omitted dimensions retain rows while omitting their column; empty code lists produce zero rows
- Immutable `Members`, `DimensionMembers` and `Member` objects, with standalone `Ivt.members(select)`, versioned JSON round trips, source/selection identities and `validate_table()` mismatch checks
- `Members.to_pandas(table)` remaps duplicate or missing labels into valid pandas categoricals without expanding Python strings per row
- Direct page-to-Arrow value buffers, concurrent value and dictionary-column construction, shared cached buffers and a bounded-memory profiling tool

### Fixed

- Read legacy member attributes in the 2011 National Household Survey profile. Unsigned integer attributes and ordinal fields stored with the note-reference type now export as integers.

### Migration

- Replace `table = ivt.to_arrow()` with `table, members = ivt.to_arrow()`
- Save `members.to_json(path)` alongside the Arrow table; `Member.code` is the native numeric ordinal and native text code fields remain in `Member.metadata`
- Read the Arrow C stream with `pyarrow.RecordBatchReader.from_stream(ivt)` for per-cell statuses, footnotes and expanded member fields. With PyArrow 14, use `pyarrow.RecordBatchReader._import_from_c_capsule(ivt.__arrow_c_stream__())`.
- See `docs/benchmarks/compact-arrow.md` for measured performance and the private-file profiling command

### Other changes

- The detailed Arrow C stream uses bounded row ranges with shared, deduplicated string dictionaries and compact annotation buffers
- Descriptor parsing requires the native axis matrix and exposes `metadata.descriptor_path`; unverified prose scanners and demo-specific scoring are removed
- Unsupported container versions 2 and 3 raise explicit errors for both path and byte sources; version 4 remains supported
- Native axis kinds and language references determine time-series types and translations; arbitrary header text no longer supplies product IDs
- Geography selection uses native geographic kinds or UID member fields; `layout.geo_dim` is `None` when no geography is declared, and `geographies.geo_uid` uses the native DGUID column

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

[Unreleased]: https://github.com/natebabyak/pyivt/compare/v0.0.10...HEAD
[0.0.10]: https://github.com/natebabyak/pyivt/compare/v0.0.9...v0.0.10
[0.0.9]: https://github.com/natebabyak/pyivt/compare/v0.0.8...v0.0.9
[0.0.8]: https://github.com/natebabyak/pyivt/compare/v0.0.7...v0.0.8
[0.0.7]: https://github.com/natebabyak/pyivt/compare/v0.0.6...v0.0.7
[0.0.6]: https://github.com/natebabyak/pyivt/compare/v0.0.5...v0.0.6
[0.0.5]: https://github.com/natebabyak/pyivt/compare/v0.0.4...v0.0.5
[0.0.4]: https://github.com/natebabyak/pyivt/compare/v0.0.3...v0.0.4
[0.0.3]: https://github.com/natebabyak/pyivt/compare/v0.0.2...v0.0.3
[0.0.2]: https://github.com/natebabyak/pyivt/compare/v0.0.1...v0.0.2
[0.0.1]: https://github.com/natebabyak/pyivt/releases/tag/v0.0.1
