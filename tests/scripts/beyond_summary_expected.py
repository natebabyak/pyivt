"""Generate expected IVT metadata JSON from Beyond 20/20 File Summary HTML.

Windows-only. Not run by pytest::

    uv sync --group beyond
    uv run python tests/scripts/beyond_summary_expected.py --suite demo

    # parser check against checked-in samples (no GUI):
    uv run python tests/scripts/beyond_summary_expected.py --parse-samples

Per file (Beyond stays open for the whole run):

1. ``os.startfile`` the IVT (first call launches Beyond; later calls reuse it)
2. Alt+F Y — File Summary
3. Copy newest %TEMP%/~iv####.htm (English)
4. Tab, Alt+V H — switch language
5. Alt+F Y — File Summary
6. Copy newest %TEMP%/~iv####.htm (French)
7. Tab, Alt+V H — restore language
8. Alt+F C — close the IVT in Beyond
9. Parse both with bs4 into golden-shaped JSON
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
from pathlib import Path

from bs4 import BeautifulSoup, NavigableString

_TESTS = Path(__file__).resolve().parents[1]
_ROOT = _TESTS.parent
if str(_TESTS) not in sys.path:
    sys.path.insert(0, str(_TESTS))

from conftest import DEMO_MANIFEST, STATCAN_DIR, STATCAN_MANIFEST, resolve_demo_ivt
from metadata_expected import DEMO_EXPECTED_DIR, STATCAN_EXPECTED_DIR, first_last_words

BRWPRO = Path(
    os.environ.get(
        "PYIVT_BRWPRO",
        r"C:\Program Files\Beyond 2020\Professional Browser\Brwpro.exe",
    )
)
TEMP = Path(os.environ.get("TEMP") or os.environ.get("TMP") or ".")
SAMPLES = _ROOT / "temp_summaries"
SUMMARY_RE = re.compile(r"^~iv\d{4,5}\.htm$", re.IGNORECASE)

MONTHS = {
    "jan": 1,
    "january": 1,
    "feb": 2,
    "february": 2,
    "mar": 3,
    "march": 3,
    "apr": 4,
    "april": 4,
    "may": 5,
    "jun": 6,
    "june": 6,
    "jul": 7,
    "july": 7,
    "aug": 8,
    "august": 8,
    "sep": 9,
    "sept": 9,
    "september": 9,
    "oct": 10,
    "october": 10,
    "nov": 11,
    "november": 11,
    "dec": 12,
    "december": 12,
}


def _summary_files() -> list[Path]:
    return [TEMP / n for n in os.listdir(TEMP) if SUMMARY_RE.match(n)]


def _newest_mtime() -> float:
    matches = _summary_files()
    return max((p.stat().st_mtime for p in matches), default=0.0)


def newest_summary_htm(after_mtime: float = 0.0, timeout: float = 10.0) -> Path:
    """Newest ~iv####.htm whose mtime is strictly greater than *after_mtime*."""
    from pywinauto.timings import TimeoutError as WaitTimeoutError
    from pywinauto.timings import wait_until

    def found() -> bool:
        matches = _summary_files()
        if not matches:
            return False
        newest = max(matches, key=lambda p: p.stat().st_mtime)
        return newest.stat().st_mtime > after_mtime

    try:
        wait_until(timeout, 0.05, found)
    except WaitTimeoutError as e:
        raise TimeoutError(f"no new ~iv####.htm in {TEMP}") from e
    return max(_summary_files(), key=lambda p: p.stat().st_mtime)


