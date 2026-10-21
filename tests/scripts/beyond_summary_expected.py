"""Generate expected IVT metadata JSON from Beyond 20/20 File Summary HTML.

Windows-only. Not run by pytest::

    uv sync --group beyond
    uv run python tests/scripts/beyond_summary_expected.py --suite demo

Per file:

1. Open the IVT
2. Alt+F Y — File Summary
3. Copy newest %TEMP%/~iv####.htm (English)
4. Tab, then Alt+V H — switch language
5. Alt+F Y — File Summary again
6. Copy newest %TEMP%/~iv####.htm (French)
7. Parse both with bs4 into golden-shaped JSON
"""

from __future__ import annotations

import argparse
import json
import os
import re
import sys
import time
from pathlib import Path

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
TEMP = Path(os.environ.get("TEMP") or os.environ.get("TMP") or os.getenv("TMPDIR", "."))
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


def newest_summary_htm(timeout: float = 15.0) -> Path:
    """Return the newest %TEMP%/~iv####.htm, waiting until one appears."""
    deadline = time.time() + timeout
    while time.time() < deadline:
        matches = []
        for name in os.listdir(TEMP):
            if SUMMARY_RE.match(name):
                matches.append(TEMP / name)
        if matches:
            return max(matches, key=lambda p: p.stat().st_mtime)
        time.sleep(0.25)
    raise TimeoutError(f"no ~iv####.htm in {TEMP}")


class Beyond2020:
    def __init__(self, exe: Path = BRWPRO) -> None:
        from pywinauto import Application

        self._Application = Application
        self.exe = Path(exe)
        self.app = None
        self.win = None

    def open(self, ivt_path: Path) -> None:
        self.close()
        ivt_path = Path(ivt_path).resolve()
        self.app = self._Application(backend="uia").start(
            f'"{self.exe}" "{ivt_path}"', wait_for_idle=False
        )
        time.sleep(2)
        self.win = self.app.top_window()
        self.win.set_focus()

    def close(self) -> None:
        if self.app is None:
            return
        try:
            self.app.kill()
        except Exception:
            pass
        self.app = None
        self.win = None

    def keys(self, sequence: str) -> None:
        self.win.set_focus()
        self.win.type_keys(sequence, with_spaces=True, pause=0.05)

    def extract(self, ivt_path: Path) -> tuple[str, str]:
        """Open one IVT and return (en_html, fr_html)."""
        self.open(ivt_path)

        # English summary
        self.keys("%fy")
        en_path = newest_summary_htm()
        en_html = en_path.read_text(encoding="utf-8", errors="replace")
        print(f"  en: {en_path.name}", flush=True)

        # Tab to focus, then switch language
        self.keys("{TAB}")
        time.sleep(0.3)
        self.keys("%vh")
        time.sleep(1.5)

        # French summary
        self.keys("%fy")
        fr_path = newest_summary_htm()
        fr_html = fr_path.read_text(encoding="utf-8", errors="replace")
        print(f"  fr: {fr_path.name}", flush=True)

        # Restore language for the next file
        self.keys("{TAB}")
        time.sleep(0.3)
        self.keys("%vh")
        time.sleep(0.5)
        self.close()
        return en_html, fr_html


def _fields(soup) -> dict[str, str]:
    out: dict[str, str] = {}
    for tr in soup.find_all("tr"):
        cells = tr.find_all(["td", "th"])
        if len(cells) < 2:
            continue
        label = cells[0].get_text(" ", strip=True).rstrip(":").strip().lower()
        value = cells[1].get_text(" ", strip=True)
        if label:
            out[label] = value
    return out


