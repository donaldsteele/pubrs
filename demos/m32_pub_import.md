# M32: `.pub` import, part 2 (pictures, shapes, colours, tables, masters, entry points)

What works:

- Pictures: the Escher BLIP store (`EscherStm` drawing group, pictures in `EscherDelayStm`) is read; PNG and JPEG are placed
  as they are, GIF (two of SampleBrochure's "PNG" blips are GIFs) is converted to PNG. Windows metafiles (WMF/EMF) are
  not drawn: they leave an empty picture frame and a `picture-wmf` entry in the report. (No corpus metafile holds a
  bitmap, so there is no extractor; ADR 0026.)
- Shapes: rectangle, rounded rectangle, ellipse and line with fill and outline colour and width; other shape types are
  reported as `shape` with the Escher type. Fill and outline also apply to text frames.
- Colour scheme: the file's palette becomes the seven "Scheme:" swatches plus "Scheme: Background"; colours that refer to
  the palette use the swatches, direct colours stay colours.
- Tables: a plain grid with one paragraph of text per cell (the corpus' Sample files) becomes a table frame; anything else
  keeps its text as a text frame and is reported as `table`.
- Masters: each page's master reference (page block 0x0d) gives a master page, assigned to the pages that use it.
- Report: pages, frames, pictures, shapes, tables, masters, swatches, fonts used, and everything dropped or substituted
  by kind (`font-fallback` for families that are not installed). `summary()` is the one-line form.
- Entry points: MCP `doc_import`, CLI `pubrs import`, File > Import Publisher File…, `.pub` files dropped on the window or
  given on the command line, `.pub` offered under "Open with" by the Linux, macOS and Windows packages (never the default).

See it:

```
cargo run -p pubrs-cli -- import tests/corpus/pub/poi/SampleNewsletter.pub -o target/newsletter.pubrs
cargo run -p pubrs-cli -- export target/newsletter.pubrs -o target/newsletter.pdf
cargo run -p pubrs-cli -- import tests/corpus/pub/poi/SampleBrochure.pub -o target/brochure.pubrs --json
cargo test -p pubrs-io --test pubfile -- --nocapture
```

`pubrs import` on SampleNewsletter prints:

```
Imported 4 pages, 82 frames, 1 picture, 9 shapes, 1 master, 8 swatches; dropped: 1 unknown-block, 55 chunk, 6 escher, 10 shape, 9 picture-wmf; 7 fonts substituted
```

Placed pictures against the oracle's image count (`pdfimages` of LibreOffice's PDF; SampleBrochure's 5 are 3 pictures and
2 soft masks of its transparent GIFs):

```
poi/51318.pub             oracle images 1 placed 1
poi/SampleBrochure.pub    oracle images 5 placed 3 (5 WMF drawings left as empty frames)
poi/SampleNewsletter.pub  oracle images 1 placed 1 (9 WMF drawings left as empty frames)
```

Word recall is unchanged or better (Sample*.pub 1.000 now that the table text counts; 51318 0.917; SampleBrochure 0.998;
SampleNewsletter 0.955 per frame, 0.901 distinct). Golden pages (our own output, 72 dpi, test fonts):
`tests/golden/render/pub_sample_newsletter_p1.png`, `pub_sample_brochure_p1.png`, `pub_sample2000_p1.png`.

In the app, drop `tests/corpus/pub/poi/SampleNewsletter.pub` on the window: the import report dialog lists what was
dropped. The GUI self-test does this in steps 195 to 197.
