# M17: Character formatting and missing glyphs

What works:

- **Super and subscript.** Character field `baseline`: `"superscript"`, `"subscript"` or `{"shift": points}` (positive
  raises). Super and subscript take the font's own OS/2 size and offset (Liberation Sans: 65% size, raised 48% of the
  size) and fall back to 58% and 33% when the font has none. A raised run makes its line taller, as in Publisher.
- **Caps.** `caps`: `"small"`, `"all"` or `"none"`. Small caps use the font's `smcp` feature when it has one, else
  lowercase letters are drawn as capitals at 70% size. All caps leaves 'ß' alone. The text itself is never changed:
  offsets, find, spelling and copy see what was typed.
- **Strikethrough, ligatures, kerning.** `strikethrough` draws a rule through the text (also in the PDF); `ligatures`
  and `kerning` (default on) switch the font features off.
- **Missing glyphs.** `validate` names the characters no bundled or installed font covers:
  `no available font covers: ✓ (U+2713), …` (`missing_glyphs` warning, at most 20 named). Missing families stay
  `missing_font`.
- **MCP.** `text_apply_char_style` and the style tools take `baseline`, `caps`, `strikethrough`, `ligatures` and
  `kerning`. `text_apply_list` now rejects `restart_at` over 1,000,000 up front.
- **GUI.** Text ▸ Character… (Cmd/Ctrl+Alt+C): font, size, bold, italic, underline, strikethrough, baseline (normal,
  superscript, subscript, shift), capitals and the ligature and kerning checkboxes; Enter applies, Esc cancels, one undo
  step. The text panel has x² and x₂ buttons next to B, I and U; Text ▸ Superscript and Subscript are Cmd/Ctrl+Shift+=
  and Cmd/Ctrl+= (Publisher's keys; Zoom In moved to Cmd/Ctrl+Alt+=).
- **Schema 8.** `v7_to_v8` stamps the version; `tests/fixtures/schema_v8.pubrs` holds every new field.

Golden: `character_formats` (sub and superscript, a 3 pt shift, small and all caps, strikethrough, kerning off). Run
`cargo test -p pubrs-layout --test char_format` and `cargo test -p pubrs-io --test pdf_export`.

Try it:

```
claude mcp add pubrs -- pubrs-mcp
# text_apply_char_style {"frame_id": "...", "start": 1, "end": 2, "baseline": "subscript"}
# text_apply_char_style {"frame_id": "...", "start": 4, "end": 20, "caps": "small", "strikethrough": true}
# text_apply_char_style {"frame_id": "...", "baseline": {"shift": 3}, "kerning": false}
```
