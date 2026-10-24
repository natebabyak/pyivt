# Beyond 20/20 dimensions and members: internal structure

Investigation: 2026-10-23. Target:
`C:\Program Files\Beyond 2020\Professional Browser\Brwpro.exe`.
SHA-256: `f9c2a065f6b7c110842f9f4f5eea492f7ff1969df50994e81f95cfe105230bae`.
Native x86-64 PE; addresses below are preferred-image virtual addresses.

## Main finding

A dimension is an ordered set of member slots with a schema of typed fields.
Member attributes are stored by field in independently chunked arrays. A member
is reconstructed by joining those arrays at the same slot, rather than reading
one record containing its code, translations, notes and numeric attributes.
The code field additionally contains a pointer to a separate group definition.
Allocated slots, active members and the displayed member order are distinct
concepts; a label string is not a member identity.

```text
table master entry 1: dimension counts, index widths, kinds, names
header dimension slot [824 + 14 * axis]
  -> dimension component directory (direct or paged)
       entry 0: field definitions (34-byte rows)
          field.chunk_index -> u32 vector of component ordinals
             -> chunk A, chunk B, ... -> values in member-slot order
       entry 3: localized dimension summaries (86-byte rows)
          -> display field / member-note field / dimension-note component
       other entries: chunks, note text, group definitions, other metadata

member slot k = Code[k] + label_field[k] + attribute_field[k] + ...
Code[k].group_component -> ordered member slot list (members can have groups)
```

This report describes the file representation and the corresponding native
validation state that could be established. It does not claim a recovered C++
class declaration or complete ABI.

## Evidence and reproduction

REA successfully opened and identified the selected binary. Its session reported
Ghidra unavailable because `GHIDRA_INSTALL_DIR` is unset, with no selected native
provider. Native conclusions below therefore come from GNU objdump static
disassembly, corroborated with original IVT bytes. Brwpro.exe was not executed.
No REA decompilation Evidence IDs were produced for these native conclusions.

The evidence labels in this report are local finding references, not REA IDs:

| Reference | Evidence and scope |
| --- | --- |
| N1 | [Retained native disassembly and diagnostic literals](evidence/brwpro-dimensions-native.txt), code/member validation `0x14022c7f0` through `0x14022d053` |
| N2 | Same capture, field-definition validation at `0x14022eba0`, definition-table validation around `0x14022ef47` |
| N3 | Same capture, field chunk/type validation beginning `0x14022fe30` |
| N4 | Same capture, group loader/validator at `0x140230540` and code-to-group traversal at `0x140230940` |
| F1 | Direct component-byte inspection of 20 repository `data/*.ivt` files and installed Sample, People, Products and Oecd demo tables; selected concrete examples below |
| P1 | Existing parsers in [members.rs](../src/members.rs), [metadata.rs](../src/metadata.rs) and [descriptor.rs](../src/descriptor.rs); implementation evidence only where native confirmation is absent |

The [read-only probe](evidence/brwpro_dimension_probe.py) hashes the executable,
maps PE file offsets to addresses, finds RIP-relative references to selected
ASCII literals, and disassembles selected ranges using GNU objdump. For example:

```powershell
python docs/evidence/brwpro_dimension_probe.py --string 'Undefined language in dimension'
python docs/evidence/brwpro_dimension_probe.py --address 0x14022eba0
python docs/evidence/brwpro_dimension_probe.py --address 0x14022c7f0 --end 0x14022cbc3
python docs/evidence/brwpro_dimension_probe.py --address 0x140230540 --end 0x140230931
```

PE `.pdata` can split a function into several ranges. A returned `.pdata` range
is not necessarily the entire function; explicit `--end` follows the selected
code beyond such fragments. Embedded jump tables and padding are not asserted
to be instructions merely because objdump renders them.

## Dimension identity and localization

Master/table entry 1 has expanded 82-byte dimension rows. Existing fixture
analysis establishes these offsets within each row:

| Offset | Representation | Meaning |
| --- | --- | --- |
| 0 | u32 | Authoritative axis item count |
| 4 | u16 | Bits needed for an item index |
| 6 | u16 | Axis kind: 0 normal, 1 geographic, 2 time |
| 12 | 18 bytes | Short name |
| 30 | 52 bytes | Description |

Index width and axis kind are different fields. For example, the 718-member
normal axis in fixture `941238d8…` has index width 10 and kind 0. Sample's
39-member time axis has index width 6 and kind 2. Widths are not type codes.
These master-row facts extend the existing [metadata report](ivt-metadata-format.md).

