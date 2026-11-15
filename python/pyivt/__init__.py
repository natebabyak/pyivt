"""Read Beyond 20/20 IVT metadata, member lookups, and Arrow tables.

The public API is exactly the names in ``__all__`` plus ``__version__``.
Everything else, including underscore-prefixed modules and attributes, is
private and may change in any release. See ``docs/api.md`` for the reference
and the stability policy.

```python
import pyivt

ivt = pyivt.open("table.ivt")
table = ivt.to_arrow()
members = ivt.to_member_table()
```
"""

from pyivt._errors import (
    CorruptFileError,
    IvtError,
    ResourceLimitError,
    UnsupportedFormatError,
)
from pyivt._native import (
    Dimension,
    Footnote,
    Geographies,
    Ivt,
    IvtMetadata,
    LocalizedLabels,
    LocalizedText,
    MissingValue,
)
from pyivt._open import open

__all__ = [
    "CorruptFileError",
    "Dimension",
    "Footnote",
    "Geographies",
    "Ivt",
    "IvtError",
    "IvtMetadata",
    "LocalizedLabels",
    "LocalizedText",
    "MissingValue",
    "ResourceLimitError",
    "UnsupportedFormatError",
    "open",
]


def _version() -> str:
    from importlib.metadata import PackageNotFoundError, version

    try:
        return version("pyivt")
    except PackageNotFoundError:
        # A source checkout without installed metadata: use the version the
        # native extension was built with, which also comes from Cargo.toml.
        from pyivt._native import __version__ as native

        return native


__version__: str = _version()
"""The installed pyivt version, read from the package metadata."""
del _version
