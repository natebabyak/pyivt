"""Exercise the codegen command and the generated module as a caller would."""

import hashlib
import os
import runpy
import shutil
import subprocess
import sys
from pathlib import Path

import pyivt
import pytest
from test_typed_arrow import _typed_fixture


def _command(source, output, *args, check=True):
    return subprocess.run(
        [
            sys.executable,
            "-m",
            "pyivt.cli",
            "codegen",
            str(source),
            "--output",
            str(output),
            *args,
        ],
        check=check,
        capture_output=True,
        text=True,
    )


def _tool(name):
    suffix = ".exe" if sys.platform == "win32" else ""
    bundled = Path(sys.executable).with_name(name + suffix)
    return str(bundled) if bundled.is_file() else shutil.which(name)


def test_codegen_writes_deterministic_ruff_clean_types(tmp_path):
    source, output = tmp_path / "table.ivt", tmp_path / "table_types.py"
    data = _typed_fixture(
        geography_labels=("Été", "Ete"), geography_codes=("0007", "500")
    )
    source.write_bytes(data)
    _command(source, output)
    content = output.read_bytes()
    assert hashlib.sha256(data).hexdigest().encode() in content
    assert pyivt.__version__.encode() in content
    assert b"# Language: en" in content
    assert b"Do not edit" in content
    _command(source, output)
    assert output.read_bytes() == content
    subprocess.run(
        [_tool("ruff"), "format", "--isolated", "--check", str(output)], check=True
    )
    module = runpy.run_path(str(output))
    assert module["Geo"].ETE == 7
    assert module["Geo"].ETE_500 == 500
    assert module["Time"]._1970_01_01 == 0
    ivt = pyivt.open(source)
    dims = module["Dimensions"](geo=[module["Geo"].ETE_500])
    assert ivt.to_arrow(**dims)["geo"].to_pylist() == [500] * 6
    assert ivt.members(**dims)["geo"]["code"].to_pylist() == [500]

    _command(source, output, "--language", "fr")
    assert b"# Language: fr" in output.read_bytes()
    module = runpy.run_path(str(output))
    assert module["Geo"].QUEBEC_EST == 7
    assert module["Measure"].CODE_1 == 1


def test_codegen_preserves_input_and_existing_output_on_error(tmp_path):
    source, output = tmp_path / "table.ivt", tmp_path / "table_types.py"
    data = _typed_fixture(geography_codes=("X", "2"))
    source.write_bytes(data)
    output.write_text("existing", encoding="utf-8")
    invalid = _command(source, output, check=False)
    assert invalid.returncode == 2
    assert "invalid native Code" in invalid.stderr
    assert output.read_text(encoding="utf-8") == "existing"
    same_file = _command(source, source, check=False)
    assert same_file.returncode == 2
    assert "output must differ" in same_file.stderr
    assert source.read_bytes() == data


def test_codegen_accepts_a_dimension_whose_class_name_is_a_keyword(tmp_path):
    source, output = tmp_path / "table.ivt", tmp_path / "table_types.py"
    source.write_bytes(_typed_fixture(measure_name="None"))
    _command(source, output)
    module = runpy.run_path(str(output))
    dimensions = module["Dimensions"](none=[module["None_2"].LEVEL])
    assert pyivt.open(source).to_arrow(**dimensions)["none"].to_pylist() == [1] * 6


def test_codegen_keeps_long_enum_names_and_matches_ruff(tmp_path):
    source, output = tmp_path / "table.ivt", tmp_path / "table_types.py"
    labels = ("A" * 79, "B" * 100)
    source.write_bytes(
        _typed_fixture(geography_labels=labels, geography_codes=("65536", "2"))
    )
    _command(source, output)
    subprocess.run(
        [_tool("ruff"), "format", "--isolated", "--check", str(output)], check=True
    )
    geo = runpy.run_path(str(output))["Geo"]
    assert [member.name for member in geo] == list(labels)


@pytest.mark.parametrize("checker", ["ty", "mypy", "pyright"])
def test_generated_kwargs_type_check_at_call_site(tmp_path, checker):
    command = _tool(checker)
    if command is None:
        pytest.skip(f"{checker} is not installed")
    source = tmp_path / "table.ivt"
    source.write_bytes(_typed_fixture(geography_labels=("Class", "West")))
    _command(source, tmp_path / "table_types.py")
    script = tmp_path / "check.py"
    good = """\
import pyivt
from table_types import Dimensions, Geo, Measure

def check(ivt: pyivt.Ivt) -> None:
    ivt.to_arrow(**Dimensions(geo=[Geo._CLASS], measure=[Measure.LEVEL]))
    ivt.members(language="fr", **Dimensions(geo=Geo.WEST))
"""
    args = [command]
    if checker == "ty":
        args += ["check", "--python", sys.executable]
    elif checker == "pyright":
        config = tmp_path / "pyrightconfig.json"
        config.write_text('{"exclude": []}', encoding="utf-8")
        args += ["--pythonpath", sys.executable, "--project", str(config)]
    elif checker == "mypy":
        args += ["--python-executable", sys.executable]
    args.append(script.name)
    environment = {**os.environ, "VIRTUAL_ENV": str(Path(sys.prefix))}
    script.write_text(good, encoding="utf-8")
    subprocess.run(args, check=True, cwd=tmp_path, env=environment)
    script.write_text(
        good
        + "    ivt.to_arrow(**Dimensions(unknown=Geo.WEST))\n"
        + "    ivt.to_arrow(**Dimensions(geo=[Measure.LEVEL]))\n",
        encoding="utf-8",
    )
    bad = subprocess.run(
        args, check=False, cwd=tmp_path, env=environment, capture_output=True, text=True
    )
    assert bad.returncode == 1
    assert ("No overloads" if checker == "pyright" else "unknown") in bad.stdout
    assert "Measure" in bad.stdout
