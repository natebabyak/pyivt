"""Integration tests for IVT metadata against Beyond File Summary goldens."""

from __future__ import annotations

import json
import re
import struct
from datetime import datetime, timezone
from math import prod
from pathlib import Path
from types import SimpleNamespace

import pytest
from pyivt import IvtMetadata, LocalizedLabels, LocalizedText
from pyivt import open as open_ivt

# All top-level Beyond File Summary keys. Dimension notes/members are exposed on
# the API but intentionally omitted from golden comparison.
COMPARED_KEYS: tuple[str, ...] = (
    "title_en",
    "title_fr",
    "file_name",
    "directory",
    "author_en",
    "author_fr",
    "category_en",
    "category_fr",
    "keywords_en",
    "keywords_fr",
    "notes_en",
    "notes_fr",
    "last_update",
    "file_size",
    "number_of_datapoints",
    "frequency",
    "start_date",
    "end_date",
    "read_only",
    "dimensions",
    "footnotes",
    "missing_values",
)


def _load_expect(case: SimpleNamespace) -> dict:
    expect = json.loads(case.expect_path.read_text(encoding="utf-8"))
    # The checkout location is not part of the hashed IVT contents.
    expect["directory"] = str(case.path.resolve().parent)
    # The copy's filesystem timestamp is not part of the hashed IVT contents.
    expect["last_update"] = datetime.fromtimestamp(
        case.path.stat().st_mtime, timezone.utc
    ).isoformat()
    return expect


def _expect_slice(expect: dict) -> dict:
    return {
        key: _notes_sentences(expect[key])
        if key in ("notes_en", "notes_fr")
        else expect[key]
        for key in COMPARED_KEYS
    }


def _notes_sentences(text: str | None) -> tuple[str, ...] | None:
    """Keep only the first and last sentences for notes comparison."""
    if text is None:
        return None
    sentences = re.split(r"(?<=[.!?])\s+", text.strip())
    return (sentences[0], sentences[-1])


def _keywords_list(text: str | None) -> list[str]:
    if not text or not text.strip():
        return []
    return [part.strip() for part in text.split(",") if part.strip()]


def _footnotes_beyond(meta: IvtMetadata) -> list[dict]:
    """Pair EN/FR footnote rows into Beyond {symbol, description_*} shape."""
    by_symbol: dict[str, dict] = {}
    order: list[str] = []
    for footnote in meta.footnotes:
        symbol = footnote.symbol
        if symbol not in by_symbol:
            by_symbol[symbol] = {
                "symbol": symbol,
                "description_en": None,
                "description_fr": None,
            }
            order.append(symbol)
        if footnote.language == "fr":
            by_symbol[symbol]["description_fr"] = footnote.text
        else:
            by_symbol[symbol]["description_en"] = footnote.text
    return [by_symbol[symbol] for symbol in order]


def _missing_values_beyond(meta: IvtMetadata) -> list[dict]:
    return [
        {
            "symbol": missing.symbol,
            "description_en": missing.label.en,
            "description_fr": missing.label.fr,
        }
        for missing in meta.missing_values
    ]


def snapshot_comparable(meta: IvtMetadata, path: Path) -> dict:
    """Build Beyond-shaped fields for every COMPARED_KEYS entry.

    Dimension ``notes`` / ``members`` are available on ``meta.dimensions``
    but are not included in the golden snapshot.
    """
    stats = path.stat()
    path = path.resolve()
    full = {
        "title_en": meta.title.en,
        "title_fr": meta.title.fr,
        "file_name": path.name,
        "directory": str(path.parent),
        "author_en": meta.author.en,
        "author_fr": meta.author.fr,
        "category_en": meta.category.en,
        "category_fr": meta.category.fr,
        "keywords_en": _keywords_list(meta.keywords.en),
        "keywords_fr": _keywords_list(meta.keywords.fr),
        "notes_en": _notes_sentences(meta.notes.en),
        "notes_fr": _notes_sentences(meta.notes.fr),
        "last_update": datetime.fromtimestamp(stats.st_mtime, timezone.utc).isoformat(),
        "file_size": stats.st_size,
        "number_of_datapoints": meta.number_of_datapoints,
        "frequency": meta.frequency,
        "start_date": meta.start_date,
        "end_date": meta.end_date,
        "read_only": meta.read_only,
        "dimensions": [
            {
                "name_en": dimension.name.en,
                "name_fr": dimension.name.fr,
                "items": dimension.count,
                "type": dimension.type_label,
                "description_en": dimension.description.en,
                "description_fr": dimension.description.fr,
            }
            for dimension in meta.dimensions
        ],
        "footnotes": _footnotes_beyond(meta),
        "missing_values": _missing_values_beyond(meta),
    }
    return {key: full[key] for key in COMPARED_KEYS}


def test_metadata_matches_expected(demo_ivt):
    expect = _load_expect(demo_ivt)
    meta = open_ivt(demo_ivt.path).metadata
    assert snapshot_comparable(meta, demo_ivt.path) == _expect_slice(expect)
    # Summary datapoints are the product of Beyond Items (may exceed decode cube).
    assert prod(d.count for d in meta.dimensions) == meta.number_of_datapoints


