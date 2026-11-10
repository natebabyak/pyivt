"""Real version-two tables against pinned independent full-grid references."""

import csv
import gzip
import hashlib
import json
import struct
from math import prod
from pathlib import Path

import pyarrow.compute as pc
import pyivt
import pytest
from arrow_helpers import expanded_table
from conftest import DATA_DIR

ROOT = Path(__file__).parent.parent
MANIFEST = json.loads((ROOT / "tests/corpus/legacy.json").read_text())
BUSINESS = {"PROVIND", "PROVSIC3", "PROVSIC3-1", "PRVNAIC1"}


def _list(value):
    return value if isinstance(value, list) else [value]


def _metadata(ivt):
    meta = ivt.metadata
    return (
        meta.signature,
        meta.version,
        meta.product_id,
        meta.title.en,
        meta.title.fr,
        meta.number_of_datapoints,
        meta.frequency,
        meta.start_date,
        meta.end_date,
        [
            (
                dim.name.en,
                dim.name.fr,
                dim.items,
                dim.type,
                dim.description.en,
                dim.description.fr,
                dim.members.en,
                dim.members.fr,
                dim.notes.en,
                dim.notes.fr,
            )
            for dim in meta.dimensions
        ],
        [
            (v.symbol, v.code, v.description.en, v.description.fr)
            for v in meta.missing_values
        ],
        [(v.symbol, v.description.en, v.description.fr) for v in meta.footnotes],
    )


@pytest.mark.parametrize("entry", MANIFEST["files"], ids=lambda e: e["filename"])
def test_real_v2_path_bytes_metadata_members_and_every_value(entry):
    path = DATA_DIR / f"{entry['sha256']}.ivt"
    assert path.is_file(), (
        "Run scripts/download_test_inputs.py --manifest tests/corpus/legacy.json"
    )
    data = path.read_bytes()
    assert len(data) == entry["source_size"]
    assert hashlib.sha256(data).hexdigest() == entry["sha256"]
    assert data[:4] == bytes.fromhex("02002000")
    oracle_path = ROOT / entry["oracle"]
    assert (
        hashlib.sha256(oracle_path.read_bytes()).hexdigest() == entry["oracle_sha256"]
    )
    metadata_path = oracle_path.with_name(oracle_path.name.replace(".csv.gz", ".json"))
    assert (
        hashlib.sha256(metadata_path.read_bytes()).hexdigest()
        == entry["metadata_oracle_sha256"]
    )
    oracle_meta = json.loads(metadata_path.read_text(encoding="utf-8"))["metadata"]
    with gzip.open(oracle_path, "rt", encoding="utf-8", newline="") as source:
        rows = list(csv.DictReader(source))
    file, memory = pyivt.open(path), pyivt.open(data)
    assert _metadata(file) == _metadata(memory)
    assert [d.items for d in file.metadata.dimensions] == oracle_meta[
        "dimension_counts"
    ]
    expected_count = prod(oracle_meta["dimension_counts"])
    assert file.metadata.number_of_datapoints == len(rows) == expected_count
    table = file.to_arrow()
    assert table.equals(memory.to_arrow(), check_metadata=True)
    assert table.num_rows == expected_count
    lookups = {}
    for language in ("en", "fr"):
        members = file.to_member_table(language=language)
        assert members.equals(
            memory.to_member_table(language=language), check_metadata=True
        )
        # The original PROVSIC3 total has no French Etiquette value.
        missing_labels = (
            1 if Path(entry["filename"]).stem == "PROVSIC3" and language == "fr" else 0
        )
        assert members["label"].null_count == missing_labels
        lookups[language] = {
            dim.slug: [
                r["label"] for r in members.to_pylist() if r["dimension"] == dim.slug
            ]
            for dim in file.metadata.dimensions
        }
    stem = Path(entry["filename"]).stem
    if stem in BUSINESS:
        # The reference reader's business labels are provisional. Joining on
        # them proves numerical agreement for that assignment; native exports
        # are a separate check of the assignment itself.
        expected = {}
        for row in rows:
            coords = [
                int(v) for k, v in row.items() if k not in {"value", "symbol", "status"}
            ]
            key = tuple(
                _list(dim["members"])[code - 1].strip()
                for dim, code in zip(oracle_meta["dimensions"], coords, strict=True)
            )
            assert key not in expected
            expected[key] = float(row["value"]) if row["value"] else None
        actual = {}
        for row in table.to_pylist():
            key = tuple(
                lookups["en"][slug][row[slug] - 1].strip()
                for slug in file._layout.slugs
            )
            assert key not in actual
            actual[key] = row["value"]
        assert actual.keys() == expected.keys()
        for key, value in actual.items():
            assert (
                value == pytest.approx(expected[key], rel=0, abs=1e-8)
                if expected[key] is not None
                else value is None
            )
    else:
        for actual, expected in zip(table.to_pylist(), rows, strict=True):
            coordinates = [
                int(v)
                for k, v in expected.items()
                if k not in {"value", "symbol", "status"}
            ]
            assert [actual[slug] for slug in file._layout.slugs] == coordinates
            value = float(expected["value"]) if expected["value"] else None
            assert (
                actual["value"] == pytest.approx(value, rel=0, abs=1e-8)
                if value is not None
                else actual["value"] is None
            )
        if stem != "EMPLOY1":
            for dim, reference in zip(
                file.metadata.dimensions, oracle_meta["dimensions"], strict=True
            ):
                for language, field in [("en", "members"), ("fr", "members_fr")]:
                    if reference[field] is not None:
                        actual = [
                            label.strip().removesuffix("-01-01")
                            for label in lookups[language][dim.slug]
                        ]
                        assert actual == [
                            label.strip() for label in _list(reference[field])
                        ]
    detailed = expanded_table(file)
    assert detailed["value"].equals(table["value"])
    assert detailed.equals(expanded_table(memory), check_metadata=True)
    if stem == "00060208":
        statuses = detailed["status_code"].to_pylist()
        assert statuses.count(-1) == 8
        assert statuses == [-1 if v is None else 0 for v in table["value"].to_pylist()]
    if stem == "00060117":
        directory, _, count, _ = struct.unpack_from("<IIIH", data, 558)
        offsets = [
            struct.unpack_from("<I", data, directory + 8 * i)[0] for i in range(count)
        ]
        assert {data[offset] & 15 for offset in offsets if offset} == {4, 8}
    slug = file._layout.slugs[-1]
    selected = file.to_arrow(**{slug: [1, file._layout.counts[-1]]})
    expected = table.filter(pc.is_in(table[slug], value_set=selected[slug].unique()))
    assert selected.equals(expected, check_metadata=True)


