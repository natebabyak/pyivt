"""Sample usage of every public name, checked by ty, mypy and pyright in CI."""

from io import BytesIO
from pathlib import Path
from typing import assert_type

import pyarrow as pa
import pyivt


def sample(path: Path) -> None:
    assert_type(pyivt.__version__, str)
    ivt = pyivt.open(path)
    assert_type(ivt, pyivt.Ivt)
    assert_type(pyivt.open(str(path), member="table.ivt"), pyivt.Ivt)
    assert_type(pyivt.open(path.read_bytes()), pyivt.Ivt)
    assert_type(pyivt.open(BytesIO(b"")), pyivt.Ivt)
    assert_type(pyivt.open(memoryview(b"")), pyivt.Ivt)

    meta = ivt.metadata
    assert_type(meta, pyivt.IvtMetadata)
    assert_type(meta.title, pyivt.LocalizedText)
    assert_type(meta.title.en, str | None)
    assert_type(meta.product_id, str | None)
    assert_type(meta.version, str)
    assert_type(meta.number_of_datapoints, int | None)
    assert_type(meta.read_only, bool | None)

    dimensions = meta.dimensions
    assert_type(dimensions, list[pyivt.Dimension])
    selection: dict[str, int | list[int]] = {dimensions[0].slug: [1, 2]}
    assert_type(dimensions[0].members, pyivt.LocalizedLabels)
    assert_type(dimensions[0].members["fr"], list[str] | None)

    geographies = meta.geographies
    assert_type(geographies, pyivt.Geographies)
    assert_type(geographies.member_id, list[int])
    assert_type(geographies.geo_uid, list[str | None] | None)

    assert_type(meta.footnotes, list[pyivt.Footnote])
    assert_type(meta.footnotes[0].number, int)
    assert_type(meta.missing_values, list[pyivt.MissingValue])
    assert_type(meta.missing_values[0].code, int | None)

    assert_type(ivt.to_arrow(select=selection), pa.Table)
    assert_type(ivt.to_arrow(max_memory_bytes=4 << 30), pa.Table)
    with ivt.iter_batches(
        batch_size=1000, max_memory_bytes=1 << 30, select=selection
    ) as reader:
        assert_type(reader, pa.RecordBatchReader)
    assert_type(ivt.to_member_table(language="en", select=selection), pa.Table)

    assert_type(pyivt.LocalizedText(en="a"), pyivt.LocalizedText)
    assert_type(pyivt.LocalizedLabels(fr=["a"]), pyivt.LocalizedLabels)

    try:
        pyivt.open(b"")
    except pyivt.ResourceLimitError as error:
        assert_type(error.limit, int | None)
        assert_type(error.estimate, int | None)
    except pyivt.CorruptFileError as error:
        assert_type(error.offset, int | None)
    except pyivt.UnsupportedFormatError as error:
        assert_type(error.dimension, int | None)
    except pyivt.IvtError as error:
        assert_type(error.file_len, int | None)