class Beyond2020:
    def __init__(self, exe: Path = BRWPRO) -> None:
        from pywinauto import Application

        self._Application = Application
        self.exe = Path(exe)
        self.app = None
        self.win = None

    def close(self) -> None:
        if self.app is None:
            return
        try:
            self.app.kill()
        except Exception:
            pass
        self.app = None
        self.win = None

    def _main(self):
        self.win = self.app.top_window()
        self.win.wait("ready", timeout=30)
        self.win.set_focus()
        return self.win

    def keys(self, sequence: str) -> None:
        self._main().type_keys(sequence, with_spaces=True, pause=0.02)

    def open_ivt(self, ivt_path: Path) -> None:
        """Open via Windows file association; first call also launches Beyond."""
        path = Path(ivt_path).resolve()
        os.startfile(path)  # noqa: S606 — intentional local IVT open
        if self.app is None:
            self.app = self._Application(backend="uia").connect(
                path=str(self.exe), timeout=30
            )
        stem = path.stem
        self.win = self.app.window(title_re=f".*{re.escape(stem)}.*")
        self.win.wait("ready", timeout=30)
        self.win.set_focus()

    def close_ivt(self) -> None:
        """Close the current table in Beyond (Alt+F C)."""
        self.keys("%fc")
        self._main()

    def read_summary_html(self) -> str:
        before = _newest_mtime()
        self.keys("%fy")
        path = newest_summary_htm(after_mtime=before)
        html = path.read_text(encoding="utf-8", errors="replace")
        print(f"  {path.name}", flush=True)
        self.keys("{ESC}")
        self._main()
        return html

    def switch_language(self) -> None:
        self.keys("{TAB}")
        self.keys("%vh")
        self._main()

    def extract(self, ivt_path: Path) -> tuple[str, str]:
        self.open_ivt(ivt_path)
        try:
            en_html = self.read_summary_html()
            self.switch_language()
            fr_html = self.read_summary_html()
            self.switch_language()
            return en_html, fr_html
        finally:
            self.close_ivt()


def _clean(text: str | None) -> str | None:
    if text is None:
        return None
    text = text.replace("\xa0", " ").strip()
    return text or None


def _fields(soup: BeautifulSoup) -> dict[str, str]:
    out: dict[str, str] = {}
    for tr in soup.find_all("tr"):
        th = tr.find("th")
        td = tr.find("td")
        if not th or not td:
            continue
        label = th.get_text(" ", strip=True).rstrip(":").strip().lower()
        value = td.get_text(" ", strip=True).replace("\xa0", " ").strip()
        if label:
            out[label] = value
    return out


def _notes(soup: BeautifulSoup) -> str | None:
    h4 = soup.find("h4", string=lambda s: bool(s) and "note" in s.lower())
    if h4 is None:
        return None
    parts: list[str] = []
    for sib in h4.next_siblings:
        name = getattr(sib, "name", None)
        if name in ("hr", "h4"):
            break
        if isinstance(sib, NavigableString):
            parts.append(str(sib))
        elif hasattr(sib, "get_text"):
            parts.append(sib.get_text("\n", strip=False))
    text = re.sub(r"[ \t]+", " ", "".join(parts))
    text = re.sub(r"\n+", "\n", text).strip()
    text = re.sub(r"\s+", " ", text).strip()
    return text or None


def _dims(soup: BeautifulSoup) -> list[dict]:
    for table in soup.find_all("table"):
        rows = table.find_all("tr")
        if not rows:
            continue
        headers = [
            c.get_text(" ", strip=True).lower() for c in rows[0].find_all(["th", "td"])
        ]
        try:
            name_i = next(
                i for i, h in enumerate(headers) if h in ("dimension", "name", "nom")
            )
            items_i = next(
                i
                for i, h in enumerate(headers)
                if h in ("items", "éléments", "elements")
            )
        except StopIteration:
            continue
        type_i = next((i for i, h in enumerate(headers) if h == "type"), None)
        desc_i = next((i for i, h in enumerate(headers) if h.startswith("desc")), None)
        dims = []
        for row in rows[1:]:
            cells = row.find_all(["td", "th"])
            if len(cells) <= max(name_i, items_i):
                continue
            try:
                items = int(cells[items_i].get_text(" ", strip=True).replace(",", ""))
            except ValueError:
                continue
            name = cells[name_i].get_text(" ", strip=True)
            dims.append(
                {
                    "name": name,
                    "items": items,
                    "type": cells[type_i].get_text(" ", strip=True)
                    if type_i is not None
                    else None,
                    "description": (
                        cells[desc_i].get_text(" ", strip=True)
                        if desc_i is not None
                        else name
                    ),
                }
            )
        if dims:
            return dims
    return []


def _get(fields: dict[str, str], *keys: str) -> str | None:
    for key in keys:
        value = _clean(fields.get(key))
        if value:
            return value
    return None


