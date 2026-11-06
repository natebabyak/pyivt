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


@pytest.mark.parametrize(
    "stored_codes",
    [("0007", "INNC"), ("INNC", "INNC"), ('A"B', "A'B"), ("00", "")],
)
def test_codegen_writes_deterministic_ruff_clean_types(tmp_path, stored_codes):
    source, output = tmp_path / "table.ivt", tmp_path / "table_types.py"
    data = _typed_fixture(
        geography_labels=("Été", "Ete"),
        geography_codes=stored_codes,
        time_name="Reference Period",
    )
    source.write_bytes(data)
    _command(source, output)
    content = output.read_bytes()
    assert hashlib.sha256(data).hexdigest().encode() in content
    assert f"# pyivt version: {pyivt.__version__}".encode() in content
    assert b"# Language: en" in content
    assert b"Do not edit" in content
    _command(source, output)
    assert output.read_bytes() == content
    subprocess.run(
        [_tool("ruff"), "format", "--isolated", "--check", str(output)], check=True
    )
    module = runpy.run_path(str(output))
    geography = module["Geography"]
    assert geography.ETE == 1
    assert geography.ETE_2 == 2
    period = module["ReferencePeriod"]
    assert period._1970_01_01 == 1
    assert module["NATIVE_CODES"]["geography"] == {
        geography.ETE: stored_codes[0],
        geography.ETE_2: stored_codes[1] or None,
    }
    assert module["NATIVE_CODES"]["reference_period"][period._1970_01_01] == "000"
    ivt = pyivt.open(source)
    dims = module["Dimensions"](
        geography=[geography.ETE_2], reference_period=[period._1970_01_01]
    )
    table = ivt.to_arrow(**dims)
    assert table["geography"].to_pylist() == [2] * 2
    assert table["reference_period"].to_pylist() == [1] * 2
    assert ivt.to_member_table(**dims).select(["dimension", "code"]).to_pydict() == {
        "dimension": ["geography", "measure", "measure", "reference_period"],
        "code": [2, 1, 2, 1],
    }
    assert "geography" not in ivt.to_arrow(geography=geography.ETE_2).column_names

    _command(source, output, "--language", "fr")
    assert b"# Language: fr" in output.read_bytes()
    module = runpy.run_path(str(output))
    assert module["Geography"].QUEBEC_EST == 1
    assert module["Measure"].CODE_1 == 1


def test_codegen_preserves_input_and_existing_output_on_error(tmp_path):
    source, output = tmp_path / "table.ivt", tmp_path / "table_types.py"
    data = b"invalid IVT"
    source.write_bytes(data)
    output.write_text("existing", encoding="utf-8")
    invalid = _command(source, output, check=False)
    assert invalid.returncode == 2
    assert "header" in invalid.stderr
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
    geography = runpy.run_path(str(output))["Geography"]
    assert [member.name for member in geography] == list(labels)


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
from table_types import Dimensions, Geography, Measure

def check(ivt: pyivt.Ivt) -> None:
    ivt.to_arrow(**Dimensions(geography=[Geography._CLASS], measure=[Measure.LEVEL]))
    ivt.to_member_table(language="fr", **Dimensions(geography=Geography.WEST))
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
        + "    ivt.to_arrow(**Dimensions(unknown=Geography.WEST))\n"
        + "    ivt.to_arrow(**Dimensions(geography=[Measure.LEVEL]))\n",
        encoding="utf-8",
    )
    bad = subprocess.run(
        args, check=False, cwd=tmp_path, env=environment, capture_output=True, text=True
    )
    assert bad.returncode == 1
    assert ("No overloads" if checker == "pyright" else "unknown") in bad.stdout
    assert "Measure" in bad.stdout
