# Paged metadata directories

Investigation: 2026-10-23. Selected binary:
`C:\Program Files\Beyond 2020\Professional Browser\Brwpro.exe`.
SHA-256: `f9c2a065f6b7c110842f9f4f5eea492f7ff1969df50994e81f95cfe105230bae`.

REA opened and identified this x64 PE. Native-provider analysis was unavailable
because Ghidra and IDA were not configured. The observations below come from
GNU objdump static disassembly and original fixture bytes, not REA pseudocode
or runtime observation. The REA session was closed after investigation.

## Native addressing observations

The directory accessor at `0x1402b65a0` divides the requested entry ordinal by
the page capacity (`0x1402b65bf` through `0x1402b65cb`). Directory descriptors
are 14 bytes. Bit 0 of their u16 flags at descriptor +12 selects indirection
(`0x1402b66a3` through `0x1402b66aa`).

For indirect directories the descriptor's u32 pointer addresses a u32 page
pointer array, indexed by the quotient (`0x1402b66bd`, `0x1402b6747`). The
remainder selects an eight-byte entry in that page (`0x1402b67e8` through
`0x1402b6803`). Otherwise the descriptor points directly at the entries.
The reader uses a u16 stored at object +28 for page capacity. Native header
validation at `0x1402b4440` checks the on-disk u16 at header +20 against
4096 (`0x1402b4479` through `0x1402b4482`). The implementation reads this
declared capacity and rejects zero.

Reproduce the relevant disassembly with:

```powershell
objdump -d -M intel --start-address=0x1402b65a0 --stop-address=0x1402b6822 'C:\Program Files\Beyond 2020\Professional Browser\Brwpro.exe'
objdump -d -M intel --start-address=0x1402b4440 --stop-address=0x1402b448b 'C:\Program Files\Beyond 2020\Professional Browser\Brwpro.exe'
```

## Failing fixture and implementation

In `cf698c42fbb1ae18e0527fe167a5e269fe4ad8064ffaf564f67d2aed6669460c.ivt`,
dimension slot 824 declares 5298 entries, capacity 8192, and flags 1.
Its pointer 2598159 addresses two page pointers: 2532623 and 2565391.
Those pages contain 4096 and 1202 active entries. All their record ranges
are inside the file. Treating the pointer array as entries instead consumes
adjacent text as offsets and causes `metadata record range`.

`metadata_directory.rs` centralizes direct and paged addressing. Master,
dimension, legend, member, descriptor, and table-settings readers share it.
Selective file reads flatten active directory entries into the metadata
catalog, relocate their records, and clear only the paging flag in that
catalog. Empty entries keep their ordinals. Unused allocated entries are
not read, and malformed page pointers return errors.

Regression coverage includes the real fixture, ordinal 4095/4096 boundaries,
holes, partial pages at EOF, invalid pointers and zero page size. Mutated
fixtures exercise paged master, missing-value legend, annotation legend,
and member directories with nonadjacent pages, checking both path and bytes
metadata and Arrow parity. No filename or hash selects parsing behavior.

## Arrow differences revealed after directory repair

The existing Parquet reference is produced by canivt. Its installed 0.5.0
package identifies commit `219486b22413f42971ff6d8b3e26b1498af3a01e`.
Its [decoder](https://github.com/mountainMath/canivt/blob/219486b22413f42971ff6d8b3e26b1498af3a01e/R/decode.R)
and [page-status reader](https://github.com/mountainMath/canivt/blob/219486b22413f42971ff6d8b3e26b1498af3a01e/R/status.R)
publish numeric zeros on pages without a missing-value mask and represent
legacy mask missingness with code -1 and no named reason. Positive codes
refer to explicit reasons in extended records.

The fixture has 155 masked legacy pages, 92 legacy pages without a mask,
and one extended page. Exactly 60 real zero-valued cells on mask-free pages
were incorrectly made null by pyivt; 904 legacy masked missings were
incorrectly assigned the first legend's named reason. The decoder now follows
the reference contract for both cases. Regression tests retain the distinction
between unnamed legacy masks and positive extended codes.

These Arrow semantics are corroborated by the fixture/reference comparison
and reference implementation. Their independent native getter/runtime trace
remains unestablished; the native observations above establish directory
addressing only. Other unknown native flag bits remain uninterpreted.
