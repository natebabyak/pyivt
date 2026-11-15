"""The share re-scan records a verdict and its evidence for every file."""

import json

from scripts.ivt_synthetic import cube
from scripts.triage_files import main


def test_rescan_classifies_opened_corrupt_and_unsupported_files(tmp_path, capsys):
    root = tmp_path / "share"
    (root / "nested").mkdir(parents=True)
    good = cube((2, 3))
    (root / "good.IVT").write_bytes(good)
    (root / "nested" / "copy.ivt").write_bytes(good)
    (root / "cut.ivt").write_bytes(good[:100])
    unsupported = bytearray(good)
    unsupported[0] = 3
    (root / "v3.ivt").write_bytes(unsupported)
    (root / "notes.txt").write_text("not scanned")
    output = tmp_path / "triage.jsonl"

    assert main([str(root), "--output", str(output), "--values"]) == 0

    rows = {
        row["path"].replace("\\", "/").split("share/")[1]: row
        for row in map(json.loads, output.read_text(encoding="utf-8").splitlines())
    }
    assert sorted(rows) == ["cut.ivt", "good.IVT", "nested/copy.ivt", "v3.ivt"]
    assert rows["good.IVT"]["status"] == "opened"
    assert rows["good.IVT"]["dimensions"] == [["Axis 0", 2], ["Axis 1", 3]]
    assert rows["good.IVT"]["conversion"]["rows"] == 6
    assert rows["good.IVT"]["sha256"] == rows["nested/copy.ivt"]["sha256"]
    assert rows["cut.ivt"]["status"] == "corrupt"
    assert rows["cut.ivt"]["bytes"] == 100
    assert rows["cut.ivt"]["message"].startswith(
        "Corrupt IVT file: the file is truncated"
    )
    assert rows["v3.ivt"]["status"] == "unsupported"
    assert rows["v3.ivt"]["head"].startswith("03 00 20 00")
    assert "4 files: corrupt=1, opened=2, unsupported=1" in capsys.readouterr().err
