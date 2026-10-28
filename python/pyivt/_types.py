"""Public type aliases shared by editor stubs and Python callers."""

from typing import Literal, TypeAlias

DimensionType: TypeAlias = Literal["geographic", "normal", "time_series"]
"""Dimension classification as a closed set of string values.

```python
from pyivt import DimensionType

kind: DimensionType = "time_series"
time_axes = [dim for dim in ivt.metadata.dimensions if dim.type == kind]
```
"""

Frequency: TypeAlias = Literal["monthly", "quarterly", "semi_annual", "annual"]
"""Time-series frequency as a closed set of string values.

`IvtMetadata.frequency` is `None` for non-time-series tables or unknown rates.

```python
from pyivt import Frequency

frequency: Frequency = "monthly"
is_monthly = ivt.metadata.frequency == frequency
```
"""
