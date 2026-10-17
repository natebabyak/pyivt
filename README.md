# pyivt

Minimal Python library for Statistics Canada **Beyond 20/20** `.ivt` tables:

- extract **file summary** and **dimension** metadata (members, geographies, footnotes)
- decode cell values into a tidy grid
- write **Parquet** (plus a `_members` sidecar) and metadata CSVs

Byte layout and decode geometry follow the reverse-engineered Beyond 20/20 format documented by [canivt](https://github.com/mountainMath/canivt). pyivt is an independent Python implementation, not an R binding.

Beyond 20/20 Professional Browser (`ProBrowser.exe` installer → `Brwpro.exe`) is useful for visual ground truth (File Summary / dimension item lists). This library reads the `.ivt` bytes directly and does not automate the GUI.

## Install

```bash
pip install -e ".[dev]"
```

## Quick start

```python
from pyivt import read_metadata, read_table, write_parquet, write_metadata

meta = read_metadata("98-400-X2016112.IVT")
print(meta.product_id, meta.title_en)
for d in meta.dimensions:
    print(d.name, d.count, (d.members or [])[:3])

tab = read_table("98-400-X2016112.IVT")          # published grid (absent → 0)
write_parquet(tab, "out.parquet")                # labelled long table + _members
write_metadata(meta, "out_metadata/")            # table_info / dimension_members / geographies
```

### CLI

```bash
python -m pyivt meta path/to/table.ivt --json
python -m pyivt meta path/to/table.ivt -o ./metadata
python -m pyivt parquet path/to/table.ivt -o ./table.parquet
```

## Metadata ↔ Beyond 20/20 File Summary

| Beyond 20/20 Summary field | pyivt field |
| --- | --- |
| Title (EN/FR) | `title_en`, `title_fr` (short FACET04 titles) |
| File Name / Directory | `file_name`, `directory` |
| Author (EN/FR) | `author_en`, `author_fr` |
| Category (EN/FR) | `category_en`, `category_fr` |
| Keywords | `keywords` |
| Notes (EN/FR) | `notes_en`, `notes_fr` (`notes` aliases `notes_en`) |
| Last Update | `last_update` (file mtime, UTC `YYYY/MM/DD HH:MM:SS`) |
| File Size | `file_size` |
| Number of Datapoints | `n_datapoints` |
| Time Series | `time_series` |
| Read-Only | `read_only` |
| Product id (in notes / keywords) | `product_id` |
| Universe | `universe` |
| Dimension name + items | `dimensions[].name`, `.members` / `.members_fr` |
| Geography codes / labels | `geographies` (`geo_name`, `geo_uid`, …) |
| Footnotes | `footnotes` |

Parquet columns use structural **slugs** (`geo`, `age_9`, …) plus optional `*_label` columns. Member ids are **1-based**, matching StatCan Member IDs / Beyond 20/20 item order.

## Scope (v1)

Validated against modern StatCan crosstabs such as `98-400-X2016112` (signature `04 00 20 00`). Not every canivt corpus edge case is covered yet (dense 1991 profile pages, `02 00 20 00` survey generation, type-00 Business Patterns, full cell-status / suppression tails).

## Tests

```bash
# optional: point at your IVT
set PYIVT_TEST_IVT=C:\path\to\98-400-X2016112.IVT
pytest
```

## License

MIT. Format understanding credited to the canivt project’s published notes.
