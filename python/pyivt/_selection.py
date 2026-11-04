"""Immutable, reusable projections of native member codes."""

from __future__ import annotations

import hashlib
import json
from collections.abc import Mapping, Sequence
from dataclasses import dataclass
from enum import IntEnum
from typing import TYPE_CHECKING, TypeAlias

if TYPE_CHECKING:
    from pyivt._native import Ivt, Layout

SelectionMapping: TypeAlias = Mapping[str, Sequence[int]]


def _fingerprint(value: object) -> str:
    return hashlib.sha256(
        json.dumps(value, ensure_ascii=True, separators=(",", ":")).encode("ascii")
    ).hexdigest()


def _projection_id(axes: Sequence[tuple[str, Sequence[int]]]) -> str:
    def encode(codes: Sequence[int]) -> object:
        if codes and codes[-1] - codes[0] + 1 == len(codes):
            return {"range": [codes[0], codes[-1]]}
        return list(codes)

    return _fingerprint([(slug, encode(codes)) for slug, codes in sorted(axes)])


@dataclass(frozen=True, init=False, slots=True)
class Selection:
    """Select by dimension slug and native integer member codes.

    Codes are sorted into file order. Omitting an axis hides its column while
    keeping its rows. An empty code sequence produces zero rows.
    """

    _axes: tuple[tuple[str, tuple[int, ...]], ...]

    def __init__(self, codes: SelectionMapping) -> None:
        if not isinstance(codes, Mapping):
            raise ValueError("selection must map dimension slugs to member codes")  # noqa: TRY004
        axes = []
        for slug, values in codes.items():
            if not isinstance(slug, str):
                raise ValueError(f"invalid dimension slug {slug!r}")  # noqa: TRY004
            if not isinstance(values, Sequence) or isinstance(values, (str, bytes)):
                raise ValueError(  # noqa: TRY004
                    f"dimension {slug}: member codes must be a sequence of integers"
                )
            if any(
                not (type(code) is int or isinstance(code, IntEnum)) or code < 0
                for code in values
            ) or len(set(values)) != len(values):
                raise ValueError(
                    f"dimension {slug}: invalid or duplicate codes {values!r}"
                )
            axes.append((slug, tuple(sorted(map(int, values)))))
        object.__setattr__(self, "_axes", tuple(sorted(axes)))

    @property
    def selection_id(self) -> str:
        """Stable fingerprint of the normalized mapping."""
        return _projection_id(self._axes)

    def to_dict(self) -> dict[str, list[int]]:
        """Return a plain mapping suitable for JSON serialization."""
        return {slug: list(codes) for slug, codes in self._axes}

    @classmethod
    def from_dict(cls, codes: SelectionMapping) -> Selection:
        return cls(codes)

    def to_json(self) -> str:
        return json.dumps(self.to_dict(), ensure_ascii=True)

    @classmethod
    def from_json(cls, text: str) -> Selection:
        return cls.from_dict(json.loads(text))

    def validate(self, ivt: Ivt) -> None:
        """Check slugs and member codes against the source without decoding."""
        _selected_axes(ivt.layout, self)


SelectionInput: TypeAlias = Selection | SelectionMapping | None


def _selected_axes(
    layout: Layout, selection: SelectionInput
) -> list[tuple[str, Sequence[int] | None]]:
    counts = dict(zip(layout.slugs, layout.counts, strict=True))
    if selection is None:
        return [(slug, range(1, count + 1)) for slug, count in counts.items()]
    selected = dict(
        (selection if isinstance(selection, Selection) else Selection(selection))._axes
    )
    for slug, codes in selected.items():
        if slug not in counts:
            raise ValueError(
                f"unknown dimension {slug!r}; expected one of {list(counts)!r}"
            )
        if codes and (codes[0] < 1 or codes[-1] > counts[slug]):
            raise ValueError(
                f"dimension {slug}: invalid codes {codes!r}; expected 1..{counts[slug]}"
            )
    return [(slug, selected.get(slug)) for slug in counts]
