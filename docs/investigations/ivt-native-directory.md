# Beyond 20/20 native directory evidence

Investigation date: 2026-10-28. These are static observations from the installed
Beyond 20/20 Professional Browser. Disassembly establishes the byte-addressing
algorithm; it does not establish that the inaccessible NAICS234 file uses this
variant or that its complete output has been validated.

## Artifact and method

- Target: `C:\Program Files\Beyond 2020\Professional Browser\Brwpro.exe`.
- SHA-256: `f9c2a065f6b7c110842f9f4f5eea492f7ff1969df50994e81f95cfe105230bae`.
- PE32+, x86-64, preferred image base `0x140000000`, 10,399,744 bytes.
- REA `open_binary` identified this artifact. Ghidra and IDA were not configured
  in the active session, so the focused native analysis below used the installed
  GNU objdump through ordinary shell tools. These are **shell observations, not
  REA Evidence records**, and no pseudocode or runtime trace is claimed.
- The binary contains a source-location string
  `Packages\Ifile\ifile.Cpp`, referenced by the examined routines' error paths.
  This identifies a useful search region; conclusions below rely on instructions,
  rather than the string alone.

Reproduce the relevant disassembly in PowerShell:

```powershell
$binary = 'C:\Program Files\Beyond 2020\Professional Browser\Brwpro.exe'
$objdump = 'C:\msys64\ucrt64\bin\objdump.exe'
Get-FileHash -Algorithm SHA256 -LiteralPath $binary
& $objdump -h $binary
& $objdump -d -M intel --start-address=0x1402b4440 --stop-address=0x1402b448b $binary
& $objdump -d -M intel --start-address=0x1402b65a0 --stop-address=0x1402b6822 $binary
& $objdump -d -M intel --start-address=0x1402b6b57 --stop-address=0x1402b6d25 $binary
& $objdump -d -M intel --start-address=0x1402b6dcc --stop-address=0x1402b6e81 $binary
& $objdump -d -M intel --start-address=0x1402b5cab --stop-address=0x1402b5cdb $binary
& $objdump -d -M intel --start-address=0x1402b700f --stop-address=0x1402b703c $binary
```

`.text` starts at file offset `0x400`, RVA `0x1000`. Consequently, the file offset
of an instruction here is its virtual address minus `0x140000c00`.

## Header and slot observations

The open routine at `0x1402b6b57..0x1402b6c34` reads the first 32 bytes from file
offset zero and copies them into the native object at displacement eight:

```text
1402b6b57  lea r9,[rbp-0x30]       ; header destination
1402b6b5b  xor edx,edx             ; file offset 0
1402b6b5d  lea r8d,[rdx+0x20]      ; length 32
1402b6b65  call 0x140315e20        ; read
1402b6be2  movups xmm0,[rbp-0x30]
1402b6be6  movups [rax+0x8],xmm0
1402b6bea  movups xmm1,[rbp-0x20]
1402b6bee  movups [rax+0x18],xmm1
1402b6bf2  movzx eax,WORD PTR [rbp-0x20] ; raw header +16: slot count
1402b6bf6  imul ecx,eax,0xe        ; allocate slot count * 14
1402b6c25  imul r8d,eax,0xe        ; read length: slot count * 14
1402b6c2d  mov edx,[rbp-0x24]      ; raw header +12: slot-array offset
1402b6c34  call 0x140315e20
```

The validator at `0x1402b4440..0x1402b448a` checks these raw header fields:

| Raw header byte | Native check | Interpretation supported by later use |
| --- | --- | --- |
| 0 | `u16` in `2..=4` on the ordinary route | container version |
| 2 | `u16 == 0x20` | header size, 32 bytes |
| 8 | `u16 == 0x40` | observed invariant; meaning not resolved here |
| 10 | `u16 == 8` | directory-entry size |
| 16 | `u16 == 0x20` | 32 directory slots |
| 18 | `u16 == 0x0e` | 14 bytes per slot |
| 20 | `u16 == 0x1000` | 4096 entries per directory page |

All 20 local corpus files have `u16@20 == 4096`; their data slot paging flags are
zero. A local example's first 24 bytes are:

```text
04 00 20 00 20 00 00 00 40 00 08 00 20 02 00 00 20 00 0e 00 00 10 00 00
```

The slot-array pointer `u32@12` is `544` (`0x220`), so slot zero is at byte 544
and slot one at byte 558. This agrees with the existing parser's master and data
slot locations. Each slot has this native-consumed representation:

```text
u32 +0   absolute file offset of the flat directory or page-pointer table
u32 +4   allocated entry capacity
u32 +8   declared entry count (ordinal upper bound)
u16 +12  flags; bit zero selects a page-pointer table
```

## Directory resolution, observed in instructions

