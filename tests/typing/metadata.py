"""Editor-facing API contracts, checked by ty in CI."""

from collections.abc import Iterator
from typing import Literal

import pyarrow as pa
import pyivt
from typing_extensions import assert_type


def check_metadata(ivt: pyivt.Ivt) -> None:
    assert_type(ivt.layout.geo_dim, int | None)
    assert_type(ivt.metadata.descriptor_path, str)
    assert_type(ivt.to_batches(), Iterator[pa.RecordBatch])
    frequency: pyivt.Frequency = "semi_annual"
    assert_type(
        ivt.metadata.frequency,
        Literal["monthly", "quarterly", "semi_annual", "annual"] | None,
    )
    if ivt.metadata.frequency == frequency:
        assert_type(ivt.metadata.frequency, Literal["semi_annual"])
    kind: pyivt.DimensionType = "time_series"
    for dimension in ivt.metadata.dimensions:
        assert_type(dimension.type, Literal["geographic", "normal", "time_series"])
        assert_type(dimension.items, int)
        if dimension.type == kind:
            assert_type(dimension.members.en, list[str] | None)
    for footnote in ivt.metadata.footnotes:
        assert_type(footnote.description, pyivt.LocalizedText)
        assert_type(footnote.description.fr, str | None)
    for missing in ivt.metadata.missing_values:
        assert_type(missing.description["en"], str | None)
