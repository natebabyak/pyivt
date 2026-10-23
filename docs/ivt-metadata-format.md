# IVT summary extraction: evidence and implemented layout

Investigation date: 2026-10-23. Target:
`C:\Program Files\Beyond 2020\Professional Browser\Brwpro.exe`.
SHA-256: `f9c2a065f6b7c110842f9f4f5eea492f7ff1969df50994e81f95cfe105230bae`.

The implementation reads indexed metadata components and expands their zero
bitmaps before interpreting fields. It does not split concatenated printable
strings, identify languages from prose, or treat numbered text in notes as the
table annotation legend.

## Evidence boundary

REA identified the target as a native x64 PE. Its native decompiler was not
configured (`GHIDRA_INSTALL_DIR` absent). GNU objdump was already installed and
was used for focused static disassembly; the executable was not run during
this investigation. Existing `tests/expected/*.json` files were produced by the
repository's browser summary export script and provide the behavioral oracle.

REA records retained in `.cache/ivt-rea-evidence.json`:

- `ev_3fb3b964a4113d711d7db38c3f1e3eaa243bc09b34ec9d61dddf339e9d5279b4`:
  GNU objdump section inventory of Brwpro.exe, unchanged before/after digest.
- `ev_e360e5c7ec706e4d852374c387f6e3aab6b2152cd86d462bc1bb66234cc6bfec`:
  complete PE resource inventory of Breseng.dll, 471 resource payloads.

The function addresses below are ordinary shell disassembly observations,
not REA decompilation Evidence or observations of runtime execution.

## Component compression

The existing `decode::decompress_component` implements the same expansion
visible in native function `0x1402b5740`:

- Byte 0: scalar width in its low nibble; bit 7 indicates compression.
- Byte 1: rank; following little-endian u16 values specify extents.
- A compressed record has `ceil(element_count / 16)` little-endian bitmap
  words after its header. Each word is consumed most significant bit first.
- A set bit consumes one scalar; a clear bit produces an all-zero scalar.

Native anchors: `0x1402b5830` reads and multiplies extents;
`0x1402b59bf` reads a bitmap word; `0x1402b5a84` through `0x1402b5aa6`
implements the byte-scalar copy/zero cases; `0x1402b5acc` shifts the bitmap.

Reproduce with:

```powershell
objdump -d -M intel --start-address=0x1402b5740 --stop-address=0x1402b5b20 'C:\Program Files\Beyond 2020\Professional Browser\Brwpro.exe'
```

## File and language indexes

Header directory descriptors contain a u32 file offset, u32 allocation count,
u32 entry count, and u16 flags. Each directory entry contains a u32 component
offset and two u16 length/allocation values. Empty entries retain their
ordinals; compacting them changes the meaning of references.

| Header offset | Directory |
| --- | --- |
| 544 | Master/table metadata |
| 698 | Missing-value legend |
| 712 | Table annotation/footnote legend |
| 824 + 14 × axis | Dimension metadata |

Master entry 7 is a byte matrix with 32-byte language rows. The sampled files
use language IDs 1 (English), 2 (French), and 6 (Spanish). Row u32 offsets are
0: language ID, 4: FACET entry, 8: notes entry, 12: keywords entry. A zero
FACET reference selects master entry 0; zero notes/keyword references mean
absent. English notes are entry 3. French notes are entry 10 in the census
files and entry 11 in the three-language demo files; this difference is why
following the matrix matters.

## Summary title, author, category, and notes

Native loader `0x140230be0` validates component payload sizes `0x304`,
`0x3cc`, and the older `0xc3`. It copies fields from the expanded payload:

| Payload size | Title offset/width | Author offset/width | Category offset/width |
| --- | --- | --- | --- |
| 0x304 or 0x3cc | 7 / 255 | 0x106 / 255 | 0x205 / 255 |
| 0xc3 | 7 / 60 | 0x43 / 64 | 0x83 / 64 |

The payload begins with `FACET` and a two-byte version. Native anchors for
the larger fields are `0x140230d9c`, `0x140230db5`, and `0x140230dcf`.
The extra 200 bytes in the `0x3cc` variant are not interpreted here.

```powershell
objdump -d -M intel --start-address=0x140230be0 --stop-address=0x140230e90 'C:\Program Files\Beyond 2020\Professional Browser\Brwpro.exe'
```

