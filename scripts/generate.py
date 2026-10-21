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

from pywinauto import Application, Desktop, ElementNotFoundError
from pywinauto.keyboard import send_keys
from selectolax.lexbor import LexborHTMLParser, LexborNode
from src.constants import BEYOND_PATH
from tqdm import tqdm


@dataclass
class IvtSummaryStatisticsDimension:
    name_en: str
    name_fr: str
    items: int
    type: str
    description_en: str
    description_fr: str


@dataclass
class IvtSummaryFootnote:
    symbol: str
    description_en: str
    description_fr: str


@dataclass
class IvtSummaryMissingValue:
    symbol: str
    description_en: str
    description_fr: str


@dataclass
class IvtSummary:
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
    dimensions: list[IvtSummaryStatisticsDimension] = dataclass_field(
        default_factory=list,
    )
    footnotes: list[IvtSummaryFootnote] = dataclass_field(
        default_factory=list,
    )
    missing_values: list[IvtSummaryMissingValue] = dataclass_field(
        default_factory=list,
    )


SUMMARY_TIMEOUT_SECONDS = 5.0


def text(node: LexborNode) -> str:
    return node.text(deep=True, separator="", strip=True).strip()


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


def read_summary() -> LexborHTMLParser:
    deadline = time.monotonic() + SUMMARY_TIMEOUT_SECONDS
    while time.monotonic() < deadline:
        path = summary_path()
        if path is not None:
            try:
                html = path.read_text(encoding="cp1252").strip()
            except OSError:
                html = ""
            if html[:6].upper() == "<HTML>" and html[-7:].upper() == "</HTML>":
                return LexborHTMLParser(html)
        time.sleep(0.1)
    raise RuntimeError("Timed out waiting for summary HTML")


class Beyond:
    def __init__(self, executable: Path = BEYOND_PATH) -> None:
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

    def summaries(self, path: Path) -> tuple[LexborHTMLParser, LexborHTMLParser]:
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


def field(tree: LexborHTMLParser, labels: tuple[str, ...]) -> str:
    for row in tree.css("tr"):
        cells = row.css("th, td")
        if len(cells) >= 2 and text(cells[0]) in labels:
            return text(cells[1])
    raise ValueError(f"Missing field: {labels}")


def optional_field(tree: LexborHTMLParser, labels: tuple[str, ...]) -> str | None:
    value = field(tree, labels)
    return value or None


def section_table(
    tree: LexborHTMLParser, headings: tuple[str, ...]
) -> LexborNode | None:
    for heading in tree.css("h4"):
        if text(heading) in headings:
            node = heading.next
            while node is not None:
                if node.tag == "table":
                    return node
                if node.tag == "h4":
                    break
                node = node.next
    return None


def notes(tree: LexborHTMLParser) -> str | None:
    for heading in tree.css("h4"):
        if text(heading) in ("Notes:", "Notes :"):
            values: list[str] = []
            node = heading.next
            while node is not None and node.tag != "hr":
                if node.tag == "br":
                    values.append("\n")
                else:
                    value = text(node)
                    if value:
                        values.append(value)
                node = node.next
            return "".join(values).rstrip("\n") or None
    raise ValueError("Missing notes section")


def dimensions(
    english_summary: LexborHTMLParser,
    french_summary: LexborHTMLParser,
) -> list[IvtSummaryStatisticsDimension]:
    english_table = next(
        (
            table
            for table in english_summary.css("table")
            if any(text(node) == "Dimension" for node in table.css("b"))
        ),
        None,
    )
    french_table = next(
        (
            table
            for table in french_summary.css("table")
            if any(text(node) == "Dimension" for node in table.css("b"))
        ),
        None,
    )
    if english_table is None or french_table is None:
        return []

    english_cells = english_table.css("td")
    french_cells = french_table.css("td")
    if len(english_cells) != len(french_cells) or len(english_cells) % 4:
        raise ValueError("Invalid dimension table")

    return [
        IvtSummaryStatisticsDimension(
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
    english_summary: LexborHTMLParser,
    french_summary: LexborHTMLParser,
) -> list[IvtSummaryFootnote]:
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

    english_cells = english_table.css("td")
    french_cells = french_table.css("td")
    if len(english_cells) % 2 or len(french_cells) < len(english_cells):
        raise ValueError("Invalid footnote table")

    return [
        IvtSummaryFootnote(
            symbol=text(english_cells[index]),
            description_en=text(english_cells[index + 1]),
            description_fr=text(french_cells[index + 1]),
        )
        for index in range(0, len(english_cells), 2)
    ]


def missing_values(
    english_summary: LexborHTMLParser,
    french_summary: LexborHTMLParser,
) -> list[IvtSummaryMissingValue]:
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

    english_cells = english_table.css("td")
    french_cells = french_table.css("td")
    if len(english_cells) % 2:
        raise ValueError("Invalid missing-value table")

    values: list[IvtSummaryMissingValue] = []
    for index in range(0, len(english_cells), 2):
        english_description = text(english_cells[index + 1])
        french_description = (
            text(french_cells[index + 1])
            if index + 1 < len(french_cells)
            else english_description
        )
        values.append(
            IvtSummaryMissingValue(
                symbol=text(english_cells[index]),
                description_en=english_description,
                description_fr=french_description,
            )
        )
    return values


def parse_time_series(
    value: str | None,
) -> tuple[str | None, datetime | None, datetime | None]:
    if not value:
        return None, None, None

    frequency, separator, date_range = value.partition(",")
    if not separator:
        return frequency.strip() or None, None, None

    start, separator, end = date_range.strip().partition("-")
    if not separator:
        return frequency.strip() or None, None, None

    try:
        start_date = datetime.fromisoformat(start.strip())
        end_date = datetime.fromisoformat(end.strip())
    except ValueError:
        start_date = None
        end_date = None

    return frequency.strip() or None, start_date, end_date


def build_ivt_summary(beyond: Beyond, ivt_path: Path) -> IvtSummary:
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

    return IvtSummary(
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
    parser = argparse.ArgumentParser(
        description="Generate local JSON metadata from Beyond 20/20 IVT files."
    )
    parser.add_argument("paths", nargs="+", type=Path)
    parser.add_argument("--out-dir", type=Path, required=True)
    parser.add_argument("--exe", type=Path, default=BEYOND_PATH)
    args = parser.parse_args(argv)

    args.out_dir.mkdir(parents=True, exist_ok=True)
    written_count = 0

    with (
        Beyond(args.exe) as beyond,
        tqdm(
            desc="Generating summaries", total=len(args.paths), unit="IVTs"
        ) as progress,
    ):
        for ivt_path in args.paths:
            try:
                summary = build_ivt_summary(beyond, ivt_path)
                output_path = args.out_dir / f"{ivt_path.stem.lower()}.beyond.json"
                output_path.write_text(
                    json.dumps(
                        asdict(summary),
                        default=datetime.isoformat,
                        ensure_ascii=False,
                        indent=2,
                    )
                    + "\n",
                    encoding="utf-8",
                )
                tqdm.write(f"Wrote {output_path}")
                written_count += 1
            except (OSError, RuntimeError, ValueError) as error:
                tqdm.write(f"Skipping {ivt_path}: {error}")
            finally:
                progress.update()

    print(f"Done: {written_count} written")
    return 0 if written_count else 1


if __name__ == "__main__":
    raise SystemExit(main())
