"""Measure path-to-Arrow conversion without serialization or cached readers."""


def test_arrow(run_benchmark):
    record = run_benchmark("arrow")
    for sample in record["samples"]:
        assert sample["rows"] > 0
        assert sample["columns"] > 0
