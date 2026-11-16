"""The public API contract: ``__all__``, docs, stubs, version and every public item."""

from __future__ import annotations

import ast
import inspect
import re
import subprocess
import sys
import zipfile
from importlib.metadata import version
from io import BytesIO
from pathlib import Path

import pyarrow as pa
import pyivt
import pytest
from pyivt import _native

ROOT = Path(__file__).resolve().parents[1]
PACKAGE = ROOT / "python" / "pyivt"
FIXTURE = ROOT / "tests" / "fixtures" / "heldout" / "98100036.ivt"
PUBLIC = {
    "CorruptFileError",
    "Dimension",
    "Footnote",
    "Geographies",
    "Ivt",
    "IvtError",
    "IvtMetadata",
    "LocalizedLabels",
    "LocalizedText",
    "MissingValue",
    "ResourceLimitError",
    "UnsupportedFormatError",
    "open",
}
# Private hooks that exist at runtime for pyivt's own code and tests only.
PRIVATE_MEMBERS = {
    "_arrow_c_stream",
    "_full_loads",
    "_io_metrics",
    "_layout",
    "_legacy_to_arrow",
    "_loaded",
    "_member_fields",
    "_source_owner",
}


def _stub_all(path: Path) -> set[str]:
    for node in ast.parse(path.read_text(encoding="utf-8")).body:
        if isinstance(node, ast.Assign) and any(
            isinstance(target, ast.Name) and target.id == "__all__"
            for target in node.targets
        ):
            return set(ast.literal_eval(node.value))
    raise AssertionError(f"{path.name} has no __all__")


def _stub_class_members(path: Path, name: str) -> set[str]:
    tree = ast.parse(path.read_text(encoding="utf-8"))
    for node in tree.body:
        if isinstance(node, ast.ClassDef) and node.name == name:
            return {
                child.name
                for child in node.body
                if isinstance(child, (ast.FunctionDef, ast.AsyncFunctionDef))
            }
    raise AssertionError(f"stub has no class {name}")


def _public_members(cls: type) -> set[str]:
    return {name for name in vars(cls) if not name.startswith("_")}


@pytest.fixture(scope="module")
def ivt() -> pyivt.Ivt:
    return pyivt.open(FIXTURE)


def test_all_is_the_documented_public_api():
    assert set(pyivt.__all__) == PUBLIC
    assert len(pyivt.__all__) == len(PUBLIC)
    assert _stub_all(PACKAGE / "__init__.pyi") == PUBLIC
    docs = (ROOT / "docs" / "api.md").read_text(encoding="utf-8")
    table = docs.split("<!-- public-api:start -->")[1].split("<!-- public-api:end -->")
    assert set(re.findall(r"^\| `(\w+)` \|", table[0], re.MULTILINE)) == PUBLIC


