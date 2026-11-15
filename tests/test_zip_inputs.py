"""Real StatCan ZIP input, independent CSV values, and archive ownership."""

import csv
import gc
import hashlib
import json
import sys
from io import BytesIO
from pathlib import Path
from zipfile import ZIP_STORED, ZipFile

import pyivt
import pytest
from pyivt import _open

from scripts.ivt_synthetic import cube

ROOT = Path(__file__).parent / "fixtures" / "zip"
IDENTITY = json.loads((Path(__file__).parent / "corpus" / "zip.json").read_text())


def _zip(members):
    output = BytesIO()
    with ZipFile(output, "w", compression=ZIP_STORED) as archive:
        for name, content in members:
            archive.writestr(name, content)
    return output.getvalue()


def _metadata(metadata):
    text_fields = ("title", "notes", "author", "category", "keywords")
    return {
        "text": [
            (getattr(metadata, name).en, getattr(metadata, name).fr)
            for name in text_fields
        ],
        "dimensions": [
            (
                dim.name.en,
                dim.name.fr,
                dim.slug,
                dim.items,
                dim.type,
                dim.members.en,
                dim.members.fr,
                dim.notes.en,
                dim.notes.fr,
            )
            for dim in metadata.dimensions
        ],
        "signature": metadata.signature,
        "datapoints": metadata.number_of_datapoints,
    }


@pytest.mark.parametrize("route", ["path", "bytes", "bytearray", "memoryview", "file"])
def test_real_statcan_zip_matches_extracted_ivt_and_every_official_csv_cell(route):
    path = ROOT / "98100001.zip"
    content = path.read_bytes()
    assert hashlib.sha256(content).hexdigest() == IDENTITY["zip_sha256"]
    with ZipFile(BytesIO(content)) as archive:
        assert archive.testzip() is None
        extracted = archive.read(IDENTITY["member"])
    assert hashlib.sha256(extracted).hexdigest() == IDENTITY["files"][0]["sha256"]
    source = {
        "path": path,
        "bytes": content,
        "bytearray": bytearray(content),
        "memoryview": memoryview(content),
        "file": BytesIO(content),
    }[route]
    ivt = pyivt.open(source)
    assert isinstance(ivt, pyivt.Ivt)
    reference = pyivt.open(extracted)
    assert _metadata(ivt.metadata) == _metadata(reference.metadata)
    assert ivt.to_member_table().equals(reference.to_member_table())
    assert ivt.to_arrow().equals(reference.to_arrow())
    csv_content = (ROOT / "98100001.csv").read_bytes()
    assert hashlib.sha256(csv_content).hexdigest() == IDENTITY["csv_sha256"]
    rows = list(csv.reader(csv_content.decode("utf-8-sig").splitlines()))
    header, rows = rows[0], [row for row in rows[1:] if row]
    assert header[3] == "Coordinate"
    expected = {
        (int(row[3]), member + 1): float(row[4 + 2 * member])
        for row in rows
        for member in range(11)
    }
    table = ivt.to_arrow().to_pydict()
    actual = dict(
        zip(
            zip(
                table["geography"],
                table["population_and_dwelling_counts_11"],
                strict=True,
            ),
            table["value"],
            strict=True,
        )
    )
    assert len(actual) == len(expected) == 154
    assert actual == expected


class _Reader:
    """A file object whose read() hands out the caller's mutable buffer."""

    def __init__(self, data):
        self.data = data

    def read(self):
        return self.data


def _ivt_routes(content, tmp_path):
    path = tmp_path / "table.ivt"
    path.write_bytes(content)
    padded = b"pad" + content + b"pad"
    return {
        "path": path,
        "bytes": content,
        "bytearray": bytearray(content),
        "memoryview": memoryview(content),
        "memoryview_slice": memoryview(padded)[3:-3],
        "memoryview_cast": memoryview(content).cast("B", (len(content),)),
        "memoryview_writable": memoryview(bytearray(content)),
        "memoryview_readonly_bytearray": memoryview(bytearray(content)).toreadonly(),
        "file": BytesIO(content),
        "zip": _zip([("table.ivt", content)]),
        "zip_bytearray": bytearray(_zip([("table.ivt", content)])),
    }


def test_every_input_route_matches_path_metadata_and_exports(tmp_path):
    content = (ROOT / "98100001.zip").read_bytes()
    with ZipFile(BytesIO(content)) as archive:
        extracted = archive.read(IDENTITY["member"])
    routes = _ivt_routes(extracted, tmp_path)
    reference = pyivt.open(routes["path"])
    for route, source in routes.items():
        ivt = pyivt.open(source)
        assert _metadata(ivt.metadata) == _metadata(reference.metadata), route
        assert ivt.to_member_table().equals(reference.to_member_table()), route
        assert ivt.to_arrow().equals(reference.to_arrow()), route
        assert ivt._legacy_to_arrow().equals(reference._legacy_to_arrow()), route


