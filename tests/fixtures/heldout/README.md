# Independent held-out CSV checks

These unmodified IVTs and CSVs were fetched from Statistics Canada after the
parser implementation was frozen at 9cbaee9. They are part of the disjoint
held-out manifest, not inputs used to choose parser rules. The CSVs are stored in deterministic gzip containers; their decompressed bytes
remain unchanged. The paired JSON
files record original URLs, hashes and independent WDS dimension counts.

- 98-10-0036: Broad age groups and gender: Canada, provinces and territories.
  Three axes, 14 × 3 × 8 = 336 cells.
- 98-10-0042: Structural type of dwelling, age and gender: Canada and census
  metropolitan areas. Four axes, 174 × 3 × 20 × 9 = 93,960 cells.

The tests derive coordinates from the official CSV's `Coordinate` field and
bracketed column ordinals. They compare every value and published symbol,
member IDs, English labels and DGUIDs. No expected values are generated with
pyivt. Native auxiliary fields, French labels and raw footnotes are not covered
by these CSVs; the synthetic typed fixture covers their encoding separately.

Adapted from Statistics Canada, Census of Population, 2021. This does not
constitute an endorsement by Statistics Canada of this product. CSV data are
reproduced and distributed on an “as is” basis with the permission of Statistics
Canada under the [Statistics Canada Open Licence](https://www.statcan.gc.ca/en/terms-conditions/open-licence).
