"""Fixture downloads recover from transient failures without hiding missing inputs."""

import hashlib
import json
from io import BytesIO
from unittest.mock import MagicMock, patch
from urllib.error import HTTPError
from zipfile import ZipFile

import pytest

from scripts import download_test_inputs as downloader


def response(content):
    result = MagicMock()
    result.__enter__.return_value.read.return_value = content
    return result


def test_download_retries_read_timeout():
    failed = response(b"")
    failed.__enter__.return_value.read.side_effect = TimeoutError("read timed out")
    with (
        patch.object(
            downloader, "urlopen", side_effect=[failed, response(b"ok")]
        ) as open_url,
        patch.object(downloader.time, "sleep") as sleep,
    ):
        assert downloader.download("https://example.com/input.zip") == b"ok"
    assert open_url.call_count == 2
    sleep.assert_called_once_with(1)


@pytest.mark.parametrize("status,attempts", [(404, 1), (403, 1), (503, 3), (429, 3)])
def test_download_http_retry_policy(status, attempts):
    error = HTTPError("https://example.com", status, "error", {}, None)
    with (
        patch.object(downloader, "urlopen", side_effect=error) as open_url,
        patch.object(downloader.time, "sleep"),
        pytest.raises(HTTPError),
    ):
        downloader.download("https://example.com")
    assert open_url.call_count == attempts


def test_exhausted_downloads_still_fail_when_fixtures_are_missing(
    tmp_path, monkeypatch
):
    monkeypatch.setattr(downloader, "DATA_DIR", tmp_path)
    monkeypatch.setattr("sys.argv", ["download_test_inputs.py", "--number", "1"])
    with (
        patch.object(
            downloader,
            "download",
            side_effect=[b'[{"productId": 98100001}]', TimeoutError("read timed out")],
        ),
        pytest.raises(SystemExit) as error,
    ):
        downloader.main()
    assert error.value.code == 1


def test_unavailable_candidate_does_not_prevent_later_download(tmp_path, monkeypatch):
    monkeypatch.setattr(downloader, "DATA_DIR", tmp_path)
    monkeypatch.setattr("sys.argv", ["download_test_inputs.py", "--number", "1"])
    archive_bytes = BytesIO()
    with ZipFile(archive_bytes, "w") as archive:
        archive.writestr("table.ivt", b"fixture")
    with patch.object(
        downloader,
        "download",
        side_effect=[
            b'[{"productId": 98100001}, {"productId": 98100002}]',
            TimeoutError("read timed out"),
            archive_bytes.getvalue(),
        ],
    ):
        assert downloader.main() == 0
    files = list(tmp_path.glob("*.ivt"))
    assert len(files) == 1
    assert files[0].read_bytes() == b"fixture"


def test_manifest_fetch_verifies_hash_and_reuses_cached_file(tmp_path):
    content = b"pinned IVT"
    digest = hashlib.sha256(content).hexdigest()
    manifest = tmp_path / "manifest.json"
    manifest.write_text(
        json.dumps(
            {"files": [{"sha256": digest, "url": "https://example.com/table.zip"}]}
        )
    )
    payload = BytesIO()
    with ZipFile(payload, "w") as archive:
        archive.writestr("nested/table.ivt", content)
    destination = tmp_path / "data"
    with patch.object(downloader, "download", return_value=payload.getvalue()) as fetch:
        downloader.fetch_manifest(manifest, destination)
        downloader.fetch_manifest(manifest, destination)
    fetch.assert_called_once()
    assert (destination / f"{digest}.ivt").read_bytes() == content


def test_manifest_refuses_changed_remote_bytes(tmp_path):
    manifest = tmp_path / "manifest.json"
    manifest.write_text(
        json.dumps(
            {"files": [{"sha256": "a" * 64, "url": "https://example.com/table.ivt"}]}
        )
    )
    with (
        patch.object(downloader, "download", return_value=b"changed"),
        pytest.raises(ValueError, match="refusing changed inputs"),
    ):
        downloader.fetch_manifest(manifest, tmp_path / "data")
