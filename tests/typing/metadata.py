"""Editor-facing API contracts, checked by ty in CI."""

from typing import Literal

import pyivt
from typing_extensions import assert_type


def check_metadata(ivt: pyivt.Ivt) -> None:
    kind: pyivt.DimensionType = "time_series"
    for dimension in ivt.metadata.dimensions:
        assert_type(dimension.type, Literal["geographic", "normal", "time_series"])
        if dimension.type == kind:
            assert_type(dimension.members.en, list[str] | None)
    for footnote in ivt.metadata.footnotes:
        assert_type(footnote.description, pyivt.LocalizedText)
        assert_type(footnote.description.fr, str | None)
    for missing in ivt.metadata.missing_values:
        assert_type(missing.description["en"], str | None)
