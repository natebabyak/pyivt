"""Independent coordinate/value/label checks on frozen 3- and 4-axis tables."""

import csv
import gzip
import hashlib
import json
import re
from math import prod
from pathlib import Path

import pyivt
import pytest

ROOT = Path(__file__).parent / "fixtures" / "heldout"


@pytest.mark.parametrize("product", ["98100036", "98100042"])
def test_frozen_heldout_table_matches_every_official_csv_cell(product):
    identity = json.loads((ROOT / f"{product}.json").read_text())
    for kind in ["ivt", "csv"]:
        content = (
            gzip.decompress((ROOT / f"{product}.csv.gz").read_bytes())
            if kind == "csv"
            else (ROOT / f"{product}.ivt").read_bytes()
        )
        assert hashlib.sha256(content).hexdigest() == identity[f"{kind}_sha256"]
    counts = identity["official_dimension_counts"]
    with gzip.open(
        ROOT / f"{product}.csv.gz", "rt", encoding="utf-8-sig", newline=""
    ) as source:
        reader = csv.reader(source)
        header = next(reader)
        rows = [row for row in reader if row]
    coordinate = header.index("Coordinate")
    first_value = coordinate + 1
    assert header[:3] == ["REF_DATE", "GEO", "DGUID"]
    assert len(header) == first_value + 2 * counts[-1]
    labels = []
    for member in range(counts[-1]):
        value_column = first_value + 2 * member
        match = re.fullmatch(r".*?:\s*(.*?)\s*\[(\d+)\]", header[value_column])
        assert match and int(match[2]) == member + 1
        assert header[value_column + 1] in {"Symbol", "Symbols"}
        labels.append(match[1])
    expected = {}
    strides = [prod(counts[axis + 1 :]) for axis in range(len(counts))]
    for row in rows:
        assert len(row) == len(header)
        coords = [int(part) for part in row[coordinate].split(".")]
        assert len(coords) == len(counts) - 1
        for member in range(counts[-1]):
            ids = [*coords, member + 1]
            assert all(
                1 <= value <= count for value, count in zip(ids, counts, strict=True)
            )
            position = sum(
                (value - 1) * stride for value, stride in zip(ids, strides, strict=True)
            )
            assert position not in expected
            index = first_value + 2 * member
            expected[position] = (
                ids,
                float(row[index]) if row[index] else None,
                row[index + 1] or None,
                row[1],
                row[2] or None,
                [*row[3:coordinate], labels[member]],
            )
    assert sorted(expected) == list(range(prod(counts)))
    ivt = pyivt.open(ROOT / f"{product}.ivt")
    assert ivt.layout.counts == counts
    table = ivt.to_arrow()
    assert table.num_rows == prod(counts)
    actual_ids = [table[slug].to_pylist() for slug in ivt.layout.slugs]
    actual_labels = [
        table[f"{slug}_english_desc"].to_pylist() for slug in ivt.layout.slugs
    ]
    values, symbols, dguids = [
        table[name].to_pylist() for name in ["value", "symbol", "geo_dguid_en"]
    ]
    for position, (
        ids,
        value,
        symbol,
        geo_label,
        dguid,
        member_labels,
    ) in expected.items():
        assert [axis[position] for axis in actual_ids] == ids
        assert values[position] == value
        assert symbols[position] == symbol
        assert actual_labels[0][position].strip() == geo_label.strip()
        assert dguids[position] == dguid
        assert [axis[position].strip() for axis in actual_labels[1:]] == [
            label.strip() for label in member_labels
        ]
