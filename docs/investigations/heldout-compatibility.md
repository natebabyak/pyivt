# Held-out v4 compatibility

On November 10, 2026, all ten pinned inputs in
[`tests/corpus/heldout.json`](../../tests/corpus/heldout.json) matched the published
pyivt 0.0.9 release. Complete exports covered 882,706,378 rows from
1,173,668,079 input bytes. The manifest remained unchanged.

This recorded run used the candidate native binary identified below, including
the change to read the slot-table position from the header. The binary hash
binds this result to the tested build.

The comparison checked dimensions, slugs, bilingual metadata, member tables,
complete stream fact columns where allowed, and complete compact Arrow output.
All eight StatCan dimension counts matched their pinned official WDS counts.

| Input | Complete rows | Whole stream | Whole table | Complete partition export |
| --- | ---: | --- | --- | --- |
| 98100036 | 336 | converted | converted | unnecessary |
| 98100042 | 93,960 | converted | converted | unnecessary |
| 98100021 | 890,112 | converted | converted | unnecessary |
| 98100277 | 12,261,780 | converted | converted | unnecessary |
| 98100532 | 126,198,000 | memory refusal | memory refusal | converted |
| 98100291 | 32,345,730 | memory refusal | converted | converted |
| 98100592 | 372,507,660 | memory refusal | memory refusal | converted |
| 98100439 | 315,705,600 | memory refusal | memory refusal | converted |
| JODI Primary | 6,985,600 | converted | converted | unnecessary |
| JODI Secondary | 15,717,600 | converted | converted | unnecessary |

The four stream refusals and three whole-table refusals also occurred in
the release baseline, with identical exception types and messages. They are
not successful whole-stream or whole-table conversions. For those four files,
the checker exported each first-axis member separately with a list selector.
This preserved the axis in the output. The ordered partition hashes cover
every row of the full Cartesian product, including null masks.

## Recorded artifact identities

| Artifact | SHA-256 |
| --- | --- |
| Published `pyivt-0.0.9-cp310-abi3-win_amd64.whl` | `dc24c988a0b57358eb9d53db589cf19b637fc7d38f464855eb687b6ec661b8a6` |
| Recorded candidate `_native.pyd` | `c91b4014a6ec83cfc3ff0169a4685aca23f37ccf98051351ef5dc05a8629811a` |
| Reference JSONL | `5bc8e307bf361b0c6373a62d1a8b887e2c0f7c82743908d88350ded95b7522ac` |
| Candidate JSONL | `5bc8e307bf361b0c6373a62d1a8b887e2c0f7c82743908d88350ded95b7522ac` |

The complete results are retained in
[`heldout-compatibility-reference.jsonl`](heldout-compatibility-reference.jsonl)
and [`heldout-compatibility-candidate.jsonl`](heldout-compatibility-candidate.jsonl).
Their identical hashes establish equality of every recorded comparison field.

The compact-table hash sorts schema metadata, records null masks, and fills
numeric payloads under nulls before hashing. Raw IPC bytes can differ because
metadata order and payload bytes under nulls do not determine table values.
A direct `Table.equals` comparison on the 12,261,780-row 98100277 export also
confirmed equality.

Stream hashes cover the dimension IDs, values, symbols, and their Arrow
buffers, together with a schema whose metadata is sorted. Stream hashes
depend on batch boundaries and buffer representation. A differing hash needs
a direct value comparison before it can establish a behavior change.

This is a regression comparison with a prior reader. Independent official
CSV comparisons separately cover the two committed small held-out fixtures.
The comparison does not independently establish the correct value of every
cell in all ten inputs.

## Repeat the comparison

Use Python with the development dependencies installed. Each package argument
names a directory containing a `pyivt` package and its compiled native module.
The checker imports the selected package in a fresh process for each input.

Download the pinned input files with the corpus downloader:

```powershell
python scripts/download_test_inputs.py --manifest tests/corpus/heldout.json --destination .cache/heldout
```

Run both packages and compare their complete results:

```powershell
python scripts/verify_heldout_compatibility.py `
	--manifest tests/corpus/heldout.json `
	--data-dir .cache/heldout `
	--reference-package .cache/heldout-original/package `
	--reference-wheel .cache/heldout-original/pyivt-0.0.9-cp310-abi3-win_amd64.whl `
	--candidate-package python `
	--output-dir .cache/heldout-compatibility
```

The command writes `reference.jsonl`, `candidate.jsonl`, and `summary.json`.
The summary records package-file hashes, the manifest hash, the optional
reference-wheel hash, row counts, and differences. The command exits with
status 1 if the recorded results differ. `--timeout` sets the per-file worker
timeout in seconds and defaults to 900.

On the Windows execution sandbox used for the recorded run, `TEMP` and `TMP`
pointed to a writable directory inside the workspace.
