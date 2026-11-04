"""Run generated modules and check their per-axis typing contracts."""

import importlib.util
import json
import os
import subprocess
import sys
from pathlib import Path
from types import ModuleType

import pyivt
import pytest
from test_typed_arrow import _typed_fixture


def load_generated(path):
    module = ModuleType("generated")
    exec(compile(path.read_text(encoding="utf-8"), str(path), "exec"), module.__dict__)  # noqa: S102
    return module


def test_generate_member_types_from_json_text_and_path(tmp_path):
    source = _typed_fixture()
    ivt = pyivt.open(source)
    member_path = tmp_path / "members.json"
    members = ivt.members()
    members.to_json(member_path)
    generated = tmp_path / "generated.py"
    from_text = tmp_path / "from_text.py"
    pyivt.generate_member_types(member_path, generated)
    pyivt.generate_member_types(members.to_json(), from_text)
    assert generated.read_bytes() == from_text.read_bytes()
    pyivt.generate_member_types(str(member_path), str(from_text))
    assert generated.read_bytes() == from_text.read_bytes()

    module = load_generated(generated)
    assert module.TABLE_ID == members.table_id
    assert module.COLUMN_NAMES == ("geo", "measure", "time", "value")
    assert module.Geo.EAST_COAST == 1
    assert module.Time.MEMBER_2000_02_29 == 2
    assert module.Measure.CODE_2 == 2
    selection = module.selection({"geo": [module.Geo.WEST]})
    assert ivt.to_arrow(selection)["geo"].to_pylist() == [2] * 6
    with pytest.raises(ValueError, match="unknown"):
        module.selection({"unknown": [module.Geo.WEST]})


def test_generate_member_types_handles_collisions_keywords_and_empty_axes(tmp_path):
    members = pyivt.Members(
        "0" * 64,
        (
            pyivt.DimensionMembers(
                "none",
                "",
                0,
                tuple(
                    pyivt.Member(code, label, {})
                    for code, label in enumerate(
                        ("A!", "A?", "7", "___", "class", "élan"), start=3
                    )
                ),
            ),
            pyivt.DimensionMembers("foo_bar", "", 0, ()),
            pyivt.DimensionMembers("foo-bar", "", 0, ()),
        ),
        language="fr",
    )
    generated = tmp_path / "generated.py"
    pyivt.generate_member_types(members.to_json(), generated)
    module = load_generated(generated)
    assert list(module.None_2.__members__) == [
        "A",
        "A_2",
        "MEMBER_7",
        "CODE_6",
        "CLASS",
        "ELAN",
    ]
    assert [member.value for member in module.None_2] == list(range(3, 9))
    assert module.FooBar is not module.FooBar_2
    assert module.selection({"foo-bar": []}).to_dict() == {"foo-bar": []}
    empty = pyivt.Members("0" * 64, ())
    pyivt.generate_member_types(empty.to_json(), generated)
    assert load_generated(generated).selection({}) == pyivt.Selection({})


def test_generate_member_types_uses_saved_labels(tmp_path):
    ivt = pyivt.open(Path("tests/fixtures/heldout/98100036.ivt"))
    french = ivt.members(language="fr")
    generated = tmp_path / "generated.py"
    pyivt.generate_member_types(french.to_json(), generated)
    french_enum = load_generated(generated).Geo
    pyivt.generate_member_types(ivt.members(language="en").to_json(), generated)
    assert list(french_enum.__members__) != list(
        load_generated(generated).Geo.__members__
    )
    assert [item.value for item in french_enum] == [
        member.code for member in french.dimensions[0].members
    ]


def test_generate_member_types_rejects_invalid_members_json(tmp_path):
    generated = tmp_path / "generated.py"
    with pytest.raises(ValueError, match="invalid members JSON"):
        pyivt.generate_member_types("{}", generated)
    assert not generated.exists()


@pytest.mark.parametrize("checker", ["ty", "mypy", "pyright"])
def test_generated_selection_type_contract(checker, tmp_path):
    python_source = Path(__file__).resolve().parents[1] / "python"
    pyivt.generate_member_types(
        pyivt.open(_typed_fixture()).members().to_json(), tmp_path / "generated.py"
    )
    (tmp_path / "pyrightconfig.json").write_text(
        json.dumps({"extraPaths": [str(python_source)]}), encoding="utf-8"
    )
    runner = Path(sys.executable).with_name("ty.exe" if os.name == "nt" else "ty")
    if checker == "ty":
        if not runner.is_file():
            pytest.skip("ty unavailable")
        command = [str(runner), "check", "--extra-search-path", str(python_source)]
    else:
        if importlib.util.find_spec(checker) is None:
            pytest.skip(f"{checker} unavailable")
        command = [sys.executable, "-m", checker]
        if checker == "mypy":
            command.extend(["--follow-imports=silent", "--ignore-missing-imports"])
        else:
            command.extend(["--pythonpath", sys.executable])

    source = tmp_path / "consumer.py"
    imports = "from generated import Geo, Time, selection\nfrom pyivt import Ivt\n"
    good = "def export(ivt: Ivt):\n    return ivt.to_arrow(selection({'geo': [Geo.WEST]}))\n"
    env = {**os.environ, "MYPYPATH": str(python_source)}
    source.write_text(imports + good, encoding="utf-8")
    passed = subprocess.run(
        [*command, source.name],
        cwd=tmp_path,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    assert passed.returncode == 0, passed.stdout + passed.stderr

    source.write_text(
        imports
        + "selection({'unknown': [Geo.WEST]})\nselection({'geo': [Time.CODE_3]})\n",
        encoding="utf-8",
    )
    failed = subprocess.run(
        [*command, source.name],
        cwd=tmp_path,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    diagnostics = failed.stdout + failed.stderr
    assert failed.returncode == 1, diagnostics
    assert "consumer.py:3" in diagnostics and "consumer.py:4" in diagnostics, (
        diagnostics
    )