In the reported census example, master entry 0 is the English FACET component
at offset 8355, stored length 213. Expanding it restores zeros between the
title and `Statistics Canada`. Entry 9 holds French. Empty category fields
are present empty strings, not absent values. All 14 actual fixtures use the
0x304 layout; the two other native layouts are covered by synthetic tests.

Notes follow the language matrix, use the declared component length, and
retain all text. CRLF is normalized to LF and line indentation is normalized
to match the existing browser-generated references. Full English and French
note strings match all 14 JSON references, including their middle sentences.

## Dimension summaries

Master entry 1 expands to a byte matrix of 82-byte axis rows. Relevant row
offsets: u32 item count at 0, u16 index width at 4, u16 axis kind at 6
(0 normal, 1 geographic, 2 time), short name at 12 with width 18, description
at 30 with width 52. Axis kind is distinct from index width: an index width
of six does not make a normal axis a time axis.

Dimension directory entry 3 expands to 86-byte summary rows in language order.
Row offsets: short name 0..18; u16 display member-field ordinal at 18;
u16 dimension-note component reference at 22; description at 34..86.
The member-field ordinal can differ between English and French; the code
uses that ordinal and the existing structured member reader. Dimension notes
can be single-line and do not need to look like prose.

The product of the authoritative axis counts is the File Summary datapoint
count. Member-array capacity and the lengths of unrelated attribute arrays
do not alter it. Names, descriptions, counts, and type labels match all 14
browser references. Note-reference interpretation is corroborated by the
demo records and a single-line mutation regression; dimension notes are not
included in the original browser JSON schema.

## Time settings and legends

Master entry 2 is a 32-byte settings payload. Observed fields include the
published/read-only flag word at payload +2, frequency at +6, and Gregorian
serial dates at +8/+12. The date serial is Python ordinal day plus 306, or
Unix day plus 719469. Frequency zero means `No` and its date fields are not
interpreted, even if a sentinel is nonzero. Sample.ivt stores Jan 1997 through
Mar 2000; other dates mentioned in its footnotes must not override those.
Monthly and semiannual values are corroborated by actual fixtures. The
implementation also maps rates 1 and 4 to Annual and Quarterly; other
unrecognized rates return an absent frequency rather than prose inference.
Read-only values match the fixture references; a regression verifies the
same decoded settings when stored without compression. Read-only bit
semantics outside the supplied records have not been independently traced
through the native summary getter.

Both legend directories begin with an integer matrix (`04 02`) whose rows
specify symbols in display order and whose columns specify languages. Follow
these entry ordinals rather than sorting symbols or guessing languages.
An expanded legend entry has a reserved byte, byte-length NUL-terminated
symbol, then a u16 byte length and description. Decode the component before
parsing these fields. Do not impose a 4,000-character truncation.

For the reported file, slot 712 points to offset 41230 and its index declares
seven bilingual rows: A, B, C, D, E, R, P. Numbered Footnote/Renvoi paragraphs
are part of the table notes and are not these annotation definitions.

## Selective I/O and verification

File inputs load a small header, the declared metadata directories, and their
referenced components. Their offsets are relocated into a compact metadata
image, which is reused by lazy API properties. No file-sized sparse allocation,
full-file fallback, or 4,000-byte descriptor scan is used on this path. A
missing supported structured descriptor produces an error. The bytes API
retains its older descriptor compatibility scanner.

The reported 55,257-byte example reads 49,555 bytes for metadata, in 157
requests; its largest request is 8,632 bytes. The 2,521,835-byte census file
reads 2,100,757 bytes; metadata itself occupies much of these tables. This is
selective access, not a claim that metadata is always a small percentage of
the file. Cube export still intentionally reads the full file.

Tests cover all 14 browser references, complete bilingual notes, path/bytes
parity, an added unreferenced 8 MiB tail, unknown publisher/category text,
misleading language/date prose, three FACET payload sizes, and single-line
dimension notes. Copy-time filesystem timestamps are compared to the current
file stat; they are not part of the SHA-256-addressed IVT contents.

Coverage is bounded to the supplied census files, four installed legacy demo
tables, and the stated mutations. Other native variants, nonempty keyword
records, uncommon frequency codes, and additional language APIs are not
exhaustively validated. Four legacy Arrow Parquet references describe the
browser's selected wide table view and do not match the full-cube Arrow
contract; those comparisons remain outside this metadata change.

Final validation: 103 metadata tests passed; 31 Rust unit tests passed. The
complete Python suite finished with 131 passed and those four wide-view Arrow
reference mismatches. Ruff and `git diff --check` passed.
