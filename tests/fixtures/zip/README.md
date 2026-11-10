# ZIP development fixture

`98100001.zip` is the original Statistics Canada download acquired on
November 10, 2026. `98100001.csv` is the data member of the independent official
CSV download acquired the same day. URLs and SHA-256 identities for the ZIP,
extracted IVT and CSV are pinned in `tests/corpus/zip.json`.

The IVT is already part of the training corpus. The test checks ZIP integrity,
all 154 coordinate/value pairs against the official CSV, and equality with the
extracted reader's metadata, member table and Arrow output. Neither fixture
is generated from pyivt output.