Each dimension's directory is reached through the 14-byte header descriptor at
`824 + 14 * zero_based_axis`. Entry addressing can be paged; preserve empty
ordinals and follow directory flags as described in
[paged metadata directories](ivt-paged-directories.md).

Dimension entry 3 expands to 86-byte localized summary rows:

| Row offset | Meaning established by fixtures/current parser |
| --- | --- |
| 0..17 | Dimension short-name storage |
| 18 | u16 display member-field ordinal |
| 20 | u16 member-note field selector (fixture corroboration) |
| 22 | u16 dimension-note component ordinal |
| 34..85 | Dimension description storage |

Field ordinals and component ordinals are different reference domains. In
Sample's Units dimension the field order is Code, French description, English
description, Spanish description, while localized display selectors are
`[2, 1, 3]`. In its Region dimension, display fields are `[1, 5, 6]`, member-note
fields `[8, 9, 10]`, and dimension-note components `[4, 5, 6]`. Do not infer a
language or default label from the position or spelling of a field. [F1]

## Field definition: 34 bytes

Directory entry 0 is a rank-2 byte component with row width 34. Native validation
checks scalar type 1, rank 2 and width `0x22` at `0x14022ef8b`, `0x14022efbc`
and `0x14022efed`. It requires 1 through 32 field rows in this validation path.
Field zero is handled specially as the code/time field. [N2]

All integers below are little-endian; offsets refer to an expanded definition
row, excluding the component header.

| Offset | Size | Recovered interpretation |
| --- | --- | --- |
| 0 | 1 | Name length; general validator warns above 16 |
| 1..16 | 16 | Name bytes, followed by NUL in valid native rows |
| 17 | 1 | Reserved, expected zero |
| 18 | 2 | Type word; native dispatch in the inspected paths uses the low byte |
| 20 | 2 | Blocking exponent for code fields: block capacity = `1 << value` |
| 22 | 2 | Language ID, or zero for an unlocalized field |
| 24 | 2 | Meaning not established; do not fold into the language ID |
| 26 | 4 | Component ordinal of the field's chunk-index vector |
| 30 | 2 | Display column width; code validator warns if below 5 |
| 32 | 2 | Flags, only partly understood |

Native anchors: stride multiplication at `0x14022ebeb`; name length/terminator
checks at `0x14022ec32` and `0x14022ec62`; reserved byte at `0x14022ecfb`;
language lookup at `0x14022ed70` through `0x14022edee`; chunk-index reference
at `0x14022cd01`; blocking exponent at `0x14022cce4`; column width at
`0x14022ccbd`. The blocking exponent is checked in 1..15 for the code path.
Fixture exponents include 10 for code capacity 1024 and 8 for text capacity
256; the text interpretation is corroboration, not a fully traced accessor.

Language IDs are compared to the table's language-ID list. IDs 1, 2 and 6
correspond to English, French and Spanish in the inspected tables. The validator
warns about a nonzero language on field zero. [N2, F1]

Flags at +32 are not a complete recovered enum: the native validator tests low
bits `0x0003` before generic field-content validation and rejects a selected
default label field when bit `0x8000` is set (`0x14022ee35` onward). Other bits,
and the upper byte of the type word, remain uninterpreted here.

One compatibility discrepancy is visible: the current parser permits a field
name length of 17, whereas this native validator warns above 16 and checks byte
17 as reserved. This investigation records that distinction without changing
the parser. [N2, P1]

## Member storage and chunking

A field's +26 reference points to a rank-1 u32 component. Its values are
component-directory ordinals for the field's chunks, in logical order. Different
fields have different chunk vectors and chunk sizes. Neither file offsets nor
adjacent component numbers determine member order. Component zero expansion
must happen before interpreting any of these payloads. [N1, N3, F1]

| Observed definition type | Member representation | Evidence boundary |
| --- | --- | --- |
| `0x0140` | Fixed-width code rows, 22 bytes in the native validator | Native low-byte `0x40` dispatch and fixture rows |
| `0x0180` | Time rows, 8 bytes: serial u32 and auxiliary u32 | Native low-byte `0x80` dispatch and demo rows |
| `0x0001` | Variable text: u16 string count, then byte-length/text/NUL tuples in a byte vector | Fixtures/current parser; native character path calls variable-array validator `0x140232d60` |
| `0x0304` | u32 vector of component references to individual note text records; zero means absent | Fixtures/current parser; native note path requires long storage and follows nonzero component references |
| `0x0302` | Numeric attribute vectors | Fixtures/current parser; native short/long/double validation paths confirm additional numeric storage families |

