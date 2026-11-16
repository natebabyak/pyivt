"""Run ``cargo test`` against a Python interpreter's shared libpython.

PyO3 enables ``extension-module`` only through maturin, so ``cargo test``
links the test binaries against libpython and loads it at run time. It fails
with ``unable to find library -lpython3.x`` when the interpreter cargo finds
has no shared libpython, and with a missing-library error at run time when
the library is not on the loader path. This script points PyO3 at one
interpreter and puts that interpreter's library directory on the loader path:

    uv run python scripts/cargo_test.py
    uv run python scripts/cargo_test.py --python 3.12 -- --release decode::

``--python`` takes an interpreter path or a version of a uv-managed install
(``uv python install 3.12`` adds one). It defaults to the interpreter running
this script. Arguments after ``--`` go to ``cargo test``.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from pathlib import Path

_QUERY = """
import json, sys, sysconfig
print(json.dumps({
    "executable": sys.executable,
    "base_prefix": sys.base_prefix,
    "libdir": sysconfig.get_config_var("LIBDIR"),
    "shared": bool(sysconfig.get_config_var("Py_ENABLE_SHARED")),
    "framework": bool(sysconfig.get_config_var("PYTHONFRAMEWORK")),
}))
"""


def _resolve(python: str | None) -> str:
    if python is None:
        return sys.executable
    if Path(python).exists():
        return python
    found = subprocess.run(
        ["uv", "python", "find", "--managed-python", python],
        capture_output=True,
        text=True,
        check=False,
    )
    if found.returncode != 0:
        sys.exit(f"cargo_test: no interpreter for {python!r}: {found.stderr.strip()}")
    return found.stdout.strip()


def _prepend(env: dict[str, str], name: str, directory: str) -> None:
    current = env.get(name)
    env[name] = directory if not current else directory + os.pathsep + current


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--python", help="interpreter path or uv version request")
    parser.add_argument("cargo_args", nargs="*", help="arguments for cargo test")
    args = parser.parse_args()

    query = subprocess.run(
        [_resolve(args.python), "-c", _QUERY],
        capture_output=True,
        text=True,
        check=True,
    )
    info = json.loads(query.stdout)
    env = dict(os.environ, PYO3_PYTHON=info["executable"])
    if sys.platform == "win32":
        # python3.dll and python3XY.dll live next to the base interpreter.
        _prepend(env, "PATH", info["base_prefix"])
    else:
        if not (info["shared"] or info["framework"]):
            print(
                f"cargo_test: {info['executable']} has no shared libpython; linking may "
                "fail. Use a uv-managed Python instead, e.g. --python 3.12.",
                file=sys.stderr,
            )
        if info["libdir"]:
            name = (
                "DYLD_LIBRARY_PATH" if sys.platform == "darwin" else "LD_LIBRARY_PATH"
            )
            _prepend(env, name, info["libdir"])
    print(f"cargo_test: PYO3_PYTHON={info['executable']}", file=sys.stderr)
    return subprocess.run(
        ["cargo", "test", *args.cargo_args], env=env, check=False
    ).returncode


if __name__ == "__main__":
    sys.exit(main())
