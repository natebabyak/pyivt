"""Compare complete exports from two pyivt packages against a pinned corpus.

Run each input in a fresh process. Tables refused by an existing memory guard
are also exported completely in first-axis partitions. No downloads or package
installation occur here. See docs/investigations/heldout-compatibility.md.
"""

import argparse
import gc
import hashlib
import json
import subprocess
import sys
from math import prod
from pathlib import Path


def file_digest(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def schema_identity(schema):
    def metadata(value):
        return sorted((key.hex(), item.hex()) for key, item in (value or {}).items())

    return {
        "fields": [
            (field.name, str(field.type), field.nullable, metadata(field.metadata))
            for field in schema
        ],
        "metadata": metadata(schema.metadata),
    }


def arrow_digest(table):
    import pyarrow as pa
    import pyarrow.compute as pc

    schema = json.dumps(schema_identity(table.schema), sort_keys=True)
    digest = hashlib.sha256(schema.encode())
    for column in table.combine_chunks().columns:
        array = column.chunk(0)
        if pa.types.is_dictionary(array.type):
            array = array.dictionary_decode()
        digest.update(pc.is_null(array).to_numpy(zero_copy_only=False).astype("uint8"))
        if pa.types.is_string(array.type) or pa.types.is_large_string(array.type):
            digest.update(json.dumps(array.to_pylist(), ensure_ascii=False).encode())
        elif not pa.types.is_null(array.type):
            filled = pc.fill_null(array, pa.scalar(0, type=array.type))
            digest.update(filled.to_numpy(zero_copy_only=False))
    return digest.hexdigest()


def metadata_digest(metadata):
    text = {
        name: (getattr(metadata, name).en, getattr(metadata, name).fr)
        for name in ["title", "notes", "author", "category", "keywords"]
    }
    text.update(
        universe=metadata.universe,
        version=metadata.version,
        signature=metadata.signature,
        product_id=metadata.product_id,
        frequency=metadata.frequency,
        start_date=metadata.start_date,
        end_date=metadata.end_date,
        read_only=metadata.read_only,
        datapoints=metadata.number_of_datapoints,
        dimensions=[
            (
                dimension.name.en,
                dimension.name.fr,
                dimension.items,
                dimension.type,
                dimension.description.en,
                dimension.description.fr,
                dimension.members.en,
                dimension.members.fr,
                dimension.notes.en,
                dimension.notes.fr,
            )
            for dimension in metadata.dimensions
        ],
        footnotes=[
            (note.symbol, note.number, note.description.en, note.description.fr)
            for note in metadata.footnotes
        ],
        missing=[
            (note.symbol, note.code, note.description.en, note.description.fr)
            for note in metadata.missing_values
        ],
    )
    return hashlib.sha256(json.dumps(text, sort_keys=True).encode()).hexdigest()


def stream_result(ivt, counts, slugs):
    import pyarrow as pa

    digests = {name: hashlib.sha256() for name in [*slugs, "value", "symbol"]}
    rows = 0
    with pa.RecordBatchReader._import_from_c_capsule(ivt._arrow_c_stream()) as reader:
        schema = schema_identity(reader.schema)
        for batch in reader:
            rows += batch.num_rows
            for name, digest in digests.items():
                if name not in batch.schema.names:
                    continue
                column = batch.column(name)
                for buffer in column.buffers():
                    if buffer is not None:
                        digest.update(buffer)
                if pa.types.is_dictionary(column.type):
                    for buffer in column.dictionary.buffers():
                        if buffer is not None:
                            digest.update(buffer)
    if rows != prod(counts):
        raise AssertionError(f"stream rows {rows} != dimension product {prod(counts)}")
    return {
        "status": "converted",
        "rows": rows,
        "schema": schema,
        "columns": {key: value.hexdigest() for key, value in digests.items()},
    }


def partition_result(ivt, counts, slugs):
    digests = []
    rows = 0
    for member in range(1, counts[0] + 1):
        table = ivt.to_arrow(**{slugs[0]: [member]})
        rows += table.num_rows
        digests.append(arrow_digest(table))
        del table
    if rows != prod(counts):
        raise AssertionError(
            f"partition rows {rows} != dimension product {prod(counts)}"
        )
    return {
        "status": "converted",
        "rows": rows,
        "partitions": len(digests),
        "sha256": hashlib.sha256("".join(digests).encode()).hexdigest(),
    }


def worker(package, entry, data_dir):
    sys.path.insert(0, str(package))
    import pyarrow as pa
    import pyivt

    if Path(pyivt.__file__).resolve().parent.parent != package:
        raise ValueError(
            f"pyivt was imported outside the selected package: {pyivt.__file__}"
        )
    path = data_dir / f"{entry['sha256']}.ivt"
    if file_digest(path) != entry["sha256"]:
        raise ValueError(f"pinned input hash differs: {path}")
    ivt = pyivt.open(path)
    counts, slugs = ivt._layout.counts, ivt._layout.slugs
    if counts != entry.get("official_dimension_counts", counts):
        raise AssertionError(f"dimensions differ from the official counts: {counts}")
    result = {
        "sha256": entry["sha256"],
        "product": entry.get("product_id")
        or entry.get("archive_member")
        or entry["sha256"],
        "counts": counts,
        "slugs": slugs,
        "metadata_sha256": metadata_digest(ivt.metadata),
        "members_sha256": arrow_digest(ivt.to_member_table()),
    }
    for mode in ["stream", "table"]:
        try:
            if mode == "stream":
                result[mode] = stream_result(ivt, counts, slugs)
            else:
                table = ivt.to_arrow()
                result[mode] = {
                    "status": "converted",
                    "rows": table.num_rows,
                    "sha256": arrow_digest(table),
                }
                if table.num_rows != prod(counts):
                    raise AssertionError("table rows differ from the dimension product")
                del table
        except (MemoryError, ValueError, pa.ArrowException) as error:
            result[mode] = {
                "status": "rejected",
                "exception": type(error).__name__,
                "message": str(error),
            }
        gc.collect()
    if any(result[mode]["status"] == "rejected" for mode in ["stream", "table"]):
        result["partitioned_table"] = partition_result(ivt, counts, slugs)
    return result


def package_identity(package):
    files = [
        path
        for path in (package / "pyivt").iterdir()
        if path.suffix in {".py", ".pyd", ".so", ".dll", ".dylib"}
    ]
    if not files:
        raise ValueError(f"no pyivt package found under {package}")
    return {path.name: file_digest(path) for path in sorted(files)}


def run_package(args, entries, package, destination):
    results = []
    with destination.open("w", encoding="utf-8") as output:
        for index, entry in enumerate(entries):
            command = [
                sys.executable,
                str(Path(__file__).resolve()),
                "--manifest",
                str(args.manifest.resolve()),
                "--data-dir",
                str(args.data_dir.resolve()),
                "--package",
                str(package),
                "--worker-index",
                str(index),
            ]
            completed = subprocess.run(
                command,
                check=False,
                capture_output=True,
                text=True,
                timeout=args.timeout,
            )
            if completed.returncode:
                raise RuntimeError(
                    f"worker failed for {entry['sha256']}:\n{completed.stderr}"
                )
            result = json.loads(completed.stdout)
            results.append(result)
            output.write(json.dumps(result) + "\n")
            output.flush()
            print(
                destination.stem,
                result["product"],
                result["stream"]["status"],
                result["table"]["status"],
                flush=True,
            )
    return results


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--manifest", type=Path, default=Path("tests/corpus/heldout.json")
    )
    parser.add_argument("--data-dir", type=Path, default=Path(".cache/heldout"))
    parser.add_argument(
        "--output-dir", type=Path, default=Path(".cache/heldout-compatibility")
    )
    parser.add_argument("--reference-package", type=Path)
    parser.add_argument("--candidate-package", type=Path)
    parser.add_argument("--reference-wheel", type=Path)
    parser.add_argument("--timeout", type=float, default=900)
    parser.add_argument("--package", type=Path, help=argparse.SUPPRESS)
    parser.add_argument("--worker-index", type=int, help=argparse.SUPPRESS)
    args = parser.parse_args()
    entries = json.loads(args.manifest.read_text(encoding="utf-8"))["files"]
    if not entries:
        parser.error("the manifest contains no files")
    if args.worker_index is not None:
        if args.package is None:
            parser.error("worker mode requires --package")
        print(
            json.dumps(
                worker(
                    args.package.resolve(),
                    entries[args.worker_index],
                    args.data_dir.resolve(),
                )
            )
        )
        return
    if args.reference_package is None or args.candidate_package is None:
        parser.error("provide --reference-package and --candidate-package")
    args.output_dir.mkdir(parents=True, exist_ok=True)
    reference_package = args.reference_package.resolve()
    candidate_package = args.candidate_package.resolve()
    summary = {
        "manifest_sha256": file_digest(args.manifest),
        "reference_package": package_identity(reference_package),
        "candidate_package": package_identity(candidate_package),
    }
    if args.reference_wheel is not None:
        summary["reference_wheel_sha256"] = file_digest(args.reference_wheel)
    reference = run_package(
        args, entries, reference_package, args.output_dir / "reference.jsonl"
    )
    candidate = run_package(
        args, entries, candidate_package, args.output_dir / "candidate.jsonl"
    )
    differences = [
        {
            "sha256": before["sha256"],
            "fields": [
                key
                for key in sorted(before.keys() | after.keys())
                if before.get(key) != after.get(key)
            ],
        }
        for before, after in zip(reference, candidate, strict=True)
        if before != after
    ]
    summary.update(
        status="different" if differences else "identical",
        inputs=len(entries),
        complete_rows=sum(
            result.get("partitioned_table", result["table"])["rows"]
            for result in candidate
        ),
        differences=differences,
        reference_results_sha256=file_digest(args.output_dir / "reference.jsonl"),
        candidate_results_sha256=file_digest(args.output_dir / "candidate.jsonl"),
    )
    (args.output_dir / "summary.json").write_text(
        json.dumps(summary, indent=2) + "\n", encoding="utf-8"
    )
    print(
        json.dumps(
            {
                key: summary[key]
                for key in ["status", "inputs", "complete_rows", "differences"]
            }
        )
    )
    if differences:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
