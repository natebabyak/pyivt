"""Typed, versioned member metadata kept separately from row buffers."""

from __future__ import annotations

import hashlib
import json
from collections.abc import Mapping
from dataclasses import dataclass
from itertools import pairwise
from pathlib import Path
from types import MappingProxyType
from typing import TYPE_CHECKING, Literal

if TYPE_CHECKING:
    import pandas as pd
    import pyarrow as pa

MemberValue = str | int | float | None


def _canonical(value: object) -> str:
    return json.dumps(
        value,
        ensure_ascii=False,
        sort_keys=True,
        separators=(",", ":"),
        allow_nan=False,
    )


def validate_codes(codes: list[int], table: str, dimension: str) -> int:
    """Verify a full native ordinal map and return its zero/one-based offset."""
    if not codes or type(codes[0]) is not int or codes[0] not in (0, 1):
        raise ValueError(
            f"table {table}, dimension {dimension}: codes must start at 0 or 1; got {codes[:8]!r}"
        )
    first = codes[0]
    for position, code in enumerate(codes):
        if type(code) is not int or code != first + position:
            raise ValueError(
                f"table {table}, dimension {dimension}: codes must be non-negative, unique, sorted and dense; offending codes {codes[max(0, position - 1) : position + 2]!r}"
            )
    return first


@dataclass(frozen=True)
class Member:
    """One native ordinal, display label, and all stored native attributes.

    Stored text codes (including leading zeros) remain in metadata. Dates are
    ISO strings; integers, floats and missing values retain their JSON types.
    """

    code: int
    label: str | None
    metadata: Mapping[str, MemberValue]

    def __post_init__(self) -> None:
        if type(self.code) is not int or self.code < 0:
            raise ValueError(f"invalid member code {self.code!r}")
        if self.label is not None and not isinstance(self.label, str):
            raise ValueError("member label must be text or None")
        if any(
            not isinstance(k, str)
            or (v is not None and type(v) not in (str, int, float))
            for k, v in self.metadata.items()
        ):
            raise ValueError("member metadata must map field names to JSON scalars")
        object.__setattr__(self, "metadata", MappingProxyType(dict(self.metadata)))


@dataclass(frozen=True)
class DimensionMembers:
    """Selected members in native order; dictionary index i maps to members[i]."""

    name: str
    label: str
    first_code: int
    members: tuple[Member, ...]

    def __post_init__(self) -> None:
        if self.first_code not in (0, 1):
            raise ValueError(f"dimension {self.name}: first_code must be 0 or 1")
        codes = [m.code for m in self.members]
        if any(c < self.first_code for c in codes) or any(
            a >= b for a, b in pairwise(codes)
        ):
            raise ValueError(
                f"dimension {self.name}: selected codes must be unique and sorted"
            )


@dataclass(frozen=True)
class Members:
    """Member maps for one source and selection, with JSON round-trip support.

    table_id is the SHA-256 of the source bytes. selection_id binds these
    member maps to the exact projection and subset in an exported Arrow table.
    """

    table_id: str
    dimensions: tuple[DimensionMembers, ...]
    schema_version: Literal[1] = 1

    def __post_init__(self) -> None:
        if self.schema_version != 1:
            raise ValueError(
                f"unsupported members schema version {self.schema_version}"
            )
        if len(self.table_id) != 64 or any(
            c not in "0123456789abcdef" for c in self.table_id
        ):
            raise ValueError("table_id must be a SHA-256 hex digest")
        if len({d.name for d in self.dimensions}) != len(self.dimensions):
            raise ValueError("duplicate member dimension name")

    def _payload(self) -> dict:
        return {
            "schema_version": self.schema_version,
            "table_id": self.table_id,
            "dimensions": [
                {
                    "name": d.name,
                    "label": d.label,
                    "first_code": d.first_code,
                    "members": [
                        {"code": m.code, "label": m.label, "metadata": dict(m.metadata)}
                        for m in d.members
                    ],
                }
                for d in self.dimensions
            ],
        }

    @property
    def selection_id(self) -> str:
        return hashlib.sha256(_canonical(self._payload()).encode("utf-8")).hexdigest()

    def to_json(self, path: str | Path | None = None) -> str:
        """Return JSON text, optionally writing it as UTF-8 to path."""
        text = _canonical({**self._payload(), "selection_id": self.selection_id})
        if path is not None:
            Path(path).write_text(text, encoding="utf-8")
        return text

    @classmethod
    def from_json(cls, text: str) -> Members:
        """Read JSON text, rejecting unknown versions and altered member maps."""
        try:
            data = json.loads(text)
            result = cls(
                data["table_id"],
                tuple(
                    DimensionMembers(
                        d["name"],
                        d["label"],
                        d["first_code"],
                        tuple(
                            Member(m["code"], m["label"], m["metadata"])
                            for m in d["members"]
                        ),
                    )
                    for d in data["dimensions"]
                ),
                data["schema_version"],
            )
            if data["selection_id"] != result.selection_id:
                raise ValueError("members selection_id does not match its contents")
            return result
        except (KeyError, TypeError, AttributeError) as error:
            raise ValueError("invalid members JSON") from error

    @classmethod
    def read_json(cls, path: str | Path) -> Members:
        """Read member maps from a UTF-8 JSON file."""
        return cls.from_json(Path(path).read_text(encoding="utf-8"))

    def validate_table(self, table: pa.Table) -> None:
        """Raise ValueError if the source, selection, or dictionary differs."""
        metadata = table.schema.metadata or {}
        expected = {
            b"pyivt.table_id": self.table_id.encode(),
            b"pyivt.selection_id": self.selection_id.encode(),
            b"pyivt.members_schema_version": b"1",
        }
        if any(metadata.get(k) != v for k, v in expected.items()):
            raise ValueError(
                "Arrow table and members have different table identifiers, selections, or schema versions"
            )
        if table.column_names != [d.name for d in self.dimensions] + ["value"]:
            raise ValueError("Arrow columns do not match the members dimensions")
        for d in self.dimensions:
            labels = [m.label for m in d.members]
            for chunk in table[d.name].chunks:
                if (
                    not hasattr(chunk, "dictionary")
                    or chunk.dictionary.to_pylist() != labels
                ):
                    raise ValueError(
                        f"dimension {d.name}: dictionary does not match the members"
                    )

    def to_pandas(self, table: pa.Table) -> pd.DataFrame:
        """Convert labels to pandas categoricals, including duplicate/null labels.

        pandas requires unique non-null categories. Remap only dictionary keys
        and member-sized labels; no row-sized Python strings are constructed.
        Native codes remain in this Members object.
        """
        import pandas as pd
        import pyarrow as pa
        import pyarrow.compute as pc

        self.validate_table(table)
        columns = {}
        for dimension in self.dimensions:
            categories = pd.Categorical([member.label for member in dimension.members])
            lookup = pa.array(categories.codes)
            keys = pa.chunked_array(
                [
                    pc.take(lookup, chunk.indices)
                    for chunk in table[dimension.name].chunks
                ],
                type=lookup.type,
            )
            columns[dimension.name] = pd.Categorical.from_codes(
                keys.combine_chunks().to_numpy(), categories.categories
            )
        columns["value"] = table["value"].to_pandas()
        return pd.DataFrame(columns, copy=False)