def _parse_ts(text: str | None) -> tuple[str | None, str | None, str | None]:
    """Parse Beyond Time Series values like 'Monthly, Jan1991-Mar1994'."""
    if not text:
        return None, None, None
    freq = None
    m = re.match(
        r"(?i)\s*(Annual|Yearly|Quarterly|Monthly|Weekly|Daily|Semi-?annual)\s*,\s*(.+)\s*$",
        text,
    )
    if m:
        freq = m.group(1)
        if re.match(r"(?i)semi-?annual", freq):
            freq = "Semi-annual"
        else:
            freq = freq[:1].upper() + freq[1:]
        rest = m.group(2)
    else:
        rest = text

    # 1H1980-2H1992
    m = re.match(r"(?i)\s*([12])H(\d{4})\s*[-–—]\s*([12])H(\d{4})\s*$", rest)
    if m:

        def half(h: str, year: str) -> str:
            month = 1 if h == "1" else 7
            return f"{int(year):04d}-{month:02d}-01"

        return freq, half(m.group(1), m.group(2)), half(m.group(3), m.group(4))

    # Jan1991-Mar1994 (no space)
    m = re.match(
        r"(?i)\s*(Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|Apr(?:il)?|May|Jun(?:e)?|"
        r"Jul(?:y)?|Aug(?:ust)?|Sep(?:t(?:ember)?)?|Oct(?:ober)?|Nov(?:ember)?|"
        r"Dec(?:ember)?)(\d{4})\s*[-–—]\s*"
        r"(Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|Apr(?:il)?|May|Jun(?:e)?|"
        r"Jul(?:y)?|Aug(?:ust)?|Sep(?:t(?:ember)?)?|Oct(?:ober)?|Nov(?:ember)?|"
        r"Dec(?:ember)?)(\d{4})\s*$",
        rest,
    )
    if m:
        mo1 = MONTHS[m.group(1).lower().rstrip(".")]
        mo2 = MONTHS[m.group(3).lower().rstrip(".")]
        return (
            freq,
            f"{int(m.group(2)):04d}-{mo1:02d}-01",
            f"{int(m.group(4)):04d}-{mo2:02d}-01",
        )

    return freq, None, None


def parse_summary(html: str) -> dict:
    soup = BeautifulSoup(html, "html.parser")
    f = _fields(soup)
    n_dp = _get(
        f,
        "number of datapoints",
        "nombre de points de données",
        "nombre de points de donnees",
    )
    datapoints = None
    if n_dp:
        try:
            datapoints = int(n_dp.replace(",", "").replace(" ", ""))
        except ValueError:
            pass
    ro = _get(f, "read-only", "lecture seule")
    freq, start, end = _parse_ts(
        _get(f, "time series", "série chronologique", "serie chronologique")
    )
    return {
        "title": _get(f, "title", "titre"),
        "author": _get(f, "author", "auteur"),
        "category": _get(f, "category", "catégorie", "categorie"),
        "keywords": _get(f, "keywords", "mots-clés", "mots-cles", "mots clés"),
        "notes": _notes(soup),
        "number_of_datapoints": datapoints,
        "frequency": freq,
        "start_date": start,
        "end_date": end,
        "read_only": (ro.lower() in ("yes", "oui", "true", "1") if ro else None),
        "dimensions": _dims(soup),
    }


def merge_en_fr(en: dict, fr: dict) -> dict:
    notes_en = first_last_words(en.get("notes"))
    notes_fr = first_last_words(fr.get("notes"))
    fr_dims = fr.get("dimensions") or []
    dims = []
    for i, d_en in enumerate(en.get("dimensions") or []):
        d_fr = fr_dims[i] if i < len(fr_dims) else {}
        dims.append(
            {
                "name_en": d_en.get("name"),
                "name_fr": d_fr.get("name"),
                "items": d_en.get("items"),
                "type": d_en.get("type"),
                "description_en": d_en.get("description"),
                "description_fr": d_fr.get("description") or d_fr.get("name"),
            }
        )
    return {
        "title": {"en": en.get("title"), "fr": fr.get("title")},
        "author": {"en": en.get("author"), "fr": fr.get("author")},
        "category": {"en": en.get("category"), "fr": fr.get("category")},
        "keywords": {"en": en.get("keywords"), "fr": fr.get("keywords")},
        "notes": {
            "en": {"first": notes_en[0], "last": notes_en[1]},
            "fr": {"first": notes_fr[0], "last": notes_fr[1]},
        },
        "number_of_datapoints": en.get("number_of_datapoints"),
        "frequency": en.get("frequency") or fr.get("frequency"),
        "start_date": en.get("start_date") or fr.get("start_date"),
        "end_date": en.get("end_date") or fr.get("end_date"),
        "read_only": en.get("read_only")
        if en.get("read_only") is not None
        else fr.get("read_only"),
        "dimensions": dims,
        "footnotes": [],
        "missing_values": [],
    }