def _dims(soup) -> list[dict]:
    for table in soup.find_all("table"):
        rows = table.find_all("tr")
        if not rows:
            continue
        headers = [c.get_text(" ", strip=True).lower() for c in rows[0].find_all(["th", "td"])]
        try:
            name_i = next(i for i, h in enumerate(headers) if h in ("name", "nom"))
            items_i = next(
                i for i, h in enumerate(headers) if h in ("items", "éléments", "elements")
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
            dims.append(
                {
                    "name": cells[name_i].get_text(" ", strip=True),
                    "items": items,
                    "type": cells[type_i].get_text(" ", strip=True) if type_i is not None else None,
                    "description": (
                        cells[desc_i].get_text(" ", strip=True)
                        if desc_i is not None
                        else cells[name_i].get_text(" ", strip=True)
                    ),
                }
            )
        if dims:
            return dims
    return []


def _get(fields: dict[str, str], *keys: str) -> str | None:
    for key in keys:
        if fields.get(key):
            return fields[key]
    return None


def _parse_ts(text: str | None) -> tuple[str | None, str | None, str | None]:
    if not text:
        return None, None, None
    freq = None
    m = re.search(
        r"(?i)\b(Annual|Yearly|Quarterly|Monthly|Weekly|Daily|Semi-?annual)\b", text
    )
    if m:
        freq = m.group(1)
        if re.match(r"(?i)semi-?annual", freq):
            freq = "Semi-annual"
        else:
            freq = freq[:1].upper() + freq[1:]
    start = end = None
    m = re.search(r"\((\d{4})\s*[-–—]\s*(\d{4})\)", text)
    if m:
        return freq, f"{int(m.group(1)):04d}-01-01", f"{int(m.group(2)):04d}-01-01"
    m = re.search(
        r"(?i)\b(Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|Apr(?:il)?|May|Jun(?:e)?|"
        r"Jul(?:y)?|Aug(?:ust)?|Sep(?:t(?:ember)?)?|Oct(?:ober)?|Nov(?:ember)?|"
        r"Dec(?:ember)?)\s+(\d{4})\b(?:.*?\b(Jan(?:uary)?|Feb(?:ruary)?|Mar(?:ch)?|"
        r"Apr(?:il)?|May|Jun(?:e)?|Jul(?:y)?|Aug(?:ust)?|Sep(?:t(?:ember)?)?|"
        r"Oct(?:ober)?|Nov(?:ember)?|Dec(?:ember)?)\s+(\d{4})\b)?",
        text,
    )
    if m:
        mo = MONTHS[m.group(1).lower().rstrip(".")]
        start = f"{int(m.group(2)):04d}-{mo:02d}-01"
        if m.group(3) and m.group(4):
            mo2 = MONTHS[m.group(3).lower().rstrip(".")]
            end = f"{int(m.group(4)):04d}-{mo2:02d}-01"
        else:
            end = start
    return freq, start, end


def parse_summary(html: str) -> dict:
    from bs4 import BeautifulSoup

    soup = BeautifulSoup(html, "html.parser")
    f = _fields(soup)
    n_dp = _get(
        f,
        "number of datapoints",
        "number of data points",
        "nombre de points de données",
        "nombre de points de donnees",
    )
    datapoints = None
    if n_dp:
        try:
            datapoints = int(n_dp.replace(",", "").replace(" ", ""))
        except ValueError:
            pass
    ro = _get(f, "read-only", "read only", "lecture seule")
    freq, start, end = _parse_ts(
        _get(f, "time series", "série chronologique", "serie chronologique")
    )
    return {
        "title": _get(f, "title", "titre"),
        "author": _get(f, "author", "auteur"),
        "category": _get(f, "category", "catégorie", "categorie"),
        "keywords": _get(f, "keywords", "mots-clés", "mots-cles", "mots clés"),
        "notes": _get(f, "notes", "note"),
        "number_of_datapoints": datapoints,
        "frequency": freq,
        "start_date": start,
        "end_date": end,
        "read_only": (ro.strip().lower() in ("yes", "oui", "true", "1") if ro else None),
        "dimensions": _dims(soup),
    }


def merge_en_fr(en: dict, fr: dict) -> dict:
    notes_en = first_last_words(en.get("notes"))
    notes_fr = first_last_words(fr.get("notes"))
    dims = []
    for i, d_en in enumerate(en.get("dimensions") or []):
        d_fr = (fr.get("dimensions") or [{}])[i] if i < len(fr.get("dimensions") or []) else {}
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
                print(f"skip {entry['id']}: missing {entry['filename']}", file=sys.stderr)
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


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("paths", nargs="*", type=Path)
    parser.add_argument("--suite", choices=("statcan", "demo"))
    parser.add_argument("--out-dir", type=Path)
    parser.add_argument("--exe", type=Path, default=BRWPRO)
    args = parser.parse_args(argv)
    if not args.paths and not args.suite:
        parser.error("pass IVT paths or --suite demo|statcan")

    out_dir = args.out_dir
    if out_dir is None and args.suite == "demo":
        out_dir = DEMO_EXPECTED_DIR
    elif out_dir is None and args.suite == "statcan":
        out_dir = STATCAN_EXPECTED_DIR
    if out_dir is not None:
        out_dir.mkdir(parents=True, exist_ok=True)

    print(f"TEMP={TEMP}", flush=True)
    beyond = Beyond2020(exe=args.exe)
    wrote = 0
    try:
        for product_id, ivt_path in jobs_for(args.suite, args.paths):
            print(f"processing {ivt_path} …", flush=True)
            try:
                en_html, fr_html = beyond.extract(ivt_path)
            except Exception as e:
                print(f"error {product_id}: {e}", file=sys.stderr)
                beyond.close()
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