def test_immutable_bytes_are_shared_not_copied():
    content = cube((2, 3))
    for source in (content, memoryview(content)):
        references = sys.getrefcount(content)
        ivt = pyivt.open(source)
        assert sys.getrefcount(content) == references + 1
        del ivt
        gc.collect()
        assert sys.getrefcount(content) == references


@pytest.mark.parametrize(
    "view",
    [
        lambda data: data,
        memoryview,
        lambda data: memoryview(data).toreadonly(),
        lambda data: _Reader(data),
    ],
    ids=["bytearray", "memoryview", "readonly_memoryview", "file_read_bytearray"],
)
def test_mutating_bytearray_after_open_does_not_change_reader(view):
    original = cube((2, 3))
    data = bytearray(original)
    ivt = pyivt.open(view(data))
    expected = pyivt.open(original)
    data[:] = bytes(len(data))
    assert _metadata(ivt.metadata) == _metadata(expected.metadata)
    assert ivt.to_member_table().equals(expected.to_member_table())
    assert ivt.to_arrow().equals(expected.to_arrow())


def test_zip_member_selection_and_case_insensitive_extension():
    first, second = cube((2, 3)), cube((3, 4))
    payload = _zip([("nested/first.IVT", first), ("second.ivt", second)])
    with pytest.raises(ValueError, match="2 IVT members.*member="):
        pyivt.open(payload)
    assert pyivt.open(payload, member="nested/first.IVT").to_arrow().num_rows == 6
    assert pyivt.open(payload, member="second.ivt").to_arrow().num_rows == 12
    with pytest.raises(ValueError, match="exactly one IVT member"):
        pyivt.open(payload, member="FIRST.IVT")
    assert pyivt.open(_zip([("nested/only.IVT", first)])).to_arrow().num_rows == 6


def test_zip_rejects_absent_or_duplicate_members_and_non_zip_selection():
    with pytest.raises(ValueError, match="0 IVT members"):
        pyivt.open(_zip([("readme.txt", b"no tables")]))
    with pytest.raises(ValueError, match="0 IVT members"):
        pyivt.open(_zip([]))
    with pytest.warns(UserWarning, match="Duplicate name"):
        payload = _zip([("same.ivt", cube((2, 3))), ("same.ivt", cube((3, 4)))])
    with pytest.raises(ValueError, match="exactly one IVT member"):
        pyivt.open(payload, member="same.ivt")
    with pytest.raises(ValueError, match="only to ZIP"):
        pyivt.open(cube((2, 3)), member="table.ivt")
    with pytest.raises(TypeError, match="member must be"):
        pyivt.open(payload, member=2)


def test_zip_temporary_file_survives_reader_and_lazy_metadata(monkeypatch):
    directories = []
    original = _open.TemporaryDirectory

    def temporary_directory(**kwargs):
        owner = original(**kwargs)
        directories.append(Path(owner.name))
        return owner

    monkeypatch.setattr(_open, "TemporaryDirectory", temporary_directory)
    ivt = pyivt.open(ROOT / "98100001.zip")
    directory = directories[0]
    assert (directory / "table.ivt").exists()
    metadata = ivt.metadata
    assert not metadata._loaded("dimensions")
    del ivt
    gc.collect()
    assert directory.exists()
    assert len(metadata.dimensions) == 2
    assert metadata.dimensions[0].members.en[0] == "Canada"
    del metadata
    gc.collect()
    assert not directory.exists()


def test_zip_crc_failure_and_native_parse_failure_cleanup(monkeypatch):
    directories = []
    original = _open.TemporaryDirectory

    def temporary_directory(**kwargs):
        owner = original(**kwargs)
        directories.append(Path(owner.name))
        return owner

    monkeypatch.setattr(_open, "TemporaryDirectory", temporary_directory)
    data = bytearray(_zip([("table.ivt", cube((2, 3)))]))
    data[30 + len("table.ivt") + 100] ^= 1
    with pytest.raises(ValueError, match="CRC"):
        pyivt.open(bytes(data))
    with pytest.raises(ValueError):
        pyivt.open(_zip([("table.ivt", b"invalid")]))
    assert len(directories) == 2
    assert all(not directory.exists() for directory in directories)


def test_zip_binary_file_read_once_from_current_position():
    payload = (ROOT / "98100001.zip").read_bytes()

    class NonSeekable:
        def __init__(self):
            self.calls = 0

        def read(self):
            self.calls += 1
            return payload

    source = NonSeekable()
    assert pyivt.open(source).to_arrow().num_rows == 154
    assert source.calls == 1
    stream = BytesIO(b"prefix" + payload)
    stream.seek(6)
    assert pyivt.open(stream).to_arrow().num_rows == 154
    assert not stream.closed
