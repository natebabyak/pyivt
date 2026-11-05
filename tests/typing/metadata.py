"""Editor-facing API contracts, checked by ty in CI."""

from typing import Literal

import pyarrow as pa
import pyivt
from typing_extensions import assert_type


def check_metadata(ivt: pyivt.Ivt) -> None:
    assert_type(ivt.to_arrow(geo=[1]), pa.Table)
    assert_type(ivt.to_arrow(), pa.Table)
    assert_type(ivt.members(geo=[1]), dict[str, pa.Table])
    assert_type(ivt.members(language="fr")["geo"], pa.Table)
    assert_type(
        ivt.metadata.frequency,
        Literal["monthly", "quarterly", "semi_annual", "annual"] | None,
    )
    if ivt.metadata.frequency == "semi_annual":
        assert_type(ivt.metadata.frequency, Literal["semi_annual"])
    kind = "time_series"
    for dimension in ivt.metadata.dimensions:
        assert_type(dimension.type, Literal["geographic", "normal", "time_series"])
        assert_type(dimension.items, int)
        assert_type(dimension.slug, str)
        if dimension.type == kind:
            assert_type(dimension.members.en, list[str] | None)
    for footnote in ivt.metadata.footnotes:
        assert_type(footnote.description, pyivt.LocalizedText)
        assert_type(footnote.description.fr, str | None)
    for missing in ivt.metadata.missing_values:
        assert_type(missing.description["en"], str | None)
