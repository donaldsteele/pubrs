# Third-party assets in the repository

Test data and other files that pubrs did not write. The sources under `tests/corpus/pub/` share one
`NOTICE` (exact commits; the licence texts sit beside it) and one `SHA256SUMS` that lists every file
there; run `sha256sum -c SHA256SUMS` from that folder to check them. None of these files is part of a
release build.

| Path | Source | Licence | SHA-256 |
| --- | --- | --- | --- |
| `tests/corpus/pub/poi/` | Apache POI `test-data/publisher` at `ae62bb51` (24 Publisher files and POI's `Sample*.txt` extractions) | Apache-2.0 | `tests/corpus/pub/SHA256SUMS` |
| `tests/corpus/pub/libreoffice/` | LibreOffice core `writerperfect/qa/unit/data/draw/libmspub/pass` at `63a2e191` (4 Publisher files) | MPL-2.0 | `tests/corpus/pub/SHA256SUMS` |
| `tests/corpus/pub/oracle/text/` | Text extracted from the PDFs LibreOffice 25.2.3.2 (libmspub 0.1.4) made from the files above; generated for pubrs | Same as the Publisher file each was made from | `tests/corpus/pub/SHA256SUMS` |
| `tests/corpus/pub/expected.json` | Page counts, sizes, image and word counts and file hashes for the corpus; generated for pubrs | Same as the corpus | `tests/corpus/pub/SHA256SUMS` |
