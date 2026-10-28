"""Integration tests for IVT metadata against Beyond File Summary goldens."""

from __future__ import annotations

import json
import re
import struct
from datetime import date, datetime, timezone
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
    """Normalize Beyond's blank fields to the Python optional-value contract."""
    result = {key: expect[key] for key in COMPARED_KEYS}
    for key in ("notes_en", "notes_fr"):
        result[key] = _notes_sentences(result[key])
    for key in ("author_en", "author_fr", "category_en", "category_fr"):
        result[key] = result[key] or None
    frequency = result["frequency"]
    result["frequency"] = (
        frequency.lower().replace("-", "_") if frequency and frequency != "No" else None
    )
    return result


def _notes_sentences(text: str | None) -> tuple[str, ...] | None:
    """Keep only the first and last sentences for notes comparison."""
    if text is None or not text.strip():
        return None
    sentences = re.split(r"(?<=[.!?])\s+", text.strip())
    return (sentences[0], sentences[-1])


def _keywords_list(text: str | None) -> list[str]:
    if not text or not text.strip():
        return []
    return [part.strip() for part in text.split(",") if part.strip()]


def _footnotes_beyond(meta: IvtMetadata) -> list[dict]:
    """Project localized legend entries into Beyond's description_* shape."""
    return [
        {
            "symbol": footnote.symbol,
            "description_en": footnote.description.en,
            "description_fr": footnote.description.fr,
        }
        for footnote in meta.footnotes
    ]


