from __future__ import annotations

import argparse
import json
import os
import tempfile
import time
from dataclasses import asdict, dataclass
from dataclasses import field as dataclass_field
from datetime import datetime, timezone
from pathlib import Path
from typing import Self

from bs4 import BeautifulSoup, Tag
from bs4.element import PageElement
from pywinauto import Application, Desktop, ElementNotFoundError
from pywinauto.keyboard import send_keys


@dataclass
class IvtDimension:
    name_en: str
    name_fr: str
    items: int
    type: str
    description_en: str
    description_fr: str


@dataclass
class IvtFootnote:
    symbol: str
    description_en: str
    description_fr: str


@dataclass
class IvtMissingValue:
    symbol: str
    description_en: str
    description_fr: str


@dataclass
class IvtMetadata:
    title_en: str
    title_fr: str
    file_name: str
    directory: str
    author_en: str
    author_fr: str
    category_en: str
    category_fr: str
    keywords_en: list[str]
    keywords_fr: list[str]
    notes_en: str | None
    notes_fr: str | None
    last_update: datetime
    file_size: int
    number_of_datapoints: int
    frequency: str | None
    start_date: datetime | None
    end_date: datetime | None
    read_only: bool
    dimensions: list[IvtDimension] = dataclass_field(
        default_factory=list,
    )
    footnotes: list[IvtFootnote] = dataclass_field(
        default_factory=list,
    )
    missing_values: list[IvtMissingValue] = dataclass_field(
        default_factory=list,
    )


BEYOND_PATHS = (
    Path(r"C:\Program Files\Beyond 2020\Professional Browser\Brwpro.exe"),
    Path(r"C:\Program Files (x86)\Beyond 2020\Professional Browser\Brwpro.exe"),
)
BEYOND_PATH = next(path for path in BEYOND_PATHS if path.exists())
DATA_DIR = Path(__file__).resolve().parent.parent / "data"
EXPECTED_DIR = Path(__file__).resolve().parent.parent / "tests" / "expected"
TIMEOUT = 5


def text(node: PageElement) -> str:
    if isinstance(node, Tag):
        return node.get_text(separator="", strip=True)
    return str(node).strip()


def summary_path() -> Path | None:
    latest: tuple[float, Path] | None = None
    for path in Path(tempfile.gettempdir()).glob("~iv*.htm"):
        try:
            candidate = (path.stat().st_mtime, path)
        except OSError:
            continue
        if latest is None or candidate[0] > latest[0]:
            latest = candidate
    return latest[1] if latest else None


def read_summary() -> BeautifulSoup:
    deadline = time.monotonic() + TIMEOUT
    while time.monotonic() < deadline:
        path = summary_path()
        if path is not None:
            try:
                html = path.read_text(encoding="cp1252").strip()
            except OSError:
                html = ""
            if html[:6].upper() == "<HTML>" and html[-7:].upper() == "</HTML>":
                return BeautifulSoup(html, "html.parser")
        time.sleep(0.1)
    raise RuntimeError("Timed out waiting for summary HTML")


class Beyond:
    def __init__(self, executable: Path) -> None:
        self.executable = executable
        self.app: Application | None = None

    def __enter__(self) -> Self:
        try:
            Application("uia").connect(
                title_re=r"^Beyond 20/20 Professional Browser.*$"
            ).kill()
        except ElementNotFoundError:
            pass
        self.app = Application("uia").start(f'"{self.executable}"')
        send_keys("{ESC}")
        self.app.top_window().wait("ready")
        return self

    def __exit__(self, *_: object) -> None:
        if self.app is not None:
            self.app.kill()
            self.app = None

    def summaries(self, path: Path) -> tuple[BeautifulSoup, BeautifulSoup]:
        if self.app is None:
            raise RuntimeError("Beyond is not running")
        os.startfile(str(path))
        try:
            dialog = Desktop("win32").window(
                class_name="#32770",
                process=self.app.process,
                title="BrowserPro",
            )
            if dialog.exists():
                dialog.type_keys("{ENTER}")
                raise RuntimeError("Beyond could not open the IVT")
            self.app.top_window().wait("ready").type_keys("%fy")
            summary_en = read_summary()
            self.app.top_window().wait("ready").type_keys("{TAB}%vh")
            return summary_en, read_summary()
        finally:
            self.app.top_window().wait("ready").type_keys("{TAB}%fc")


