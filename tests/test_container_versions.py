"""Container support is explicit and identical for path and bytes sources."""

import struct

import pyivt
import pytest

from scripts.ivt_synthetic import cube


@pytest.mark.parametrize("version", [2, 3])
@pytest.mark.parametrize("source", ["bytes", "path"])
def test_unsupported_container_versions_raise_clear_errors(version, source, tmp_path):
    data = bytearray(cube((2, 3)))
    struct.pack_into("<H", data, 0, version)
    path = tmp_path / f"v{version}.ivt"
    path.write_bytes(data)
    with pytest.raises(
        ValueError, match=f"container version {version}; only version 4"
    ):
        pyivt.open(path if source == "path" else bytes(data))


def test_supported_version_four_fixture():
    ivt = pyivt.open(cube((2, 3)))
    assert ivt.metadata.signature == "04002000"
    assert ivt.to_arrow().num_rows == 6
