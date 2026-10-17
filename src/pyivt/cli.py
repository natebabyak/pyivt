"""Command-line interface: ``python -m pyivt`` / ``pyivt``."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

from .metadata import read_metadata
from .parquet import write_metadata, write_parquet


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        prog="pyivt",
        description="Extract Beyond 20/20 IVT metadata and convert cells to Parquet.",
    )
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_meta = sub.add_parser("meta", help="Print or write file / dimension metadata")
    p_meta.add_argument("ivt", type=Path)
    p_meta.add_argument(
        "-o", "--out", type=Path, help="Write CSV sidecars to this directory"
    )
    p_meta.add_argument(
        "--json", action="store_true", help="Print metadata JSON to stdout"
    )

    p_pq = sub.add_parser("parquet", help="Decode cells and write Parquet")
    p_pq.add_argument("ivt", type=Path)
    p_pq.add_argument("-o", "--out", type=Path, required=True)
    p_pq.add_argument("--sparse", action="store_true", help="Store only non-zero cells")
    p_pq.add_argument("--lang", default="en", choices=("en", "fr"))
    p_pq.add_argument("--no-labels", action="store_true")
    p_pq.add_argument("--no-members", action="store_true")

    args = parser.parse_args(argv)

    if args.cmd == "meta":
        meta = read_metadata(args.ivt)
        if args.out:
            write_metadata(meta, args.out)
            print(args.out)
        if args.json or not args.out:
            print(json.dumps(meta.to_dict(), ensure_ascii=False, indent=2))
        return 0

    if args.cmd == "parquet":
        path = write_parquet(
            args.ivt,
            args.out,
            labels=not args.no_labels,
            members=not args.no_members,
            language=args.lang,
            complete=not args.sparse,
        )
        print(path)
        return 0

    return 1


if __name__ == "__main__":
    sys.exit(main())
