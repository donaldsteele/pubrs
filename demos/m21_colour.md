# M21: swatches, colour schemes, CMYK, spot colours and gradients

What it does: a publication can have named swatches (RGB, CMYK, or a spot ink with a CMYK alternative), twelve built-in colour schemes, tints and gradient fills. Everything that uses a swatch recolours when the swatch or the scheme changes.

## In the app
1. Open **Design > Swatches** (or select nothing and look at the bottom of the right-hand panel). Click **+ CMYK**, name it, set the four percentages and Save. CMYK is shown on screen by a plain formula, not colour-managed.
2. Select a shape. In **Fill**, open the picker: the document's swatches come first, then a custom colour, then Linear and Radial gradient. Pick the swatch; use **Tint** for 25/50/100 percent.
3. **Design > Colour Scheme...** shows the twelve schemes. **Apply** recolours everything that uses the `Scheme:` swatches, including the colours in the built-in templates. Ctrl/Cmd+Z undoes it in one step.
4. A swatch that is in use cannot be deleted; its Delete button says where it is used.

## From a script or an AI assistant
```json
{"tool": "swatch_create", "args": {"name": "Brand Cyan", "value": {"cmyk": {"c": 100, "m": 30, "y": 0, "k": 10}}}}
{"tool": "frame_set_style", "args": {"frame_id": "...", "fill": {"swatch": "<swatch_id>", "tint": 25}}}
{"tool": "frame_set_style", "args": {"frame_id": "...", "fill": {"gradient": "linear", "angle": 90, "stops": [{"offset": 0, "color": "#1E3A5F"}, {"offset": 1, "color": "#FFFFFF"}]}}}
{"tool": "scheme_apply", "args": {"scheme": "coral_reef"}}
```
`swatch_list`, `swatch_update`, `swatch_delete` (confirm), `scheme_list` and the resource `pubrs://doc/{id}/swatches` complete the set. Hex colours keep working everywhere.

## Checking it
- `tests/golden/render/colour.png`: gradients, tints, a CMYK fill with a spot outline, scheme slots and swatch-coloured text.
- `cargo test -p pubrs-render --test tiles`: gradient pages tile byte-identically.
- `cargo test -p pubrs-io --test pdf_export`: gradients become PDF shadings.
- The GUI self-test creates a CMYK swatch in the editor, fills a shape from the picker, applies two schemes from the dialog and checks the rendered pixel each time.

Known limits: the PDF writes swatches as RGB until M22; the GPU canvas draws gradients with vello but that path is not exercised by CI (no GPU adapter).
