"""Read-only PE/string cross-reference probe for the dimension investigation.

Run with Python 3 and GNU objdump on PATH. Does not execute the target.
Addresses are preferred-image virtual addresses, not file offsets.
"""

import argparse
import hashlib
import re
import struct
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser(__doc__)
parser.add_argument(
    "--binary", default=r"C:\Program Files\Beyond 2020\Professional Browser\Brwpro.exe"
)
parser.add_argument("--string", action="append", default=[])
parser.add_argument("--address", type=lambda s: int(s, 0))
parser.add_argument(
    "--end",
    type=lambda s: int(s, 0),
    help="Explicit exclusive disassembly end; .pdata can describe only a function fragment",
)
args = parser.parse_args()
data = Path(args.binary).read_bytes()
pe = struct.unpack_from("<I", data, 0x3C)[0]
count = struct.unpack_from("<H", data, pe + 6)[0]
opt_size = struct.unpack_from("<H", data, pe + 20)[0]
opt = pe + 24
base = struct.unpack_from("<Q", data, opt + 24)[0]
sections = []
for i in range(count):
    p = opt + opt_size + i * 40
    name = data[p : p + 8].split(b"\0")[0].decode()
    size, rva, raw_size, raw = struct.unpack_from("<IIII", data, p + 8)
    sections.append((name, base + rva, raw, raw_size, size))


def va(offset):
    for _, start, raw, raw_size, _ in sections:
        if raw <= offset < raw + raw_size:
            return start + offset - raw
    raise ValueError(offset)


functions = []
for name, _, raw, raw_size, _ in sections:
    if name == ".pdata":
        for p in range(raw, raw + raw_size - 11, 12):
            start, end, _ = struct.unpack_from("<III", data, p)
            if start and end > start:
                functions.append((base + start, base + end))


def body(address):
    return next(((s, e) for s, e in functions if s <= address < e), None)


print("sha256", hashlib.sha256(data).hexdigest())
if args.address is not None:
    bounds = (args.address, args.end) if args.end is not None else body(args.address)
    if bounds is None:
        raise ValueError("No .pdata range for address")
    print(
        "range [start,end); .pdata ranges may be function fragments",
        *(hex(x) for x in bounds),
        flush=True,
    )
    subprocess.run(
        [
            "objdump",
            "-d",
            "-M",
            "intel",
            f"--start-address={bounds[0]}",
            f"--stop-address={bounds[1]}",
            args.binary,
        ],
        check=True,
    )
else:
    targets = {}
    for text in args.string:
        needle = text.encode("ascii")
        pos = 0
        while (pos := data.find(needle, pos)) >= 0:
            targets[va(pos)] = text
            print("string", hex(va(pos)), "file", hex(pos), repr(text))
            pos += len(needle)
    output = subprocess.check_output(
        ["objdump", "-d", "-M", "intel", args.binary], text=True
    )
    for line in output.splitlines():
        match = re.search(r"# (?:0x)?([0-9a-f]+)", line)
        if match and int(match[1], 16) in targets:
            address = int(line.strip().split(":")[0], 16)
            bounds = body(address)
            print(
                "xref",
                targets[int(match[1], 16)],
                line.strip(),
                "pdata",
                None if bounds is None else tuple(hex(x) for x in bounds),
            )
