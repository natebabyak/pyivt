# Older IVT support

Investigation and verification date: November 10, 2026. Branch:
`feature/support-older-files`. Scope: Linear NAT-222, NAT-223, and NAT-224.

## Observed formats and implementation

The development corpus contains 11 public version-2 tables from Statistics
Canada collections in Borealis. The [manifest](../../tests/corpus/legacy.json)
pins download URLs, published sizes and MD5s, acquired SHA-256s, and independent
reference hashes. The files contain 103,856 cells in total.

Every collected version-2 file contains a native master axis matrix with
82-byte rows. The existing structured descriptor reads its names, counts, and
axis kinds. No descriptor reconstruction from prose or guessed counts is needed.
Version 4 retains its existing matrix requirement.

The version-2 matrix also records an allocation exponent at row offset 4.
The Business Patterns industry axes allocate 2,048 slots even when they have
only 19 or 322 members. The allocation determines page and directory strides.
Reconstructing allocation from the visible member count addresses the wrong
pages. The reader now uses the recorded allocation for version 2.

Fixed-width Code chunks contain empty rows between live members. Their decoded
row positions supply the physical cell slots. Public member codes remain dense,
one-based positions in file order. Member attributes use the same map. A zero
Pascal-string length marks an empty Code row even when old text bytes remain
after that length.

Time fields contain eight-byte records with date serials. Zero-date rows retain
deleted slots. In `tb611996`, the three members occupy physical slots 0, 1, and 3
and represent 1996, 1997, and 1995. The reader keeps those public member positions
and addresses their physical slots separately.

Older bilingual summary matrices can appear after directory entry 3. The
version-2 reader locates a unique, correctly shaped 86-byte-row matrix within
the declared dimension directory. Its display references select English and
French member fields, including Code when the file explicitly selects it.

ZIP inputs select the sole IVT automatically. `member=` selects an exact member
when several exist. Extraction streams into an owned temporary file, so lazy
metadata access remains valid after the original ZIP closes.

Both input routes validate the same metadata catalog. The reader validates the
native header constants and reads the slot table at its declared file offset.
Only the 12 dimension slots within the declared 32-slot table are available.
Metadata relocation reads the slot table separately, including when the table
is near EOF. It does not allocate a prefix extending through that file offset.

## Native evidence and its limits

REA `open_binary` identified the installed Professional Browser executable:

`C:\Program Files\Beyond 2020\Professional Browser\Brwpro.exe`

SHA-256: `f9c2a065f6b7c110842f9f4f5eea492f7ff1969df50994e81f95cfe105230bae`.

REA's static PE resource inventory is retained in
[legacy-rea-evidence.json](legacy-rea-evidence.json), Evidence ID
`ev_5dbb3178ed7641c9f7c0ffb7f9bfaa96a313c62fe15aa79b943f03c75b45eb39`.
That record establishes artifact identity and resource inventory, not parser
behavior. `analyze_function` reported `provider_unavailable`: Ghidra and IDA are
not configured, and Hopper does not support this host. No engine was installed.

The installed GNU objdump reproduced the header validator at
`0x1402b4440..0x1402b448a`. Its instructions accept versions 2 through 4 on the
ordinary route and check header words at offsets 2, 8, 10, 16, 18, and 20.
These are shell observations, consistent with the existing
[native directory investigation](ivt-native-directory.md). They are not REA
decompilation or execution evidence.

The allocation and live-slot interpretations above are derived from the native
records, complete independent value comparisons, and native browser exports.
They are not claims that a decompiler recovered the original source.

Professional Browser opened the fertility, time-hole, and sparse-industry files. Its CSV
exports independently verify the selected coordinates, member labels, and
values. The [export manifest](../../tests/fixtures/legacy/browser-exports.json)
records each selection and pins the original CSV bytes. CSV views can cover a
slice rather than the full cube. The tests assert every exported cell and state
each export's coverage.

The saved native CSVs cover 3,421 cells. The fertility export covers its complete
cube. The mortality and industry exports cover the selections in the manifest.

## Verification coverage

| Requirement | Evidence |
| --- | --- |
| Direct StatCan ZIP paths and bytes | Original ZIP and official CSV fixture, all 154 cells, multiple-member and lifecycle tests |
| Real version-2 values | All 103,856 cells across 11 pinned inputs compared with independent canivt output |
| Bilingual members and dates | Full member comparisons where the reference supplies reliable labels, native field-reference assertions, and browser CSV comparisons |
| Sparse industry slots | All 89,089 Business Patterns cells compared by member labels, plus native browser industry export |
| Path and bytes parity | Metadata, bilingual members, full Arrow output, and detailed stream comparisons for every legacy input |
| Dimension slot bounds | A realistic 12-axis, 4,096-cell fixture with non-slot bytes immediately after slot 31 |
| Relocated slot table | Synthetic version-4 and real version-2 copies with their slot table moved beyond a 1 MiB gap |
| Corrupt and cut metadata | Identical contextual errors for both routes, including deliberate cuts of a pinned real input |
| Version-4 regression | All 20 training outputs match published 0.0.9, plus complete comparisons across ten held-out inputs |

The [independent reference notes](../../tests/fixtures/legacy/README.md) retain
canivt's warnings. Its Business Patterns label assignments were provisional.
The native industry CSV separately checks the exported assignment. Its EMPLOY1
English metadata mixes languages and some names include bytes after a NUL.
Those reference defects do not replace the native summary matrices. Numeric
coordinate comparisons still cover that complete table.

The [held-out comparison](heldout-compatibility.md) covers 882,706,378 rows.
Existing memory refusals remain explicit. Complete partition exports cover every
cell in files that exceed a whole-export limit. Release equality is regression
evidence. The two frozen official CSV tests provide separate independent
version-4 value checks.

The final local Python suite passed 446 tests and skipped 16. Rust passed 48
default tests. The separately requested installed-demo member and date test
also passed. Rust formatting, Python lint and formatting, public API type
checks, fuzz-feature compilation, and diff whitespace checks passed. Git
attributes preserve the exact bytes of hashed CSV, JSON, and JSONL references.

No original user file reproducing NAT-224 was supplied. Every collected complete
input opens. The deliberately cut fixtures have known original sizes and SHA-256s.
An out-of-range pointer alone does not prove download truncation, so errors say
that the record extends past EOF and include its slot, ordinal, lengths, and
file size. On-disk bit 31 is not silently masked.

## Version 3 search

The [search record](../../tests/corpus/legacy-version-search.json) retains 45
successful, unique header reads across four historical collections. It found
41 version-2 files and four version-4 files. The installed four IVT demos are
also version 4. Failed requests establish no format facts and are recorded
separately. No version-3 sample was found. Version 3 remains explicitly
unsupported and has path and bytes rejection tests.

## Reproduce the checks

```sh
python scripts/download_test_inputs.py --manifest tests/corpus/training.json
python scripts/download_test_inputs.py --manifest tests/corpus/legacy.json
cargo test --lib
pytest tests/test_legacy_files.py tests/test_directory_bounds.py tests/test_zip_inputs.py
python -m scripts.verify_v4_baseline --baseline tests/corpus/v4-baseline.json
```

The [held-out report](heldout-compatibility.md) records the separate full-corpus
comparison command and release artifact identity. The legacy fixture directory
contains the command for regenerating its independent references.
