# Share scan failures

Date: November 15, 2026. Scope: Linear NAT-245. The 26 files from the StatCan
Data and LMIC share scan are private and were not available while this work was
done. Nothing here establishes that any of them is corrupt or supported. That
needs the re-scan below.

## Classification rule

Every rejected file now reports a verdict before the detail:

- `Corrupt IVT file:` means the evidence is at the file envelope. The signature
  or a fixed header word is wrong, the file is shorter than its header, or a
  slot table, directory or record lies outside the file. pyivt stops there and
  does not try to parse around the damage.
- `Unsupported IVT file:` means the envelope is valid but a record inside it
  uses a layout pyivt does not recognise. The descriptor message names the
  variant, so a re-scan gives what is needed to add support.

An offset past EOF fits a truncated copy, but it also fits a directory that
pyivt misreads. Messages say the file "appears truncated" and state both sizes.
Comparing with a fresh copy or Beyond 20/20 settles it.

Exceptions remain `ValueError`. NAT-257 tracks distinct exception types. The
Rust `IvtError::kind()` already separates the two verdicts for that mapping.

## Expected messages by scan failure type

| Scan error | Now reported as |
| --- | --- |
| `unsupported IVT descriptor: native axis matrix is missing or invalid` (14) | `Unsupported IVT file: descriptor layout not recognised (container version V): <reason>`. The reason is the matrix component header and row width, axis count, failing axis fields, an empty entry 1, or the record's first bytes when it cannot be decoded. |
| `unrecognised IVT signature` (6) | `Corrupt IVT file: not an IVT file or its header is corrupted: the first bytes are .., but IVT files start with 02 00 20 00 or 04 00 20 00`, plus a hint for zero-filled or HTML content. |
| `IVT metadata record extends past EOF` (4) | `Corrupt IVT file: the file appears truncated: the <slot> record at entry N needs at least E bytes but the file is M bytes (likely an incomplete copy)` |
| `invalid directory: metadata directory range` (1 file, 2 copies) | `Corrupt IVT file: the <slot> directory points outside the file: it declares N entries (flat or paged) at byte O, but the file is M bytes` |

[`test_failure_classification.py`](../../tests/test_failure_classification.py)
covers each type through both path and bytes inputs. The signature cases use
the six leading byte sequences from the scan report, and the EOF case uses its
offset-to-size ratio.

## Re-scan

Run this over the share folders, or over the 26 files, with pyivt from this
branch installed:

```sh
python -m scripts.triage_files "<share folder>" ... --output triage.jsonl
```

Each row records `status` (`opened`, `corrupt`, `unsupported`, `failed`,
`unreadable`), the message, size, SHA-256, the first 32 bytes and the trailing
zero byte count. `--values` also converts every cell of the opened files in a
memory-capped subprocess. Matching hashes confirm that the two
`4ctl_abo_uhrs_3MMA.ivt` copies are the same file.

To close a file as corrupt, record its row and one independent check: a fresh
copy with a different size or hash, or Beyond 20/20 refusing it. For an
`unsupported` descriptor, the reason names the variant to implement. Promote
one such file into the development corpus before changing the parser.