def jobs_for(suite: str | None, paths: list[Path]) -> list[tuple[str, Path]]:
    if paths:
        return [(p.stem.lower(), p) for p in paths]
    assert suite is not None
    out: list[tuple[str, Path]] = []
    if suite == "demo":
        for entry in DEMO_MANIFEST:
            path = resolve_demo_ivt(entry["filename"])
            if path is None:
                print(
                    f"skip {entry['id']}: missing {entry['filename']}", file=sys.stderr
                )
                continue
            out.append((entry["id"], path))
    else:
        for entry in STATCAN_MANIFEST:
            path = STATCAN_DIR / entry["filename"]
            if not path.is_file():
                print(f"skip {entry['id']}: missing {path.name}", file=sys.stderr)
                continue
            out.append((entry["id"], path))
    return out


def parse_samples() -> int:
    """Run parser against temp_summaries/*.htm and print a short dump."""
    files = sorted(SAMPLES.glob("~iv*.htm"))
    if not files:
        print(f"no samples in {SAMPLES}", file=sys.stderr)
        return 1
    for path in files:
        parsed = parse_summary(path.read_text(encoding="utf-8", errors="replace"))
        print(path.name)
        print(
            json.dumps(
                {
                    "title": parsed["title"],
                    "author": parsed["author"],
                    "number_of_datapoints": parsed["number_of_datapoints"],
                    "frequency": parsed["frequency"],
                    "start_date": parsed["start_date"],
                    "end_date": parsed["end_date"],
                    "read_only": parsed["read_only"],
                    "notes": first_last_words(parsed["notes"]),
                    "dimensions": parsed["dimensions"],
                },
                ensure_ascii=False,
                indent=2,
            )
        )
        print()
    return 0


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="*", type=Path)
    parser.add_argument("--suite", choices=("statcan", "demo"))
    parser.add_argument("--out-dir", type=Path)
    parser.add_argument("--exe", type=Path, default=BRWPRO)
    parser.add_argument(
        "--parse-samples",
        action="store_true",
        help="Parse temp_summaries HTML only (no GUI)",
    )
    args = parser.parse_args(argv)
    if args.parse_samples:
        return parse_samples()
    if not args.paths and not args.suite:
        parser.error("pass IVT paths, --suite, or --parse-samples")

    out_dir = args.out_dir
    if out_dir is None and args.suite == "demo":
        out_dir = DEMO_EXPECTED_DIR
    elif out_dir is None and args.suite == "statcan":
        out_dir = STATCAN_EXPECTED_DIR
    if out_dir is not None:
        out_dir.mkdir(parents=True, exist_ok=True)

    jobs = jobs_for(args.suite, args.paths)
    beyond = Beyond2020(exe=args.exe)
    wrote = 0
    try:
        for product_id, ivt_path in jobs:
            print(f"processing {ivt_path} …", flush=True)
            try:
                en_html, fr_html = beyond.extract(ivt_path)
            except Exception as e:
                print(f"error {product_id}: {type(e).__name__}: {e!r}", file=sys.stderr)
                continue
            payload = merge_en_fr(parse_summary(en_html), parse_summary(fr_html))
            text = json.dumps(payload, ensure_ascii=False, indent=2) + "\n"
            if out_dir is None:
                print(text)
            else:
                out = out_dir / f"{product_id}.beyond.json"
                out.write_text(text, encoding="utf-8")
                rel = out.relative_to(_ROOT) if out.is_relative_to(_ROOT) else out
                print(f"wrote {rel}", flush=True)
            wrote += 1
    finally:
        beyond.close()
    print(f"done: {wrote} written")
    return 0 if wrote else 1


if __name__ == "__main__":
    raise SystemExit(main())