The complete native type enum is broader than the five observed full type
words. Its general validator accepts low-byte values 1, 2, 4, 8, 32, 64 and
128; inspected content checks include character, short, long, double and boolean
diagnostics. This does not establish every type's complete serialization. [N2, N3]

Expanded component headers contain scalar width/type, rank and u16 extents.
The variable-string payload's count at record +4 is separate from that byte
vector's extent; tuples start at +6. Fixed codes use rank 2, rows begin at +6,
and their width is the second extent. Note-reference targets are individual byte
vectors with an additional text-format byte before their text. [F1, P1]

Concrete fixture examples (filenames identify the records, not parsing rules):

| Fixture / axis | Axis count | Field references and physical capacity |
| --- | ---: | --- |
| `625ff7c387bd3d45af25b0bfd48aff3e186d46d93e749d0632f1840dee806192.ivt`, axis 0 | 14 | Entry 0 at file offset 22297, stored length 526, expands to `01 02 16 00 22 00` plus 22 definitions. Code index entry 1 contains `[2]`; English entry 4 contains `[5]`; French entry 6 contains `[7]`. Chunks have 16 slots. |
| `941238d8af783ad01ca29687cd8d39bc5eb5be1811f52678c336293e3c03a601.ivt`, axis 1 | 718 | Code index entry 1 contains `[2]`, capacity 1024. English entry 4 contains `[5, 8, 10, 11]`; French entry 6 contains `[7, 9, 12, 13]`. Each text chunk has 256 slots. |
| `cf698c42fbb1ae18e0527fe167a5e269fe4ad8064ffaf564f67d2aed6669460c.ivt`, axis 0 | 63404 | Code vector has 64 chunks, beginning `[2, 94, 167, 168, 313, 314, 315, 316]`; English vector has 256 chunks, beginning `[5, 40, 58, 59, 95, 96, 97, 98]`. |

Trailing empty capacity must not create members or change the cube's declared
axis size. Conversely, deletion holes must not be compacted without an explicit
slot-to-exported-member mapping. pyivt's exported member IDs are one-based;
the native group validator indexes its validity arrays with zero-based slot
numbers. Their equivalence for files containing holes has not been established.

## Code rows, validity and groups

For low-byte type `0x40`, native validation requires 22-byte rows. It reads the
first byte as a signed status/length: zero is inactive, a negative value is a
deleted entry, and a positive value is active. It checks reserved byte +17,
uses the code string starting at +1 for duplicate detection, and marks the
corresponding slot in a validity array. [N1: `0x14022c954`, `0x14022c9b1`,
`0x14022c9be`, `0x14022c9fe`, `0x14022ca06`, `0x14022ca3e`]

The remaining u32 at row +18 is a group-component ordinal. Native traversal
at `0x140230940` reads the code field's chunk index, walks code rows at stride
22, and passes positive group ordinals to `0x140230540`. A group pointer on an
invalid slot produces a diagnostic. Thus explicit group membership exists
separately from label indentation. [N4]

The inspected nonempty group format is a rank-1 byte component. Within its
expanded payload, excluding the four-byte component header:

| Payload offset | Representation | Native use |
| --- | --- | --- |
| 0 | u16 | Group kind; inspected validation admits 1, 2 and 9 |
| 2 | i32 | Owner/item slot, required to identify a valid slot |
| 6 | u32 | Number of member slots; inspected validator limits it to 8000 |
| 10 | u32[count] | Explicit member slot indices |
| `10 + 4 * count` | Additional data for kind 2 | u16 length and trailing text storage; diagnostic identifies a computed-group definition |

Anchors: group-kind check `0x1402306d6`; count and base size `0x1402306ef`;
computed suffix check `0x140230738`; owner validity `0x140230780`; count limit
`0x1402307a5`; member bounds/validity/duplicate checks `0x140230818` through
`0x14023089c`. Kind names for 1 and 9 and the computed expression grammar are
unknown. The minimum-size check is 14 bytes in this path. A separate empty
short-vector case is diagnosed, rather than treated as this payload layout.

Fixture inspection confirms actual nested groups: Sample's Projects axis,
slot 0 (All Projects), has this exact 22-byte Code row:

```text
0c416c6c2050726f6a6563747300000000000d000000
```

Its u32 at +18 is 13. Directory entry 13 expands to:

```text
01011600 0100 00000000 03000000 01000000 05000000 09000000
header   kind owner    count    members: 1, 5, 9
```