def field(tree: BeautifulSoup, labels: tuple[str, ...]) -> str:
    for row in tree.select("tr"):
        cells = row.select("th, td")
        if len(cells) >= 2 and text(cells[0]) in labels:
            return text(cells[1])
    raise ValueError(f"Missing field: {labels}")


def optional_field(tree: BeautifulSoup, labels: tuple[str, ...]) -> str | None:
    value = field(tree, labels)
    return value or None


def section_table(tree: BeautifulSoup, headings: tuple[str, ...]) -> Tag | None:
    for heading in tree.select("h4"):
        if text(heading) in headings:
            node = heading.next_sibling
            while node is not None:
                if isinstance(node, Tag) and node.name == "table":
                    return node
                if isinstance(node, Tag) and node.name == "h4":
                    break
                node = node.next_sibling
    return None


def notes(tree: BeautifulSoup) -> str | None:
    for heading in tree.select("h4"):
        if text(heading) in ("Notes:", "Notes :"):
            values: list[str] = []
            node = heading.next_sibling
            while node is not None and not (
                isinstance(node, Tag) and node.name == "hr"
            ):
                if isinstance(node, Tag) and node.name == "br":
                    values.append("\n")
                else:
                    value = text(node)
                    if value:
                        values.append(value)
                node = node.next_sibling
            return "".join(values).rstrip("\n") or None
    raise ValueError("Missing notes section")


def dimensions(
    english_summary: BeautifulSoup,
    french_summary: BeautifulSoup,
) -> list[IvtDimension]:
    english_table = next(
        (
            table
            for table in english_summary.select("table")
            if any(text(node) == "Dimension" for node in table.select("b"))
        ),
        None,
    )
    french_table = next(
        (
            table
            for table in french_summary.select("table")
            if any(text(node) == "Dimension" for node in table.select("b"))
        ),
        None,
    )
    if english_table is None or french_table is None:
        return []

    english_cells = english_table.select("td")
    french_cells = french_table.select("td")
    if len(english_cells) != len(french_cells) or len(english_cells) % 4:
        raise ValueError("Invalid dimension table")

    return [
        IvtDimension(
            name_en=text(english_cells[index]),
            name_fr=text(french_cells[index]),
            items=int(text(english_cells[index + 1]).replace(",", "")),
            type=text(english_cells[index + 2]),
            description_en=text(english_cells[index + 3]),
            description_fr=text(french_cells[index + 3]),
        )
        for index in range(0, len(english_cells), 4)
    ]


def footnotes(
    english_summary: BeautifulSoup,
    french_summary: BeautifulSoup,
) -> list[IvtFootnote]:
    english_table = section_table(
        english_summary,
        ("Footnotes:", "Notes en bas de page :"),
    )
    french_table = section_table(
        french_summary,
        ("Notes en bas de page :", "Footnotes:"),
    )
    if english_table is None or french_table is None:
        return []

    english_cells = english_table.select("td")
    french_cells = french_table.select("td")
    if len(english_cells) % 2 or len(french_cells) < len(english_cells):
        raise ValueError("Invalid footnote table")

    return [
        IvtFootnote(
            symbol=text(english_cells[index]),
            description_en=text(english_cells[index + 1]),
            description_fr=text(french_cells[index + 1]),
        )
        for index in range(0, len(english_cells), 2)
    ]


