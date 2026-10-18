import io

import pytest
from pyivt import open as open_ivt


def test_open_rejects_unsupported_type():
    with pytest.raises(TypeError, match="path, bytes-like, or binary file"):
        open_ivt(123)  # type: ignore[arg-type]


def test_open_from_bytes_and_fileobj(statcan_ivt):
    raw = statcan_ivt.path.read_bytes()
    from_bytes = open_ivt(raw)
    from_file = open_ivt(io.BytesIO(raw))
    assert from_bytes.metadata.product_id == statcan_ivt.id
    assert from_file.metadata.product_id == statcan_ivt.id
