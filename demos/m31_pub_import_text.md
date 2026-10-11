# M31: `.pub` import, part 1 (container, pages, text frames, text)

What works:

- `pubrs_io::pubfile::import(bytes)` reads a Microsoft Publisher file and returns a pubrs `Document` built through
  `DocBuilder` commands plus an `ImportReport { pages, frames, dropped }`. Nothing it does not understand fails the
  import; it is listed in `dropped` by kind and count.
- Three file generations: Publisher 98 (text inside `Contents`), 98/2000 with Quill text, and 2002 and later with
  Escher geometry (the corpus goes up to 2010). Any other version byte is an error that names the byte.
- Pages (master and placeholder pages left out), page size, text frames with their geometry, insets, columns and vertical
  alignment, linked chains in flow order, story text, character formatting (font, size, bold, italic, underline, colour)
  and paragraph formatting (alignment, line spacing, space before and after, indents).
- A panic anywhere in the parser comes back as `IoError::Format`; every read is bounds-checked and every count is
  clamped to the stream it points into.
- Pictures, shapes, tables, masters and colour schemes are not read yet (T24); they appear as dropped chunks.

See it:

```
cargo test -p pubrs-io --test pubfile -- --nocapture
```

Per-file results (debug build; page size is the first page; recall is the share of the LibreOffice text's words, lower
case, punctuation stripped, compared as a multiset, found in the imported text; "per-story" counts each story once):

```
file                                       pages   first page (pt)   oracle per-story      poi    ms
libreoffice/fdo59355-1.pub                     2   595.28 x 841.89    1.000    1.000        -     0
poi/51318.pub                                  1   612.00 x 792.00    0.917    0.917        -     0
poi/60685.pub                                  1   612.00 x 792.00    1.000    1.000        -     1
poi/LinkAt0And10.pub                           1   595.28 x 841.89    1.000    1.000        -     0
poi/LinkAt10.pub                               1   595.28 x 841.89    1.000    1.000        -     0
poi/LinkAt10And20And30.pub                     1   595.28 x 841.89    1.000    1.000        -     0
poi/LinkAt10And20And30And40.pub                1   595.28 x 841.89    1.000    1.000        -     0
poi/LinkAt10Longer.pub                         1   595.28 x 841.89    1.000    1.000        -     0
poi/LinkAt20.pub                               1   595.28 x 841.89    1.000    1.000        -     0
poi/Sample.pub                                 2   595.28 x 841.89    1.000    1.000    1.000     0
poi/Sample2.pub                                2   595.28 x 841.89    1.000    1.000    1.000     0
poi/Sample2000.pub                             3   595.28 x 841.89    1.000    1.000        -     0
poi/Sample2_2010.pub                           2   595.28 x 841.89    1.000    1.000        -     0
poi/Sample3.pub                                2   595.28 x 841.89    1.000    1.000    1.000     0
poi/Sample3_2010.pub                           2   595.28 x 841.89    1.000    1.000        -     0
poi/Sample4.pub                                2   595.28 x 841.89    1.000    1.000    0.990     0
poi/Sample4_2010.pub                           2   595.28 x 841.89    1.000    1.000        -     0
poi/Sample98.pub                               3   595.28 x 841.89    1.000    1.000        -     0
poi/SampleBrochure.pub                         2   841.89 x 595.28    0.998    0.998        -     1
poi/SampleNewsletter.pub                       4   595.28 x 841.89    0.955    0.451        -     4
poi/Sample_2010.pub                            2   595.28 x 841.89    1.000    1.000        -     0
poi/Simple.pub                                 1   595.28 x 841.89    1.000    1.000        -     0
```

`SampleNewsletter.pub`: 4 pages, 3,306 oracle words, 63 frames, nine three-frame chains. `SampleBrochure.pub`:
841.89 x 595.28 pt, A4 landscape, 2 pages.

The oracle (libmspub) writes a story's whole text into every frame that shows it, so its text for a three-frame chain has
three copies; pubrs keeps one copy per story. The 0.90 check counts each frame's story, as the reference does; the
per-story column shows the plain figure (0.451 for the newsletter). A third check, per-file distinct-word recall (all stories pooled) (each story once, 0.901 for the newsletter, no file below 0.917 apart from it), is also enforced at 0.90 and does not depend on duplication. The readings agree wherever no story spans
several frames.

The six files LibreOffice cannot convert (three `EDB-*` exploit samples and three clusterfuzz cases) are all rejected in
the container (bad magic or malformed FAT) with an `Err` in under 1 ms each.