def _missing_values_beyond(meta: IvtMetadata) -> list[dict]:
    return [
        {
            "symbol": missing.symbol,
            "description_en": missing.description.en,
            "description_fr": missing.description.fr,
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
                "items": dimension.items,
                "type": {
                    "geographic": "Geographic",
                    "normal": "Normal",
                    "time_series": "Time Series",
                }[dimension.type],
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
    assert prod(d.items for d in meta.dimensions) == meta.number_of_datapoints


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
    english = localized.en
    assert english is not None
    english.append("changed")
    indexed_english = localized["en"]
    assert indexed_english is not None
    indexed_english.clear()
    assert localized.en == ["English"]


def test_metadata_localized_access(demo_ivt):
    meta = open_ivt(demo_ivt.path).metadata
    fields = [meta.title, meta.author, meta.category, meta.keywords, meta.notes]
    for dimension in meta.dimensions:
        fields.extend(
            [dimension.name, dimension.description, dimension.members, dimension.notes]
        )
        assert dimension.name.en is not None
        assert dimension.type in ("geographic", "normal", "time_series")
        assert dimension.items == dimension.count
        with pytest.raises(AttributeError):
            dimension.items = 0
        assert not hasattr(dimension, "type_label")
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
    fields.extend(missing.description for missing in meta.missing_values)
    fields.extend(note.description for note in meta.footnotes)
    for note in meta.footnotes:
        assert not hasattr(note, "text")
        assert not hasattr(note, "language")
    for missing in meta.missing_values:
        assert not hasattr(missing, "label")
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


@pytest.fixture
def metadata_bytes():
    path = next((Path(__file__).parent.parent / "data").glob("625ff*.ivt"), None)
    if path is None:
        pytest.skip("small census fixture is not available")
    return path.read_bytes()


def _replace_records(data: bytes, slot: int, records: dict[int, bytes]) -> bytes:
    """Relocate a fixture's flat directory, preserving holes and entry ordinals."""
    changed = bytearray(data)
    offset, _, count, flags = struct.unpack_from("<IIIH", changed, slot)
    assert flags & 1 == 0
    entries = bytearray(changed[offset : offset + count * 8])
    count = max(count, max(records) + 1)
    entries.extend(bytes(count * 8 - len(entries)))
    for index, payload in records.items():
        struct.pack_into(
            "<IHH", entries, index * 8, len(changed), len(payload), len(payload)
        )
        changed.extend(payload)
    struct.pack_into("<IIIH", changed, slot, len(changed), count, count, flags)
    changed.extend(entries)
    return bytes(changed)


def _text_record(text: bytes) -> bytes:
    return struct.pack("<BBH", 1, 1, len(text)) + text


def _keyword_record(terms: list[bytes]) -> bytes:
    payload = struct.pack("<h", len(terms))
    for term in terms:
        payload += struct.pack("<B", len(term)) + term + b"\0"
    return _text_record(payload)


def _zero_suppress(record: bytes) -> bytes:
    """Encode a byte component with the native zero-suppression bitmap."""
    header = 2 + record[1] * 2
    body = record[header:]
    bitmap = bytearray(((len(body) + 15) // 16) * 2)
    for i, byte in enumerate(body):
        if byte:
            word = struct.unpack_from("<H", bitmap, (i // 16) * 2)[0]
            struct.pack_into("<H", bitmap, (i // 16) * 2, word | (0x8000 >> (i % 16)))
    return (
        bytes([record[0] | 0x80])
        + record[1:header]
        + bitmap
        + bytes(filter(None, body))
    )


@pytest.mark.parametrize("compressed", [False, True])
def test_keywords_preserve_all_translations(metadata_bytes, tmp_path, compressed):
    english = _keyword_record(
        [
            b" not in the labour force",
            b"participation rate",
            b"unemployment rate",
            b"employment rate",
            b"industry",
            b"class of worker",
        ]
    )
    french = _keyword_record([b"emploi", b"ch\xf4mage", b"soci\xe9t\xe9"])
    if compressed:
        english, french = _zero_suppress(english), _zero_suppress(french)
    matrix = struct.pack("<BBHH", 1, 2, 2, 32)
    # Reversed language rows and a French record beyond the original directory.
    for language, facet, notes, keywords in [(2, 9, 10, 11), (1, 0, 3, 4)]:
        matrix += struct.pack("<8I", language, facet, notes, keywords, 0, 0, 0, 0)
    data = _replace_records(metadata_bytes, 544, {4: english, 7: matrix, 11: french})
    path = tmp_path / "keywords.ivt"
    path.write_bytes(data)
    for source in (data, path):
        keywords = open_ivt(source).metadata.keywords
        assert keywords.en == (
            "not in the labour force, participation rate, unemployment rate, "
            "employment rate, industry, class of worker"
        )
        assert keywords["fr"] == "emploi, chômage, société"


def test_keywords_observed_native_record(metadata_bytes, tmp_path):
    # Captured master entry 4 (and French entry 11) in 98-400-X2016112.IVT.
    record = bytes.fromhex("0101130001000f39382d3430302d583230313631313200")
    languages = struct.pack("<BBHH", 1, 2, 2, 32)
    for language in (1, 2):
        languages += struct.pack("<8I", language, 0, 3, 4, 0, 0, 0, 0)
    data = _replace_records(metadata_bytes, 544, {4: record, 7: languages})
    path = tmp_path / "native-keywords.ivt"
    path.write_bytes(data)
    for source in (data, path):
        keywords = open_ivt(source).metadata.keywords
        assert _keywords_list(keywords.en) == ["98-400-X2016112"]
        assert _keywords_list(keywords.fr) == ["98-400-X2016112"]


def test_blank_summary_fields_are_none(metadata_bytes, tmp_path):
    facet = bytearray(_text_record(bytes(0x304)))
    facet[4:11] = b"FACET04"
    facet[11:16] = b"Title"
    facet[266:269] = b" \t "
    facet[521:524] = b" \r "
    data = _replace_records(
        metadata_bytes, 544, {0: bytes(facet), 4: _keyword_record([b" \t "])}
    )
    path = tmp_path / "blank.ivt"
    path.write_bytes(data)
    for source in (data, path):
        meta = open_ivt(source).metadata
        assert meta.author.en is None
        assert meta.author.fr == "Statistique Canada"
        assert meta.category.en is None
        assert meta.category.fr is None
        assert meta.keywords.en is None
        assert meta.keywords.fr is None


@pytest.mark.parametrize("compressed", [False, True])
@pytest.mark.parametrize(
    "text, expected",
    [
        (b"", None),
        (b"\0", None),
        (b" \t\r\n \xa0\0padding", None),
        (
            b"  First line. \r\n \r Second line. \t\0ignored",
            "First line.\n\nSecond line.",
        ),
    ],
)
def test_notes_normalize_consistently(
    metadata_bytes, tmp_path, text, expected, compressed
):
    record = _text_record(text)
    if compressed:
        record = _zero_suppress(record)
    # Both table language rows reference notes entry 3. Dimension language rows
    # reference notes with a u16 at +22; nonzero references may point to blanks.
    languages = struct.pack("<BBHH", 1, 2, 2, 32)
    for language in (1, 2):
        languages += struct.pack("<8I", language, 0, 3, 0, 0, 0, 0, 0)
    data = _replace_records(metadata_bytes, 544, {3: record, 7: languages})
    rows = bytearray(struct.pack("<BBHH", 1, 2, 2, 86) + bytes(2 * 86))
    for start in (6, 92):
        struct.pack_into("<H", rows, start + 22, 10)
    data = _replace_records(data, 824, {3: bytes(rows), 10: record})
    path = tmp_path / "notes.ivt"
    path.write_bytes(data)
    for source in (data, path):
        meta = open_ivt(source).metadata
        for notes in (meta.notes, meta.dimensions[0].notes):
            assert notes.en == expected
            assert notes.fr == expected


@pytest.mark.parametrize("compressed", [False, True])
@pytest.mark.parametrize(
    "rate, expected",
    [
        (0, None),
        (1, "annual"),
        (2, "semi_annual"),
        (4, "quarterly"),
        (12, "monthly"),
        (3, None),
    ],
)
def test_frequency_uses_native_rate(
    metadata_bytes, tmp_path, rate, expected, compressed
):
    # Master record 2 is the native 32-byte table settings vector. Its periods
    # per year word is at +6 in the payload; date serials follow at +8 and +12.
    settings = bytearray(_text_record(bytes(32)))
    struct.pack_into("<H", settings, 10, rate)
    start = date(2020, 1, 1)
    end = date(2021, 1, 1)

    def serial(value):
        return (value - date(1970, 1, 1)).days + 719_469

    struct.pack_into("<II", settings, 12, serial(start), serial(end))
    record = _zero_suppress(settings) if compressed else bytes(settings)
    data = _replace_records(metadata_bytes, 544, {2: record})
    path = tmp_path / "frequency.ivt"
    path.write_bytes(data)
    for source in (data, path):
        meta = open_ivt(source).metadata
        assert meta.frequency == expected
        assert meta.start_date == (start.isoformat() if rate else None)
        assert meta.end_date == (end.isoformat() if rate else None)


def test_frequency_absent_or_malformed_is_none(metadata_bytes):
    for record in (b"", _text_record(b"short")):
        meta = open_ivt(_replace_records(metadata_bytes, 544, {2: record})).metadata
        assert meta.frequency is None
        assert meta.start_date is None
        assert meta.end_date is None


@pytest.mark.parametrize("missing_language", ["en", "fr"])
def test_footnote_missing_translation_is_none(
    metadata_bytes, tmp_path, missing_language
):
    original = open_ivt(metadata_bytes).metadata.footnotes
    directory = struct.unpack_from("<I", metadata_bytes, 712)[0]
    offset, length = struct.unpack_from("<IH", metadata_bytes, directory)
    index = bytearray(metadata_bytes[offset : offset + length])
    struct.pack_into("<I", index, 6 + (4 if missing_language == "fr" else 0), 0)
    data = _replace_records(metadata_bytes, 712, {0: index})
    path = tmp_path / "footnotes.ivt"
    path.write_bytes(data)
    present_language = "fr" if missing_language == "en" else "en"
    for source in (data, path):
        notes = open_ivt(source).metadata.footnotes
        assert len(notes) == len(original)
        assert notes[0].symbol == original[0].symbol
        assert notes[0].number == original[0].number
        assert notes[0].description[missing_language] is None
        assert (
            notes[0].description[present_language]
            == original[0].description[present_language]
        )