Function `0x1402b65a0..0x1402b6821` takes a slot index and an entry ordinal.
The following excerpts omit allocation, caching, and error-reporting paths.

```text
1402b65bc  mov eax,r8d                    ; entry ordinal
1402b65bf  movzx r10d,WORD PTR [rcx+0x1c] ; raw header +20
1402b65c9  xor edx,edx
1402b65cb  div r10d                      ; ordinal / page_entries
1402b65ce  imul rsi,r11,0xe              ; slot_index * 14
1402b65d7  add rsi,[rcx+0x48]            ; slot-array buffer
1402b65db  mov r15d,eax                  ; page index = quotient
...
1402b66a3  movzx ecx,WORD PTR [rsi+0xc]   ; slot flags
1402b66a7  test cl,0x1
1402b66aa  je 0x1402b6757                ; flat route
...
1402b66d1  mov eax,[rsi+0x4]             ; allocated entry capacity
1402b66d4  div ecx                      ; divide by page_entries
1402b66de  shl r14,0x2                  ; pointer-table length = pages * 4
...
1402b6711  mov edx,[rsi]                 ; pointer-table file offset
1402b6716  call 0x140315e20              ; read pointer table
...
1402b6747  mov r14d,[rcx+r15*4]          ; u32 pointer[page index]
1402b6755  jmp 0x1402b675a
1402b6757  mov r14d,[rsi]                ; flat directory file offset
1402b675a  test cl,0x1
1402b675d  je 0x1402b6765
1402b675f  movzx esi,WORD PTR [rdi+0x1c] ; paged length: page_entries
1402b6763  jmp 0x1402b6768
1402b6765  mov esi,[rsi+0x4]             ; flat length: allocated capacity
1402b6768  lea eax,[rsi*8]               ; directory bytes = entries * 8
...
1402b67aa  mov edx,r14d                  ; selected directory/page file offset
1402b67b0  call 0x140315e20              ; read directory/page
...
1402b67e8  movzx ecx,WORD PTR [rdi+0x1c]
1402b67ec  xor edx,edx
1402b67ee  mov eax,[rsp+0x80]            ; original entry ordinal
1402b67f5  div ecx                      ; remainder = within-page ordinal
1402b67ff  lea rax,[r8+rdx*8]            ; address of selected 8-byte entry
1402b6803  mov [rcx],rax
```

The high-level component read checks its ordinal against the declared count,
rather than the allocated capacity:

```text
1402b6dcc  imul rcx,r13,0xe
1402b6dd0  cmp r12d,[rcx+rax+0x8]        ; ordinal versus slot +8
1402b6dd5  jb 0x1402b6df6                ; only ordinal < count proceeds
```

Derived address formula, for `P = u16(header,20)` and `k < declared_count`:

```text
flags & 1 == 0:  entry_position = slot.offset + 8*k
flags & 1 != 0:  page_offset = u32(file, slot.offset + 4*(k/P))
                entry_position = page_offset + 8*(k%P)
```

These are absolute file pointers. No data-section base is added. Consecutive
fully allocated directory pages contain `4096 * 8 == 32768 == 0x8000` bytes.
Therefore the reported `0x13f0, 0x93f0, 0x113f0, ...` pattern is consistent with
absolute pointers to consecutive directory pages, rather than encoded offsets
inside value pages. The inaccessible original file's flag and header must still
be read to establish that it actually uses this path.

## Entry length and blank-page observations

The first length is content length; the second is allocation length. The native
reader does not choose the minimum of two redundant copies:

```text
1402b6e1e  mov r14d,[rax]               ; entry +0: component file offset
1402b6e21  test r14d,r14d
1402b6e24  je 0x1402b700f              ; absent component
1402b6e2a  movzx ebp,WORD PTR [rax+0x4] ; content length
...
1402b6e4f  mov r8,rbp                  ; read exactly content length
1402b6e52  mov edx,r14d                ; absolute component offset
1402b6e58  call 0x140315e20
```

The release path uses the other length to release the allocated region:

```text
1402b5cb3  movzx r8d,WORD PTR [rsi+0x6] ; allocation length
1402b5cb8  mov edx,[rsi]               ; component offset
1402b5cba  call 0x1402b6260
```

An entry with offset zero is represented by the native reader as a rank-one,
zero-length short component. At `0x1402b700f..0x1402b702a` it calls the component
constructor with width 2, rank 1, extent 0, and no extended records. This supports
the existing parser's zero-page interpretation. Malformed nonblank entries must
still be rejected, not converted into this representation.

## Limits and unresolved questions

- No original NAICS234 bytes were available at the supplied UNC path. Its header
  flags, exact directory size, all cell values, and independent full export remain
  unverified by this investigation.
