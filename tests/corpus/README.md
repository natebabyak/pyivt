# Corpus policy

`zip.json` pins a real StatCan ZIP and its independent official CSV, retained
under `tests/fixtures/zip/`. Its IVT already belongs to the training corpus;
it adds archive-input coverage without changing the held-out sample.
`tests/test_zip_inputs.py` compares all 154 official cell values and checks
identical metadata, members, and Arrow output through path and bytes routes.

`training.json` pins the 20 2021 Census inputs available during development.
CI downloads all of them before Rust or Python tests, with
`PYIVT_REQUIRE_DATA=1`. Missing inputs fail instead of making data tests pass
without assertions. Optional installed Beyond demo tests are visibly ignored;
explicitly running them without their data fails.

`heldout.json` pins ten inputs (1.17 GB total), acquired and hashed without
opening their IVT contents in pyivt. Parser changes were completed at 9cbaee9
before the first held-out conversion. Do not tune parser rules against these
hashes. Promote a failing case into development data before fixing it, and
replace it with a fresh held-out input. Record any such change explicitly.

Eight StatCan files were selected at evenly spaced compressed-size quantiles
from application/zip entries in the Wayback CDX canonical
`www150.statcan.gc.ca/n1/tbl/b2020/*` index, excluding development product IDs
98100001–98100020. The manifest retains snapshot timestamps and archive digests;
the acquired file bytes are bound by SHA-256. Official WDS `getCubeMetadata`
supplies independent dimension counts and names: the sample spans 3, 4, 7 and
9 axes and IVT sizes from 15,728 to 360,096,177 bytes. The acquisition used
current official downloads, not an assertion that a historical ZIP digest
matches the current ZIP. Source: [StatCan WDS guide](https://www.statcan.gc.ca/en/developers/wds/user-guide).

Two [JODI oil tables](https://www.jodidata.org/oil/database/data-downloads.aspx)
add another producer and a 2009–2026 time-series period. They are public downloads,
not redistributed here. JODI updates its current archive; the downloader fails
closed if the pinned bytes disappear or change. The nightly cache retains the
exact acquired inputs. Replacing a pin requires an explicit corpus review.
Historical inputs are tracked separately in `legacy.json`; the held-out set does
not establish broad historical-format coverage.

The absent `/workspace/ivtcorpus/` manifest could overlap this selection; without
that manifest, disjointness from the user's 66-file sample cannot be established.
Disjointness from the 20 development hashes is enforced by a test.

The nightly workflow builds a manylinux wheel and runs each input in a fresh
process inside an 8 GiB Docker cgroup. Both streaming and materialized exports
are measured. It validates file identity, dimension counts against WDS, and row
counts against the complete dimension product. Catchable `MemoryError` and
`ValueError` refusals are recorded distinctly from conversions. Crashes, cap
violations, timeouts and row/dimension mismatches fail the workflow. A run with
no completed stream also fails; refusals are not reported as successful conversions.
JSONL results are uploaded even when the gate fails.

```shell
python scripts/download_test_inputs.py --manifest tests/corpus/heldout.json --destination .cache/heldout
python -m scripts.probe_corpus --manifest tests/corpus/heldout.json --data-dir .cache/heldout
```

The local RSS watchdog is sampled, not a hard OS memory limit; Docker supplies
the hard limit in nightly CI. Results must say which limit was actually used.
The corpus is an availability/robustness gate, not an independent value oracle.
Official CSV comparisons cover a documented sample separately.

## Historical development inputs

`legacy.json` pins eleven public Statistics Canada tables acquired from Borealis
on November 10, 2026. Every input starts with `02 00 20 00`. The acquisition
checked each complete file against the source inventory's byte size and MD5,
then recorded SHA-256. These are development inputs, separate from the held-out
corpus. The set includes ordinary survey dimensions without a geographic
identifier, the `tb611996` date-only axis with deleted member slots, mixed-width
`00060117`, Census of Agriculture, and four Business Patterns layouts.

```shell
python scripts/download_test_inputs.py --manifest tests/corpus/legacy.json
```

The independent canivt references and their limitations are documented in
[`tests/fixtures/legacy/README.md`](../fixtures/legacy/README.md).

`legacy-version-search.json` retains the first 32 bytes of 45 distinct public
IVTs inspected across Health Statistics 1999, Census of Agriculture 1996, Small
Area Business and Labour 1996, and Business Patterns 1988–2014. Forty-one have
version 2, four have version 4, and none has version 3. An initial 299-file Range
request attempt had 273 connection failures; those failures establish no format
facts and are excluded from the inspected count. A subsequent selection of five
evenly spaced inventory members per collection succeeded without Range. The
installed four Beyond 20/20 demonstration IVTs also have version 4. Web searches
for the literal v3 signature and historical ODESI/DLI/1991/1996 products found
documentation and download sources, but no identified v3 sample. This is a
bounded negative search, not evidence that version 3 never existed.

## Version 4 regression fingerprints

`v4-baseline.json` records every Arrow column and row for the twenty training
inputs from the published pyivt 0.0.9 Windows wheel, using PyArrow 26.0.0.
`v4-baseline-meta.json` pins the wheel identity and serialization settings.
The comparison removes filesystem-dependent schema metadata. It proves that
output stayed unchanged; the independent CSV comparisons establish value
correctness separately. Use the same PyArrow version when comparing IPC hashes.

```shell
python -m scripts.verify_v4_baseline --baseline tests/corpus/v4-baseline.json
```

To capture a new reference on an explicitly selected reference build, pass
`--capture --manifest tests/corpus/training.json --baseline OUTPUT.json`.
