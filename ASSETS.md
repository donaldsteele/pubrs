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
| `tests/corpus/pub/` | `NOTICE` and the licence texts at the top level of the corpus; provenance notes written for pubrs | Same as the corpus | `tests/corpus/pub/SHA256SUMS` |

## Hyphenation patterns

pubrs links the TeX hyphenation patterns that the `hypher` 0.1.8 crate compiles in (no
separate files in the repository). They are part of the release binary. Source of the
pattern files: `patterns/hyph-*.tex` in <https://github.com/typst/hypher>, which come from the
`hyph-utf8` project (<https://www.hyphenation.org/tex>). The licence of each is the one in its
own file header. The SHA-256 below is that of `hypher-0.1.8.crate`, the archive the patterns
ship in. Only the languages in this table are enabled (`Cargo.toml`, feature list of
`hypher`); the others are listed in `TODO.md`.

| Path | Source | Licence | SHA-256 |
| --- | --- | --- | --- |
| `hypher-0.1.8.crate`, language `en` (US English) | `hyph-en-us.tex`, Copyright (C) 1990, 2004, 2005 Gerard D.C. Kuiken | Notice-preserving permissive grant, **outside the Locked Decision 5 list; the owner must confirm** (English is required by Owner decision 5). Exact terms: "Copying and distribution of this file, with or without modification, are permitted in any medium without royalty provided the copyright notice and this notice are preserved." | `45f2b4f6b5da34d400d4c112eadcc14512cc5d04307a8eee182586d9bebd7ce6` |
| `hypher-0.1.8.crate`, language `de` (German, 1996 spelling) | `hyph-de-1996.tex` | MIT | `45f2b4f6b5da34d400d4c112eadcc14512cc5d04307a8eee182586d9bebd7ce6` |
| `hypher-0.1.8.crate`, language `es` (Spanish) | `hyph-es.tex`, Javier Bezos | MIT/X11 | `45f2b4f6b5da34d400d4c112eadcc14512cc5d04307a8eee182586d9bebd7ce6` |
| `hypher-0.1.8.crate`, language `fr` (French) | `hyph-fr.tex` | MIT | `45f2b4f6b5da34d400d4c112eadcc14512cc5d04307a8eee182586d9bebd7ce6` |
| `hypher-0.1.8.crate`, language `pl` (Polish) | `hyph-pl.tex`, offered under any of several licences | MIT (chosen) | `45f2b4f6b5da34d400d4c112eadcc14512cc5d04307a8eee182586d9bebd7ce6` |
| `hypher-0.1.8.crate`, language `pt` (Portuguese) | `hyph-pt.tex` | BSD-3-Clause | `45f2b4f6b5da34d400d4c112eadcc14512cc5d04307a8eee182586d9bebd7ce6` |