def test_time_holes_preserve_public_codes_and_nonchronological_dates():
    entry = next(e for e in MANIFEST["files"] if e["filename"] == "tb611996.ivt")
    ivt = pyivt.open(DATA_DIR / f"{entry['sha256']}.ivt")
    members = ivt.to_member_table().to_pylist()
    assert [
        (r["code"], r["label"]) for r in members if r["dimension"] == "annual_rate"
    ] == [(1, "1996-01-01"), (2, "1997-01-01"), (3, "1995-01-01")]
    assert ivt.to_arrow(cause=1, sex=1, region=1)["value"].to_pylist() == [
        860.5,
        843.9,
        877.0,
    ]


def test_legacy_native_titles_and_display_references():
    entry = next(e for e in MANIFEST["files"] if e["filename"].upper() == "EMPLOY1.IVT")
    ivt = pyivt.open(DATA_DIR / f"{entry['sha256']}.ivt")
    # canivt's reference mixes French into its English labels here. The native
    # bilingual summary matrices explicitly identify these display fields.
    assert [(d.name.en, d.name.fr) for d in ivt.metadata.dimensions] == [
        ("UNITS", "UNITÉS"),
        ("TRAINING", "FORMATION"),
        ("GEOGRAPHY", "GÉOGRAPHIE"),
        ("EMPL. STATUS", "STATUT D'EMPLOI"),
        ("YEAR", "ANNÉE"),
    ]
    assert ivt.to_member_table()["label"].to_pylist()[:5] == [
        "Participants",
        "Participation Rates (%)",
        "Total",
        "Job Related",
        "Personal Interest",
    ]


@pytest.mark.parametrize("name", ["00060104", "tb611996", "PROVSIC3"])
def test_native_browser_csv_every_exported_cell(name):
    references = json.loads(
        (ROOT / "tests/fixtures/legacy/browser-exports.json").read_text()
    )
    reference = next(r for r in references["files"] if Path(r["input"]).stem == name)
    path = ROOT / "tests/fixtures/legacy" / reference["export"]
    assert hashlib.sha256(path.read_bytes()).hexdigest() == reference["sha256"]
    entry = next(e for e in MANIFEST["files"] if Path(e["filename"]).stem == name)
    ivt = pyivt.open(DATA_DIR / f"{entry['sha256']}.ivt")
    members = ivt.to_member_table().to_pylist()

    def codes(slug):
        return {
            r["label"].strip().removesuffix("-01-01"): r["code"]
            for r in members
            if r["dimension"] == slug
        }

    if name == "00060104":
        row_slug, column_slug = "region", "periods"
        selected = ivt.to_arrow(quantifier=1)
    elif name == "tb611996":
        row_slug, column_slug = "region", "annual_rate"
        selected = ivt.to_arrow(cause=1, sex=1)
    else:
        row_slug, column_slug = "industry_groups", "empclass"
        selected = ivt.to_arrow(prov_can=codes("prov_can")["Newfoundland"])
    row_codes, column_codes = codes(row_slug), codes(column_slug)
    actual = {(r[row_slug], r[column_slug]): r["value"] for r in selected.to_pylist()}
    with path.open(encoding="cp1252", newline="") as source:
        rows = list(csv.reader(source))
    assert len(actual) == reference["cells"]
    expected = {}
    for row in rows[1:]:
        for label, value in zip(rows[0][1:], row[1:], strict=True):
            key = row_codes[row[0].strip()], column_codes[label.strip()]
            assert key not in expected
            expected[key] = float(value) if value else None
    assert actual == expected
