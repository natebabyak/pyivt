# Paged IVT directory validation

## Directory rules

The parser reads flat and paged data directories using the native slot's paging
flag, capacity, and declared count. It resolves each value component from its
recorded absolute offset. For the native addressing evidence, see the
[static investigation](ivt-native-directory.md).

Each directory entry records content and allocation lengths. Data reads use the
content length and require `0 < content <= allocation` for nonblank entries.
Entirely zero entries represent blank components. Malformed nonblank entries
raise an error with the ordinal, resolved position, and directory context.

## Run the offline checks

```sh
uv run pytest tests/test_paged_arrow.py tests/test_typed_arrow.py -q
```

The paged fixture checks all 1,155 coordinate, value, and symbol cells against
the official CSV. It also checks DGUIDs and English labels. See the
[fixture provenance](../../tests/fixtures/README.md) for reconstruction details.

The hand-authored fixture checks the detailed export's native member fields,
including numeric widths, leading-zero codes, bilingual text, dates, missing
statuses, and raw annotations. Native tests cover page boundaries, reordered
pages, invalid offsets and lengths, truncated entries, and count bounds.

## Run the independent NAICS234 comparison

Set `PYIVT_REGRESSION_NAICS234_PATH` to the IVT and
`PYIVT_REGRESSION_NAICS234_EXPECTED_PARQUET` to a complete independent export.
Use the detailed schema, with matching column names, types, and row order.

```sh
uv run pytest tests/test_arrow_regression.py -q
```

The test compares schema, shape, column contents, and null positions. It skips
when the IVT path variable is absent and fails if a requested input or reference
is missing. The original NAICS234 file was unavailable during the investigation,
so the paged fixture supplies the offline reproduction.