The component header says rank-1 byte vector, payload length 22. The owner is
slot 0, and its member list is `[1, 5, 9]`. Those slots themselves reference
groups: Mammography (1) contains `[2, 3, 4]`, Prenatal Care (5) contains
`[6, 7, 8]`, and Substance Use (9) contains `[10, 11]`. In Oecd, WORLD contains
`[1, 5, 27, 23]`, proving that stored group order can differ from numeric slot
order. Across the four demos, all 26 nonzero Code-row group references resolve
to kind-1 records with matching owner slots and exactly `10 + 4 * count`
payload bytes: Oecd 5, People 8, Products 5, Sample 8. [F1]

These structures support nested, ordered group membership, but do not by
themselves establish a universal single-parent tree. Shared-child restrictions,
cycle handling, aggregation semantics and UI traversal have not been traced.
The two-space indentation in labels such as
`"  Abbotsford (Large urban population centre)"` is independently stored text.
Enumerating declared fields in the 20 repository IVTs and four installed demos
found no explicit Parent/Level/Hierarchy field; that bounded search does not rule
out other hierarchy metadata or native state. [F1]

## Time records and native validation state

Type `0x80` uses eight-byte rows and treats a zero first u32 as inactive.
Nonzero serials are checked against the table's date interval; minimum and
maximum actual serials are compared with its recorded first/last dates. The
auxiliary u32 is preserved by pyivt, but its meaning was not established.
[N1: `0x14022cab5` through `0x14022cb6f`; `0x14022cef7` onward]

Demo first rows are Sample `(729331, 0)`, People/Products `(727139, 0)` and
Oecd `(723121, 0)`. The existing fixture-correlated date conversion is
`Unix day = serial - 719469`; that conversion is not newly recovered native
calendar code in this pass. [F1, P1]

The validator also exposes a partial native object layout. These are offsets
from the validation-context pointer, not on-disk offsets or a named C++ class:

| Context offset | Observed role |
| --- | --- |
| `0x30 + 4 * language_index` | Table language IDs |
| `0x64 + 4 * axis` | Default member-label field ordinal |
| `0x128` | Component/subfile reader pointer |
| `0x140` | Base subfile number; dimension lookup adds axis |
| `0x148`, `0x14c` | First and last date serials |
| `0x150 + 8 * axis` | Pointer to per-slot validity bytes allocated during validation |
| `0x1e8 + 4 * axis` | Allocated slot count checked against code storage |
| `0x218 + 4 * axis` | Active item count checked against populated code/time slots |
| `0x244` | Number of table languages |

The code validator accumulates active and allocated counts separately and
compares them at `0x14022ce83` and `0x14022cebf`. This is direct evidence that
native slot capacity and active item count are not interchangeable. [N1, N2]

## Coverage and implications for pyivt

| Investigation question | Status |
| --- | --- |
| Dimension directory and field schema | Answered for the inspected native validation path and fixture layouts |
| Member codes, labels, localization and chunk alignment | Answered for the observed storage types; native type enum only partial |
| Active/deleted slots and separate allocation counts | Native checks established; export mapping with holes unresolved |
| Explicit groups versus label indentation | Native pointer/list checks and 26 demo groups corroborated, including nesting and stored child order; aggregation/UI behavior unresolved |
| Native in-memory classes and display order | Partial validation-context offsets only; class ownership and ordering algorithms unresolved |

For future parser work, retain field definitions and flags, expose code/group
references without conflating them with translated labels, and preserve native
slot indices until any exported-ID mapping is explicit. Display selectors should
choose the relevant field; no fixed English/French field ordinal is safe.
Leading whitespace is meaningful stored label content. Do not derive a parent
tree from it without declaring that inference.

This investigation adds documentation and reproducible evidence only. It does
not implement group decoding, change member-ID behavior, or modify production
parsers. Static validation paths establish the checks described above, not
observed application execution or exhaustive support for every IVT version.

Demo content identities for reproducing F1 (under
`C:\Program Files\Beyond 2020\Professional Browser\Data`):

| File | SHA-256 |
| --- | --- |
| Sample.ivt | `6268c7b79e4720360ba960cc5ab45643fa572b9fb265305646977185b886ca01` |
| People.ivt | `3c69fe81afee0ad5ef6775b3182b5a084bea6c5719e2829069bbaf5d87d27ef1` |
| Products.ivt | `ba4e6142cf9bafa1520dcb119b2fd9b8a14459f137d383e7926c98bd35818a4d` |
| Oecd.ivt | `ed23e0b5d8c869296889e75bf2da3eb4b2db6f7b828f7057bcb606afcb4b8297` |
