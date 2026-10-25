"""Measure full metadata extraction, including pyivt's lazy fields."""


def test_metadata(run_benchmark):
    record = run_benchmark("metadata")
    assert all(sample["dimensions"] > 0 for sample in record["samples"])