- `flags & 1` is confirmed. The meaning of other flags was not resolved. Clearing
  bit 31 from cached page pointers appears in the native memory-cache path at
  `0x1402b66c1`; the cold on-disk read at `0x1402b6747` does not clear it. This is
  insufficient evidence to mask arbitrary high bits in on-disk pointers.
- The native reader loads allocated page capacity, while a safe offline parser
  can read only the entries needed by the declared count. This does not justify
  reading beyond the declared count or treating allocation slots as records.
- This work does not establish monotonic physical page/component order. Components
  and directory pages must be resolved through their recorded pointers, including
  noncontiguous or physically reordered pages.
- Static native addressing evidence must be combined with an independent data
  oracle. Parser success or a self-comparison alone cannot validate the output.

## Bounded typed-member follow-up

The same binary was inspected statically for field definitions and scalar
dispatch. The following observations are additional native evidence, not a claim
that every member-field representation has been reverse engineered.

```powershell
& $objdump -d -M intel --start-address=0x14022ef47 --stop-address=0x14022f02d $binary
& $objdump -d -M intel --start-address=0x140259420 --stop-address=0x1402594f1 $binary
& $objdump -d -M intel --start-address=0x1401f3f61 --stop-address=0x1401f408e $binary
& $objdump -d -M intel --start-address=0x1401f4254 --stop-address=0x1401f43a5 $binary
& $objdump -s --start-address=0x1401f44bc --stop-address=0x1401f455c $binary
```

The dimension loader at `0x14022ef47..0x14022f02d` requests dimension-directory
entry zero and checks its decoded component properties:

```text
14022ef5d  xor r8d,r8d                ; record ordinal zero
14022ef67  call 0x1402b2b50           ; component read
14022ef8b  cmp DWORD PTR [rsp+0x5c],1 ; byte width
14022efbc  cmp DWORD PTR [rsp+0x60],2 ; rank two
14022efed  cmp DWORD PTR [rsp+0x68],0x22 ; 34-byte rows
```

The field reader at `0x140259420..0x1402594f0` computes
`definition + field_ordinal*34`, reads the type word at row byte 18, and reads the
chunk-index record ordinal at byte 26:

```text
14025947f  imul r8,rbx,0x22
140259483  add r8,[rdi+r14*8+0x1464]
14025948b  movsx edx,WORD PTR [r8+0x14] ; another field property
140259492  movsx eax,WORD PTR [r8+0x12] ; native field type
140259497  movzx edx,al                ; low byte for scalar dispatch
1402594a5  mov r8d,DWORD PTR [r8+0x1a] ; chunk-index record
1402594a9  call 0x1402b2b50
```

The display path at `0x1401f3f61..0x1401f3f8a` uses the low type byte as a switch.
The actual jump-table bytes at RVA `0x1f44bc`, with selectors at RVA `0x1f44dc`,
resolve these observed cases:

| Low type byte | Target address | Observed behavior |
| --- | --- | --- |
| `0x01` | `0x1401f4367` | retrieves text through `0x140394500` |
| `0x02` | `0x1401f40c2` | signed short load at `0x1401f40f2`, formatted as integer |
| `0x04` | `0x1401f4254` | 32-bit load at `0x1401f4282`, formatted as integer |
| `0x08` | `0x1401f42d7` | double load at `0x1401f4304`, formatted as floating point |
| `0x20` | `0x1401f43d3` | packed bitmap selecting one of two texts |
| `0x40` | `0x1401f3f8c` | fixed-code rows: 22-byte stride at `0x1401f3fb3`, text begins at row +1 |
| `0x80` | `0x1401f4022` | eight-byte time rows, first `u32` passed to date formatter `0x1402c75a0` |

The native initialization path also writes full type words `0x0180` at
`0x140229363..0x140229368` and `0x0140` at
`0x14022969b..0x1402296a8`. This corroborates the existing source's time and code
field families. The synthetic typed Arrow fixture uses the observed 22-byte code
row stride.

The following remain outside this bounded static pass:

- The upper type byte's complete meaning, including the interpretation of full
  words `0x0302` and `0x0304`, was not established here. A low-byte numeric display
  branch alone does not prove or disprove the source parser's higher-level
  reference-field handling. Existing `src/members.rs` behavior and its tests must
  not be described as fresh native evidence for those upper-byte semantics.
- The original date serial's Gregorian origin and the source conversion
  `serial - 719469` were not independently derived from native date arithmetic in
  this pass. Native code confirms the eight-byte rows and use of the first word
  for date display; the auxiliary word's meaning remains unresolved.
- No complete CSV export or UI behavior was observed. Hand-authored typed tests
  validate explicit expectations for known record encodings, not the complete
  externally supplied file or every Beyond export variant.