def test_no_internal_helpers_are_reachable_from_the_package():
    visible = {name for name in dir(pyivt) if not name.startswith("_")}
    assert visible == PUBLIC
    # A fresh interpreter sees the same namespace, without test-only imports.
    names = subprocess.run(
        [
            sys.executable,
            "-c",
            "import pyivt; print(sorted(n for n in dir(pyivt) if n[0] != '_'))",
        ],
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    assert ast.literal_eval(names) == sorted(PUBLIC)


def test_star_import_exports_only_public_names():
    namespace: dict[str, object] = {}
    exec("from pyivt import *", namespace)  # noqa: S102
    assert set(namespace) - {"__builtins__"} == PUBLIC


def test_public_classes_report_the_package_module():
    for name in PUBLIC:
        assert getattr(pyivt, name).__module__ == "pyivt", name


def test_every_public_item_has_a_docstring():
    assert (pyivt.__doc__ or "").strip()
    for name in PUBLIC:
        item = getattr(pyivt, name)
        assert (item.__doc__ or "").strip(), name
        if inspect.isclass(item):
            for member in _public_members(item):
                doc = getattr(vars(item)[member], "__doc__", None)
                assert (doc or "").strip(), f"{name}.{member}"


def test_stubs_cover_every_public_member_and_hide_private_ones():
    native, errors = PACKAGE / "_native.pyi", PACKAGE / "_errors.py"
    for name in PUBLIC - {"open"}:
        cls = getattr(pyivt, name)
        if issubclass(cls, BaseException):
            assert cls.__module__ == "pyivt"
            assert f"class {name}(" in errors.read_text(encoding="utf-8")
            continue
        stubbed = _stub_class_members(native, name)
        assert _public_members(cls) <= stubbed, name
        assert not stubbed & PRIVATE_MEMBERS, name
    text = native.read_text(encoding="utf-8")
    for member in PRIVATE_MEMBERS - {"_layout", "_member_fields"}:
        assert member not in text, member
    # _layout and _member_fields are typed only on the private protocol.
    assert "class _IvtInternals(Protocol)" in text
    assert inspect.signature(_native.open).parameters.keys() == {"source"}


def test_type_alias_docs_do_not_import_private_aliases():
    text = (PACKAGE / "_types.py").read_text(encoding="utf-8")
    assert "from pyivt import" not in text


def test_version_comes_from_package_metadata_and_cargo():
    # tomllib needs Python 3.11; both files keep these keys on one line.
    cargo = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    project = (ROOT / "pyproject.toml").read_text(encoding="utf-8")
    declared = re.search(r'^version = "([^"]+)"$', cargo, re.MULTILINE)
    assert declared is not None
    assert not re.search(r"^version\s*=", project, re.MULTILINE)
    assert re.search(r'^dynamic = \[[^]]*"version"', project, re.MULTILINE)
    assert pyivt.__version__ == version("pyivt")
    assert pyivt.__version__ == _native.__version__ == declared[1]
    for path in (PACKAGE / "__init__.py", PACKAGE / "cli.py"):
        assert declared[1] not in path.read_text(encoding="utf-8")


def test_open_accepts_every_documented_source(ivt):
    data = FIXTURE.read_bytes()
    archive = BytesIO()
    with zipfile.ZipFile(archive, "w") as zipped:
        zipped.writestr("a/table.ivt", data)
        zipped.writestr("b/other.ivt", data)
    sources = [
        str(FIXTURE),
        FIXTURE,
        data,
        bytearray(data),
        memoryview(data),
        BytesIO(data),
    ]
    expected = ivt.to_arrow()
    for source in sources:
        opened = pyivt.open(source)
        assert isinstance(opened, pyivt.Ivt)
        assert opened.to_arrow().equals(expected), type(source)
    opened = pyivt.open(archive.getvalue(), member="b/other.ivt")
    assert opened.to_arrow().equals(expected)
    with pytest.raises(TypeError):
        pyivt.open(1.5)  # type: ignore[arg-type]
    with pytest.raises(TypeError):
        pyivt.open(data, member=1)  # type: ignore[arg-type]
    with pytest.raises(ValueError, match="member="):
        pyivt.open(data, member="table.ivt")
    with pytest.raises(pyivt.IvtError):
        pyivt.open(b"not an ivt file")


def test_ivt_is_not_constructible_and_exposes_its_metadata(ivt):
    with pytest.raises(TypeError):
        pyivt.Ivt()  # type: ignore[call-arg]
    assert isinstance(ivt.metadata, pyivt.IvtMetadata)
    assert ivt.metadata is ivt.metadata
    assert _public_members(pyivt.Ivt) == {
        "iter_batches",
        "metadata",
        "to_arrow",
        "to_member_table",
    }
    assert not hasattr(ivt, "__arrow_c_stream__")


def test_exports_select_by_slug_with_one_based_codes(ivt):
    dims = ivt.metadata.dimensions
    first, second = dims[0].slug, dims[1].slug
    table = ivt.to_arrow(select={first: 1, second: [1, 2]})
    assert isinstance(table, pa.Table)
    assert table.column_names == [slug.slug for slug in dims[1:]] + ["value"]
    assert set(table[second].to_pylist()) == {1, 2}
    metadata = table.schema.metadata
    assert metadata[b"pyivt.dimension_encoding"] == b"one_based_index"
    assert b"pyivt.table_id" in metadata
    assert ivt.to_arrow(select={first: []}).num_rows == 0
    for bad in ({"no_such_axis": 1}, {first: 0}, {first: [1, 1]}, {first: "1"}):
        with pytest.raises(ValueError):
            ivt.to_arrow(select=bad)


def test_keyword_selection_still_works_but_is_deprecated(ivt):
    slug = ivt.metadata.dimensions[0].slug
    for export in (ivt.to_arrow, ivt.to_member_table):
        with pytest.warns(DeprecationWarning, match=r"select=\{") as record:
            legacy = export(**{slug: [1, 2]})
        # The warning points at the caller, not at pyivt.
        assert record[0].filename == __file__
        assert legacy.equals(export(select={slug: [1, 2]}))
    with pytest.warns(DeprecationWarning):
        legacy = ivt.iter_batches(batch_size=5, **{slug: [1]}).read_all()
    assert legacy.equals(ivt.to_arrow(select={slug: [1]}))


def test_select_rejects_mixed_forms_and_non_mappings(ivt):
    slug = ivt.metadata.dimensions[0].slug
    for export in (ivt.to_arrow, ivt.iter_batches, ivt.to_member_table):
        with pytest.raises(TypeError, match="select="):
            export(select={slug: 1}, **{slug: 1})
        with pytest.raises(TypeError, match="mapping"):
            export(select=[(slug, 1)])  # type: ignore[arg-type]
        export(select={})
        export(select=None)


def test_max_memory_bytes_is_public(ivt):
    for export in (ivt.to_arrow, ivt.iter_batches):
        with pytest.raises(pyivt.ResourceLimitError) as raised:
            export(max_memory_bytes=1024)
        assert raised.value.limit == 1024
        with pytest.raises(ValueError, match="positive"):
            export(max_memory_bytes=0)
    assert ivt.to_arrow(max_memory_bytes=1 << 26).equals(ivt.to_arrow())


def test_iter_batches_streams_the_to_arrow_table(ivt):
    reader = ivt.iter_batches(batch_size=7)
    assert isinstance(reader, pa.RecordBatchReader)
    batches = list(reader)
    assert all(batch.num_rows <= 7 for batch in batches)
    assert pa.Table.from_batches(batches, reader.schema).equals(ivt.to_arrow())
    slug = ivt.metadata.dimensions[0].slug
    with ivt.iter_batches(select={slug: [2]}) as selected:
        assert selected.read_all().equals(ivt.to_arrow(select={slug: [2]}))
    with pytest.raises(ValueError, match="batch_size"):
        ivt.iter_batches(batch_size=0)


def test_member_table_labels_codes_in_both_languages(ivt):
    dims = ivt.metadata.dimensions
    english = ivt.to_member_table()
    french = ivt.to_member_table(language="fr")
    assert english.column_names[:3] == ["dimension", "code", "label"]
    assert english.num_rows == french.num_rows == sum(dim.items for dim in dims)
    lookup = ivt.to_member_table(select={dims[0].slug: [3]})
    rows = [row for row in lookup.to_pylist() if row["dimension"] == dims[0].slug]
    assert [row["code"] for row in rows] == [3]
    assert rows[0]["label"] == dims[0].members.en[2]
    with pytest.raises(ValueError, match="language"):
        ivt.to_member_table(language="de")  # type: ignore[arg-type]


def test_metadata_objects(ivt):
    meta = ivt.metadata
    assert isinstance(meta.title, pyivt.LocalizedText)
    for field in ("author", "category", "keywords", "notes"):
        assert isinstance(getattr(meta, field), pyivt.LocalizedText)
    assert isinstance(meta.version, str)
    assert re.fullmatch(r"[0-9a-f]{8}", meta.signature)
    assert meta.frequency in {None, "monthly", "quarterly", "semi_annual", "annual"}
    for field in ("product_id", "universe", "start_date", "end_date"):
        assert getattr(meta, field) is None or isinstance(getattr(meta, field), str)
    assert meta.read_only in {None, True, False}
    assert meta.number_of_datapoints == ivt.to_arrow().num_rows

    dims = meta.dimensions
    assert all(isinstance(dim, pyivt.Dimension) for dim in dims)
    assert dims[0].slug == "geography"
    for dim in dims:
        assert isinstance(dim.name, pyivt.LocalizedText)
        assert isinstance(dim.description, pyivt.LocalizedText)
        assert isinstance(dim.notes, pyivt.LocalizedText)
        assert isinstance(dim.members, pyivt.LocalizedLabels)
        assert len(dim.members.en or []) == dim.items
        assert dim.type in {"geographic", "normal", "time_series"}

    geographies = meta.geographies
    assert isinstance(geographies, pyivt.Geographies)
    assert geographies.member_id == list(range(1, dims[0].items + 1))
    assert isinstance(geographies.geo_label, pyivt.LocalizedLabels)
    for field in ("geo_name", "geo_uid", "dqf_code", "tnr_short_form"):
        values = getattr(geographies, field)
        assert values is None or len(values) == dims[0].items

    assert meta.footnotes and all(
        isinstance(note, pyivt.Footnote) for note in meta.footnotes
    )
    note = meta.footnotes[0]
    assert isinstance(note.symbol, str) and isinstance(note.number, int)
    assert isinstance(note.description, pyivt.LocalizedText)
    assert meta.missing_values and all(
        isinstance(missing, pyivt.MissingValue) for missing in meta.missing_values
    )
    missing = meta.missing_values[0]
    assert isinstance(missing.symbol, str)
    assert missing.code is None or isinstance(missing.code, int)
    assert isinstance(missing.description, pyivt.LocalizedText)
    for cls in (
        pyivt.IvtMetadata,
        pyivt.Dimension,
        pyivt.Footnote,
        pyivt.MissingValue,
        pyivt.Geographies,
    ):
        with pytest.raises(TypeError):
            cls()


def test_localized_values_construct_and_index():
    text = pyivt.LocalizedText(en="Population", fr="Population totale")
    assert (text.en, text.fr, text["en"], text["fr"]) == (
        "Population",
        "Population totale",
        "Population",
        "Population totale",
    )
    assert pyivt.LocalizedText().en is None
    labels = pyivt.LocalizedLabels(en=["A", "B"])
    assert labels.en == ["A", "B"] and labels["fr"] is None
    labels.en.append("C")  # type: ignore[union-attr]
    assert labels.en == ["A", "B"]
    for value in (text, labels):
        with pytest.raises(KeyError):
            value["de"]
        with pytest.raises(AttributeError):
            value.en = None  # type: ignore[misc]


def test_exception_hierarchy():
    assert issubclass(pyivt.IvtError, ValueError)
    for cls in (
        pyivt.CorruptFileError,
        pyivt.UnsupportedFormatError,
        pyivt.ResourceLimitError,
    ):
        assert issubclass(cls, pyivt.IvtError)
    assert issubclass(pyivt.ResourceLimitError, MemoryError)
    assert not issubclass(pyivt.CorruptFileError, pyivt.UnsupportedFormatError)
    error = pyivt.CorruptFileError("bad", offset=4, file_len=10)
    assert (str(error), error.offset, error.file_len, error.slot) == (
        "bad",
        4,
        10,
        None,
    )
    limit = pyivt.ResourceLimitError("big", estimate=3, limit=2, dimension=1)
    assert (limit.estimate, limit.limit, limit.dimension) == (3, 2, 1)
    with pytest.raises(pyivt.CorruptFileError) as raised:
        pyivt.open(b"\0" * 64)
    assert isinstance(raised.value, ValueError)


def test_cli_entry_point_is_public(tmp_path):
    output = tmp_path / "types.py"
    subprocess.run(
        [sys.executable, "-m", "pyivt.cli", "codegen", str(FIXTURE), "--output"]
        + [str(output)],
        check=True,
    )
    assert f"# pyivt version: {pyivt.__version__}" in output.read_text("utf-8")
