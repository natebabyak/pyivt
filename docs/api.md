# pyivt API reference

This page lists the whole public API of pyivt and the rules for changing it.
Anything not listed here is private.

## Quick start

```python
import pyivt
import pyarrow.parquet as pq

ivt = pyivt.open("table.ivt")  # a path, bytes, a binary file, or a StatCan ZIP
meta = ivt.metadata
print(meta.title.en, [(dim.slug, dim.items) for dim in meta.dimensions])

table = ivt.to_arrow()  # one row per cell: 1-based member codes and `value`
members = ivt.to_member_table()  # labels and native attributes for each code
pq.write_table(table, "table.parquet")
pq.write_table(members, "table.members.parquet")

# Select by dimension slug: an int fixes a dimension, a list keeps a subset.
subset = ivt.to_arrow(select={"geography": 1, meta.dimensions[1].slug: [1, 2]})

# Stream tables too large to build at once.
with ivt.iter_batches(batch_size=500_000) as reader:
    for batch in reader:
        ...
```

## Public names

These are exactly the names in `pyivt.__all__`, plus `pyivt.__version__`.
A test keeps this list and `__all__` in sync.

<!-- public-api:start -->
| Name | Kind | Summary |
| --- | --- | --- |
| `open` | function | Open an IVT file or a ZIP that contains one. |
| `Ivt` | class | An opened table: metadata and Arrow exports. |
| `IvtMetadata` | class | Read-only table metadata from `Ivt.metadata`. |
| `Dimension` | class | One table axis from `IvtMetadata.dimensions`. |
| `Geographies` | class | Parsed geography member fields from `IvtMetadata.geographies`. |
| `Footnote` | class | One annotation legend entry from `IvtMetadata.footnotes`. |
| `MissingValue` | class | One status legend entry from `IvtMetadata.missing_values`. |
| `LocalizedText` | class | English and French versions of one text field. |
| `LocalizedLabels` | class | English and French label lists in member order. |
| `IvtError` | exception | Base class for files that cannot be read. A `ValueError`. |
| `CorruptFileError` | exception | The file is damaged. |
| `UnsupportedFormatError` | exception | A valid IVT file in a layout pyivt cannot read. |
| `ResourceLimitError` | exception | A memory estimate exceeds a safety limit. Also a `MemoryError`. |
<!-- public-api:end -->