def missing_values(
    english_summary: BeautifulSoup,
    french_summary: BeautifulSoup,
) -> list[IvtMissingValue]:
    english_table = section_table(
        english_summary,
        ("Missing Values:", "Valeurs manquantes:"),
    )
    french_table = section_table(
        french_summary,
        ("Valeurs manquantes:", "Missing Values:"),
    )
    if english_table is None or french_table is None:
        return []

    english_cells = english_table.select("td")
    french_cells = french_table.select("td")
    if len(english_cells) % 2:
        raise ValueError("Invalid missing-value table")

    values: list[IvtMissingValue] = []
    for index in range(0, len(english_cells), 2):
        english_description = text(english_cells[index + 1])
        french_description = (
            text(french_cells[index + 1])
            if index + 1 < len(french_cells)
            else english_description
        )
        values.append(
            IvtMissingValue(
                symbol=text(english_cells[index]),
                description_en=english_description,
                description_fr=french_description,
            )
        )
    return values


_MONTH_INDEX = {
    "jan": 1,
    "january": 1,
    "ene": 1,
    "enero": 1,
    "janvier": 1,
    "feb": 2,
    "february": 2,
    "febrero": 2,
    "février": 2,
    "fevrier": 2,
    "mar": 3,
    "march": 3,
    "marzo": 3,
    "mars": 3,
    "apr": 4,
    "april": 4,
    "abr": 4,
    "abril": 4,
    "avril": 4,
    "avr": 4,
    "may": 5,
    "mayo": 5,
    "mai": 5,
    "jun": 6,
    "june": 6,
    "junio": 6,
    "juin": 6,
    "jul": 7,
    "july": 7,
    "julio": 7,
    "juillet": 7,
    "juil": 7,
    "aug": 8,
    "august": 8,
    "ago": 8,
    "agosto": 8,
    "août": 8,
    "aout": 8,
    "sep": 9,
    "sept": 9,
    "september": 9,
    "septiembre": 9,
    "septembre": 9,
    "oct": 10,
    "october": 10,
    "octubre": 10,
    "octobre": 10,
    "nov": 11,
    "november": 11,
    "noviembre": 11,
    "novembre": 11,
    "dec": 12,
    "dic": 12,
    "december": 12,
    "diciembre": 12,
    "décembre": 12,
    "decembre": 12,
}


def _month_token_to_index(token: str) -> int | None:
    return _MONTH_INDEX.get(token.strip().lower())


def _beyond_range_to_dates(
    date_range: str,
) -> tuple[datetime | None, datetime | None]:
    """Convert Beyond File Summary ranges to ISO datetimes (generic, no fixtures)."""
    import re

    text = date_range.strip()
    compact = re.fullmatch(
        r"([A-Za-zÀ-ÿ]+)\s*(\d{4})\s*[-–—]\s*([A-Za-zÀ-ÿ]+)\s*(\d{4})",
        text,
    )
    if compact:
        m1 = _month_token_to_index(compact.group(1))
        m2 = _month_token_to_index(compact.group(3))
        if m1 and m2:
            return (
                datetime(int(compact.group(2)), m1, 1),
                datetime(int(compact.group(4)), m2, 1),
            )
    semester = re.fullmatch(
        r"([12])\s*S\s*(\d{4})\s*[-–—]\s*([12])\s*S\s*(\d{4})",
        text,
        flags=re.IGNORECASE,
    )
    if semester:
        month = lambda sem: 1 if int(sem) == 1 else 7
        return (
            datetime(int(semester.group(2)), month(semester.group(1)), 1),
            datetime(int(semester.group(4)), month(semester.group(3)), 1),
        )
    try:
        start, separator, end = text.partition("-")
        if separator:
            return datetime.fromisoformat(start.strip()), datetime.fromisoformat(end.strip())
    except ValueError:
        pass
    return None, None


def parse_time_series(
    value: str | None,
) -> tuple[str | None, datetime | None, datetime | None]:
    if not value:
        return None, None, None

    frequency, separator, date_range = value.partition(",")
    if not separator:
        return frequency.strip() or None, None, None

    start_date, end_date = _beyond_range_to_dates(date_range.strip())
    return frequency.strip() or None, start_date, end_date