def test_metadata_lazy_fields(demo_ivt):
    meta = open_ivt(demo_ivt.path).metadata
    for name in ("dimensions", "geographies", "footnotes", "missing_values"):
        assert not meta._loaded(name)

    _ = meta.product_id
    _ = meta.title
    for name in ("dimensions", "geographies", "footnotes", "missing_values"):
        assert not meta._loaded(name)

    _ = meta.dimensions
    assert meta._loaded("dimensions")
    assert not meta._loaded("geographies")
    assert not meta._loaded("footnotes")
    assert not meta._loaded("missing_values")

    _ = meta.geographies
    assert meta._loaded("geographies")

    _ = meta.footnotes
    _ = meta.missing_values
    assert meta._loaded("footnotes")
    assert meta._loaded("missing_values")


def test_open_bytes_defers_decode(demo_ivt):
    expect = _load_expect(demo_ivt)
    ivt = open_ivt(demo_ivt.path.read_bytes())
    assert len(ivt.layout.counts) == len(expect["dimensions"])


def test_to_arrow_row_count(demo_ivt):
    ivt = open_ivt(demo_ivt.path)
    expected = _load_expect(demo_ivt)
    table = ivt.to_arrow()
    assert table.num_rows == expected["number_of_datapoints"]
    assert table.num_rows == prod(ivt.layout.counts)
    assert "value" in table.column_names


def test_metadata_path_and_bytes_match(demo_ivt):
    by_path = open_ivt(demo_ivt.path).metadata
    by_bytes = open_ivt(demo_ivt.path.read_bytes()).metadata
    assert snapshot_comparable(by_path, demo_ivt.path) == snapshot_comparable(
        by_bytes, demo_ivt.path
    )
    assert by_path.notes.en == by_bytes.notes.en
    assert by_path.notes.fr == by_bytes.notes.fr
    for path_dim, bytes_dim in zip(
        by_path.dimensions, by_bytes.dimensions, strict=True
    ):
        assert path_dim.members.en == bytes_dim.members.en
        assert path_dim.members.fr == bytes_dim.members.fr
        assert path_dim.notes.en == bytes_dim.notes.en
        assert path_dim.notes.fr == bytes_dim.notes.fr


@pytest.mark.parametrize(
    "localized",
    [
        LocalizedText(en="English", fr="Français"),
        LocalizedText(en="English"),
        LocalizedText(fr="Français"),
        LocalizedText(),
        LocalizedLabels(en=["English"], fr=["Français"]),
        LocalizedLabels(en=["English"]),
        LocalizedLabels(fr=["Français"]),
        LocalizedLabels(en=[], fr=[]),
        LocalizedLabels(),
    ],
)
def test_localized_access(localized):
    for language in ("en", "fr"):
        assert localized[language] == getattr(localized, language)
        with pytest.raises(AttributeError):
            setattr(localized, language, None)
    for language in ("de", "EN", "", "en_US"):
        with pytest.raises(KeyError) as exc:
            _ = localized[language]
        assert exc.value.args == (language,)
    with pytest.raises(TypeError):
        _ = localized[0]


def test_localized_labels_are_copies():
    localized = LocalizedLabels(en=["English"])
    localized.en.append("changed")
    localized["en"].clear()
    assert localized.en == ["English"]


def test_metadata_localized_access(demo_ivt):
    meta = open_ivt(demo_ivt.path).metadata
    fields = [meta.title, meta.author, meta.category, meta.keywords, meta.notes]
    for dimension in meta.dimensions:
        fields.extend(
            [dimension.name, dimension.description, dimension.members, dimension.notes]
        )
        assert dimension.name.en is not None
        for old_name in (
            "name_en",
            "name_fr",
            "description_en",
            "description_fr",
            "members_en",
            "members_fr",
            "notes_en",
            "notes_fr",
        ):
            assert not hasattr(dimension, old_name)
    fields.append(meta.geographies.geo_label)
    assert not hasattr(meta.geographies, "geo_label_fr")
    fields.extend(missing.label for missing in meta.missing_values)
    for localized in fields:
        for language in ("en", "fr"):
            assert localized[language] == getattr(localized, language)
        with pytest.raises(KeyError):
            _ = localized["de"]


@pytest.mark.parametrize("slot", [544, 698, 712, 824])
def test_paged_metadata_directory_matches_flat(slot, tmp_path):
    """The same indirection applies to master, legend, and member directories."""
    path = next((Path(__file__).parent.parent / "data").glob("625ff*.ivt"), None)
    if path is None:
        pytest.skip("small census fixture is not available")
    original = path.read_bytes()
    data = bytearray(original)
    offset, _, count, flags = struct.unpack_from("<IIIH", data, slot)
    assert flags & 1 == 0
    page_size = struct.unpack_from("<H", data, 20)[0]
    entries = data[offset : offset + count * 8]
    # Force a second page while preserving all existing entry ordinals and
    # references. Extra holes must stay holes after catalog relocation.
    count = max(count, page_size + 1)
    entries.extend(bytes(count * 8 - len(entries)))
    page_offsets = []
    for start in range(0, count, page_size):
        data.extend(bytes(11))  # Pages need not be adjacent or aligned.
        page_offsets.append(len(data))
        data.extend(entries[start * 8 : (start + page_size) * 8])
    index_offset = len(data)
    data.extend(struct.pack(f"<{len(page_offsets)}I", *page_offsets))
    struct.pack_into(
        "<IIIH",
        data,
        slot,
        index_offset,
        len(page_offsets) * page_size,
        count,
        flags | 1,
    )
    changed_path = tmp_path / "paged.ivt"
    changed_path.write_bytes(data)
    expected = snapshot_comparable(open_ivt(original).metadata, changed_path)
    for source in (changed_path, bytes(data)):
        ivt = open_ivt(source)
        assert snapshot_comparable(ivt.metadata, changed_path) == expected
        assert ivt.to_arrow().equals(open_ivt(original).to_arrow())
