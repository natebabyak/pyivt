# pyivt

Read Statistics Canada Beyond 20/20 (`.ivt`) files in Python. Inspect bilingual metadata and export data to Apache Arrow or Parquet.

pyivt is unstable until version 0.1.0. Its public API and output schemas may change between earlier releases.

## Install

```sh
pip install pyivt
```

With uv, run `uv add pyivt`. The package requires Python 3.10 or later.

## Open a file and export it

```python
import pyarrow.parquet as pq
import pyivt

ivt = pyivt.open("data.ivt")
print(ivt.metadata.title.en or ivt.metadata.title.fr)

table = ivt.to_arrow()
members = ivt.members(language="en")
pq.write_table(table, "data.parquet")
members.to_json("data.members.json")
```

`pyivt.open()` also accepts bytes or a binary file object. For a download, pass the response bytes.

The table has one unsigned member-code column per dimension and a nullable `float64` value column. Keep the JSON with the export to retain localized labels and native attributes.

## Read metadata

```python
meta = ivt.metadata

print(meta.title.en, meta.title.fr)
for dimension in meta.dimensions:
    print(dimension.name.en, dimension.items)
```

Use `.en` or `.fr` for localized fields. Missing translations are `None`. You can inspect metadata without exporting cells.

## Select members

```python
axis = ivt.layout.slugs[0]
selection = pyivt.Selection({axis: [1]})
subset = ivt.to_arrow(selection)
subset_members = ivt.members(selection)
subset_members.validate_table(subset)
```

Keys are dimension slugs and values are 1-based member codes. An omitted dimension keeps all its rows and leaves out its column. The export does not aggregate values.

## Generate typed member codes

`pyivt.codegen("data.ivt")` returns Python source containing an `IntEnum` for each dimension and a typed selection helper. You can also pass saved members JSON. See the [codegen example](docs/usage.md#generate-types-for-one-table).

## Documentation

- [Usage guide](docs/usage.md) for metadata, member maps, pandas, and Polars.
- [Development guide](docs/development.md) for tests and benchmarks.

## License

[MIT](LICENSE)

## Acknowledgements

- [canivt](https://github.com/mountainMath/canivt)
- [REA](https://github.com/morluto/rea)
