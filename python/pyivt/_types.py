"""Private type aliases used by the editor stubs.

These aliases are not exported from ``pyivt``. Annotate with the literal
strings instead, which type checkers treat as the same type.
"""

from typing import Literal, TypeAlias

DimensionType: TypeAlias = Literal["geographic", "normal", "time_series"]
"""Dimension classification as a closed set of string values.

```python
from typing import Literal

kind: Literal["geographic", "normal", "time_series"] = "time_series"
time_axes = [dim for dim in ivt.metadata.dimensions if dim.type == kind]
```
"""

Frequency: TypeAlias = Literal["monthly", "quarterly", "semi_annual", "annual"]
"""Time-series frequency as a closed set of string values.

`IvtMetadata.frequency` is `None` for non-time-series tables or unknown rates.

```python
from typing import Literal

frequency: Literal["monthly", "quarterly", "semi_annual", "annual"] = "monthly"
is_monthly = ivt.metadata.frequency == frequency
```
"""
