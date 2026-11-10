# Legacy IVT references

The CSVs and JSON files here were generated independently of pyivt by
[canivt](https://github.com/mountainMath/canivt), commit
`2c55e9a194c39a46eb24bb85d2f2b2b9c705025b`, using R 4.6.1. Input download URLs,
published source sizes/MD5s, and acquired SHA-256 identities are pinned in
[`tests/corpus/legacy.json`](../../corpus/legacy.json). The IVT bytes remain
downloaded development inputs in `data/`, following the corpus policy.

Each `NAME.csv.gz` contains the complete coordinate-keyed grid emitted by
`canivt::read_ivt()`, including the value, symbol, and status columns. Each
`NAME.json` contains its dimension names, bilingual member labels, cell-status
summary, and all warnings. Coordinates are canivt's one-based members in its
dimension order. Compare rows by coordinates, rather than assuming pyivt emits
the same row order. JSON single-element fields may be scalars.

Reproduce with a checkout at the pinned commit and an R library containing
canivt's imports (`cli`, `tibble`, and their dependencies) plus `jsonlite`:

```shell
Rscript scripts/generate_legacy_oracle.R CANIVT_ROOT INPUT_DIR tests/fixtures/legacy R_LIBRARY
```

The seven Health/Agriculture/Small Area samples produce no canivt warnings.
The four Business Patterns samples use canivt's inferred directory-stride
reconciliation. Their warning records are retained: three have explicitly
provisional industry-label assignments, and matching arithmetic totals cannot
establish the assignment of each industry label to a cell. `00060117` carries
four pages without readable status tails, so its stored numeric values are a
stronger reference than its completion of unstated absences. These files are
independent parser references, not native-browser exports.

The separate `*-browser.csv` files are original Professional Browser exports
with **Standard Header** checked. They retain Windows-1252 encoding. Their
[manifest](browser-exports.json) pins the executable, CSV hashes, and selected
coordinates. The tests compare every exported cell: the complete 481-cell
fertility cube, a 42-cell mortality slice, and all 2,898 Newfoundland industry
cells in PROVSIC3. The industry export independently confirms the native label
assignment for that province.

As a separate published check, Canada 1997 in `00060104` is 1,552 children per
1,000 women, or 1.552 per woman. Rounded to two decimals it agrees with the
1.55 in Statistics Canada's *Report on the Demographic Situation in Canada,
1998–1999*, [Table 7, printed page 44](https://www150.statcan.gc.ca/n1/pub/91-209-x/91-209-x1999000-eng.pdf#page=58).
This corroborates that cell's magnitude and units. It does not prove the entire
table: that report's 1995/1996 series differs, and later revised publications
also differ. Exact native-browser checks are documented separately by the parser
investigation.

Adapted from Statistics Canada, Health Statistics at a Glance 1999, Census of
Agriculture 1996, Small Area Business and Labour Data 1996, and Canadian Business
Patterns. This does not constitute an endorsement by Statistics Canada. Data
are supplied under the [Statistics Canada Open Licence](https://www.statcan.gc.ca/en/terms-conditions/open-licence).