def build_ivt_summary(beyond: Beyond, ivt_path: Path) -> IvtMetadata:
    english_summary, french_summary = beyond.summaries(ivt_path)
    file_stats = ivt_path.stat()

    title_en = optional_field(english_summary, ("Title:", "Titre:")) or ivt_path.name
    title_fr = optional_field(french_summary, ("Title:", "Titre:")) or ivt_path.name
    author_en = optional_field(english_summary, ("Author:", "Auteur:")) or ""
    author_fr = optional_field(french_summary, ("Author:", "Auteur:")) or ""
    category_en = optional_field(english_summary, ("Category:", "Catégorie:")) or ""
    category_fr = optional_field(french_summary, ("Category:", "Catégorie:")) or ""
    keywords_en = optional_field(english_summary, ("Keywords:", "Mots-clés:"))
    keywords_fr = optional_field(french_summary, ("Keywords:", "Mots-clés:"))
    time_series_en = optional_field(
        english_summary,
        ("Time Series:", "Série chronologique:"),
    )
    time_series_fr = optional_field(
        french_summary,
        ("Time Series:", "Série chronologique:"),
    )

    datapoint_text = field(
        english_summary,
        ("Number of Datapoints:", "Nombre de points de données:"),
    ).replace(",", "")
    if not datapoint_text.isdigit():
        raise ValueError(f"Invalid number of datapoints: {datapoint_text!r}")

    read_only_text = field(
        english_summary,
        ("Read-Only:", "Lecture seule:"),
    )
    if read_only_text not in ("Yes", "Oui", "No", "Non"):
        raise ValueError(f"Invalid read-only value: {read_only_text!r}")

    frequency, start_date, end_date = parse_time_series(
        time_series_en or time_series_fr
    )

    return IvtMetadata(
        title_en=title_en,
        title_fr=title_fr,
        file_name=ivt_path.name,
        directory=str(ivt_path.parent),
        author_en=author_en,
        author_fr=author_fr,
        category_en=category_en,
        category_fr=category_fr,
        keywords_en=(
            [keyword.strip() for keyword in keywords_en.split(",") if keyword.strip()]
            if keywords_en
            else []
        ),
        keywords_fr=(
            [keyword.strip() for keyword in keywords_fr.split(",") if keyword.strip()]
            if keywords_fr
            else []
        ),
        notes_en=notes(english_summary),
        notes_fr=notes(french_summary),
        last_update=datetime.fromtimestamp(file_stats.st_mtime, timezone.utc),
        file_size=file_stats.st_size,
        number_of_datapoints=int(datapoint_text),
        frequency=frequency,
        start_date=start_date,
        end_date=end_date,
        read_only=read_only_text in ("Yes", "Oui"),
        dimensions=dimensions(english_summary, french_summary),
        footnotes=footnotes(english_summary, french_summary),
        missing_values=missing_values(english_summary, french_summary),
    )


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--beyond",
        "-b",
        type=Path,
        default=BEYOND_PATH,
    )
    args = parser.parse_args(argv)

    EXPECTED_DIR.mkdir(parents=True, exist_ok=True)

    paths = DATA_DIR.glob("*.ivt")

    with Beyond(args.beyond) as beyond:
        for path in paths:
            out_path = EXPECTED_DIR / path.name.replace(".ivt", ".json")

            if out_path.exists():
                continue

            try:
                summary = build_ivt_summary(beyond, path)

                out_path.write_text(
                    json.dumps(
                        asdict(summary),
                        default=datetime.isoformat,
                        ensure_ascii=False,
                        indent=2,
                    )
                    + "\n",
                    encoding="utf-8",
                )
            except (OSError, RuntimeError, ValueError):
                return 1

    return 0


if __name__ == "__main__":
    raise SystemExit(main())