The `pyivt` command (`pyivt codegen`) is also public. See [Command line](#command-line).

Modules and attributes whose names start with an underscore, such as
`pyivt._native`, `Ivt._layout` and `IvtMetadata._io_metrics()`, are private.
They exist for pyivt's own code, tests and benchmarks, are left out of the
type stubs, and can change or disappear in any release.

## Opening a table

### `pyivt.open(source, *, member=None) -> Ivt`

`source` is one of:

- a path (`str` or `os.PathLike[str]`),
- the complete file contents (`bytes`, `bytearray`, `memoryview`, or an object with `__bytes__`),
- a binary file object, read once from its current position and left open.

The contents may be an IVT file or a ZIP archive. A ZIP with exactly one
`.ivt` member opens it automatically. For an archive with several, pass the
exact, case-sensitive member name as `member`. Passing `member` for a source
that is not a ZIP raises `ValueError`.

`open()` parses the summary metadata. Cell values are decoded only by the
export methods. Immutable `bytes` are shared with the reader; mutable
contents are copied once. A reader opened from a path reads the file again
for each export, so keep it in place and unchanged while you use the reader.

Raises `TypeError` for an unsupported source type, `OSError` when the path
cannot be read, `ValueError` for a bad ZIP or `member`, and an
[`IvtError` subclass](#errors) when the file cannot be read.

## `Ivt`

Returned by `open()`; it cannot be constructed directly and needs no
`close()`. All methods are safe to call from several threads.

| Member | Returns | Notes |
| --- | --- | --- |
| `metadata` | `IvtMetadata` | The same object on every access. |
| `to_arrow(*, select=None, max_memory_bytes=2**31)` | `pyarrow.Table` | The whole selection at once. |
| `iter_batches(*, select=None, batch_size=1_048_576, max_memory_bytes=2**31)` | `pyarrow.RecordBatchReader` | The same table, streamed. |
| `to_member_table(*, select=None, language="en")` | `pyarrow.Table` | Member lookup. No cells are decoded. |

### Selecting dimensions

All three export methods take `select`, a mapping from dimension slugs
(`Dimension.slug`) to member codes. Codes are **1-based member positions**
in file order.

- An `int` fixes the dimension at one member and drops its column.
- A `list[int]` keeps the column and limits it to those members. An empty list gives zero rows.
- An omitted dimension keeps all members.

An unknown slug, an unknown or duplicate code, or a non-integer code raises
`ValueError` before anything is decoded. A `select` that is not a mapping
raises `TypeError`.

```python
ivt.to_arrow(select={"geography": 2, "census_year": [1, 2]})
ivt.to_member_table(select={"geography": 2, "census_year": [1, 2]})
```

**Deprecated:** passing dimensions as keyword arguments, as in
`to_arrow(geography=2)`, still works but emits a `DeprecationWarning` and
will be removed in a later minor release. Combining keyword dimensions with
`select` raises `TypeError`. Keyword selection was replaced because a slug
could collide with an option such as `batch_size`.

### `to_arrow(*, select=None, max_memory_bytes=2_147_483_648)`

Returns one row per selected cell in file order. There is one unsigned
integer column per retained dimension, named by its slug and holding member
codes, then a nullable `float64` column named `value`. The schema metadata
carries `pyivt.dimension_encoding=one_based_index` and `pyivt.table_id`.
Each call builds a new table that the reader does not retain.

The result plus one copy of the file must fit `max_memory_bytes`, an
estimate in bytes that defaults to 2 GiB. Larger selections raise
`ResourceLimitError`; raise the limit, or stream them with `iter_batches()`.
A limit that is not positive raises `ValueError`.

### `iter_batches(*, select=None, batch_size=1_048_576, max_memory_bytes=2_147_483_648)`

Streams the `to_arrow(select=...)` table as a `pyarrow.RecordBatchReader` of
at most `batch_size` rows per batch. Batches keep file order and the same
schema and metadata, so concatenating them equals `to_arrow(select=...)`.
Memory stays near one copy of the file plus about two batches, which must
fit `max_memory_bytes`. The reader
supports the Arrow C stream protocol, so Polars and DuckDB can read
it without copying. It can be read once, and it stays valid after the `Ivt`
is released. `batch_size` must be positive.

`Ivt` itself does not implement `__arrow_c_stream__`; call `iter_batches()`.

### `to_member_table(*, select=None, language="en")`

Returns one row per retained member. Columns are `dimension` (a
dictionary-encoded slug), `code` (the 1-based position), `label` (nullable,
in `language`, which is `"en"` or `"fr"`), then the file's native member
attributes. A native `Code` field is kept as text, usually as `native_Code`.
The same `select` picks the same members as `to_arrow()`. Join on
`dimension` and `code` to label a fact table.

## Metadata

All metadata objects are read-only snapshots; none can be constructed
directly except `LocalizedText` and `LocalizedLabels`. A missing field is
`None`, except where noted below.

Values keep the form decoded from the file. pyivt does not normalize them
across fields, so related fields can mark "absent" differently, as
`Footnote.number` and `MissingValue.code` do.

### `IvtMetadata`

Available after `open()`: `product_id`, `title`, `universe`, `notes`,
`version` (the IVT format version, not pyivt's), `signature`, `author`,
`category`, `keywords`, `number_of_datapoints`, `frequency`
(`"monthly"`, `"quarterly"`, `"semi_annual"`, `"annual"` or `None`),
`start_date`, `end_date` (ISO date strings) and `read_only`.

Loaded on first access and cached; these can raise `IvtError`:

- `dimensions: list[Dimension]` in file order.
- `geographies: Geographies`.
- `footnotes: list[Footnote]` in legend order.
- `missing_values: list[MissingValue]`.

### `Dimension`

`slug`, `name`, `description`, `notes`, `items` (the member count),
`type` (`"geographic"`, `"normal"` or `"time_series"`) and `members`
(a `LocalizedLabels`).

**Names and slugs.** IVT files store axis names cut to 15 characters. When
the table notes contain a variable list, `name` and `description` use the
full English and French titles from it; otherwise they keep the stored name.
The geographic axis always has the slug `geography`. Other slugs come from
the full English title: its ASCII letters and digits, lowercased, with
every other run of characters replaced by `_`, cut to at most 40 characters
and made unique within the table with a `_2`, `_3`, ... suffix. Slugs name the
Arrow columns, the `select` keys, the member table's `dimension` values and
the generated types.

### `Geographies`

Parallel lists for the geographic axis: `member_id`, `geo_label`
(`LocalizedLabels`), and fields parsed from the English labels:
`geo_name`, `geo_uid`, `dqf_code` and `tnr_short_form`. Each list may be
`None`, and its entries may be `None`.

### `Footnote`

`symbol`, `number` (0 when the file has no number) and `description`
(`LocalizedText`).

### `MissingValue`

`symbol`, `description` (`LocalizedText`) and `code` (`None` when the file
has no code).

### `LocalizedText` and `LocalizedLabels`

`LocalizedText(en=None, fr=None)` holds one string per language and
`LocalizedLabels(en=None, fr=None)` one list of labels per language. Read
them with `.en` / `.fr` or `["en"]` / `["fr"]`. A missing translation is
`None`, with no fallback to the other language. Any other key raises
`KeyError`.

## Errors

```text
ValueError
└── pyivt.IvtError
    ├── pyivt.CorruptFileError
    ├── pyivt.UnsupportedFormatError
    └── pyivt.ResourceLimitError   (also a MemoryError)
```

| Exception | Raised when |
| --- | --- |
| `CorruptFileError` | The file is truncated, its signature or header is damaged, or a record or directory points outside the file. The message starts with `Corrupt IVT file:`. |
| `UnsupportedFormatError` | The file is a valid IVT file, but its container version, descriptor or record layout is not supported. The message starts with `Unsupported IVT file:`. |
| `ResourceLimitError` | A memory estimate exceeds a safety limit. Narrow the selection or use `iter_batches()`. |

Every `IvtError` has the attributes `offset`, `length` and `file_len` (in
bytes), `slot` (header slot), `entry` (directory entry) and `dimension`
(zero-based axis), each `None` when unknown. `ResourceLimitError` adds
`estimate` and `limit` in bytes. Errors can come from `open()`, from the
lazy metadata properties, from the exports, and from reading a batch of an
`iter_batches()` stream.

Other failures use built-in exceptions: `OSError` for an unreadable path,
`TypeError` for an unsupported source, and `ValueError` for invalid
arguments such as an unknown slug.

## Command line

```sh
pyivt codegen table.ivt --output dimensions.py [--language en|fr]
```

Writes one Python module per table: an `IntEnum` per dimension whose values
are 1-based member codes, a `Dimensions` `TypedDict` to pass as `select`,
and `NATIVE_CODES[slug][code]`, the native code text or `None`.

```python
from dimensions import Dimensions, Geography

table = ivt.to_arrow(select=Dimensions(geography=[Geography.CANADA]))
```

## Version

`pyivt.__version__` is the installed version, read from the package
metadata. The single source of the version is `Cargo.toml`.

## Typing

pyivt ships `py.typed` and stubs for the native extension. Install
`pyarrow-stubs` to see `pyarrow.Table` and `pyarrow.RecordBatchReader`
return types. The sample in `tests/typing` is checked with ty, mypy and
pyright in CI.

## Stability and deprecation policy

pyivt follows [Semantic Versioning](https://semver.org/). Until 0.1.0 the API
can still change in any release. From 0.1.0 on, these rules apply.

**What is public.** The names in [Public names](#public-names), their
documented attributes, methods, parameters and defaults, the exception
classes and their base classes, `pyivt.__version__`, the `pyivt codegen`
options and the shape of the module it writes, and the export schemas:
column names and types, and the `pyivt.*` schema metadata keys.

**What is not.** Anything with a leading underscore; the exact text of
error messages; `repr()` output; performance and memory use, apart from the
documented bounds; and the value of `pyivt.table_id`, which identifies a
table but is not a stable checksum across versions.

**Breaking changes** include removing or renaming a public name, method,
attribute or parameter; changing a default, a return type or an export
schema; changing how slugs or names are derived for a given file; and
raising an exception that is not a subclass of the one raised before.

**Not breaking:** new public names, methods or attributes; new keyword-only
parameters with defaults; new `IvtError` subclasses below an existing
class; new schema metadata keys; reading files that used to be rejected;
and clearer error messages.

**Deprecation.** Before 1.0, breaking changes ship only in a minor release
(0.x to 0.x+1), never in a patch release. After 1.0, they ship only in a
major release. Before removing or changing anything public, pyivt emits a
`DeprecationWarning` (or a `FutureWarning` when the change alters results)
for at least one minor release, and the changelog lists it under
*Deprecated* with a migration note. The only exception is a fix for output
that was wrong, such as misread values or truncated names; those changes are
marked **Breaking** in the changelog with migration steps.

## Planned

Per-cell statuses and footnotes are not in the public exports yet. The aim
is for everything Beyond 20/20 shows for a table to be reachable from pyivt;
the shape of that API is still being designed, so these parts can change
before 0.1.0.
