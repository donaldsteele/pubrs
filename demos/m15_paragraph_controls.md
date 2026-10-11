# M15: Paragraph controls, drop caps and breaks

What works:

- **Keeps.** Paragraph attributes `keep_with_next` and `keep_together` (default off), and `widow_orphan`: the fewest
  lines left alone at the top or bottom of a column or frame (`true` = 2 lines, `0`, `1` or `false` = off). A broken
  rule moves the paragraph (or its run of keep-with-next paragraphs) to the top of the next column or frame, or, for
  a widow, moves the column break up so enough lines go together. Nothing moves more than one column or frame, and a
  rule that cannot be met leaves the usual layout, and keeps never push text the usual layout shows out of the
  chain (that move is undone, or the story is laid out without keeps). Unset, every document lays out exactly as
  before (LD2), so no golden changed.
- **Drop caps.** `drop_cap {lines, chars}` (defaults 3 and 1): the first letters are drawn as one large glyph run
  whose cap height reaches from the first line's cap height to the baseline of line `lines`; those lines are
  indented past it. The letters stay in the story: the first line's carets run across the cap, and clicking it puts
  the caret there. A cap that would hang below a column starts the paragraph in the next one; a paragraph shorter
  than the cap keeps the cap's lines.
- **Breaks.** Column (U+000E), frame (U+001D) and page (U+000C) breaks are single characters in the text, so offsets
  stay in characters. Column: next column, or next frame after the last. Frame: next frame of the chain. Page: next
  frame of the chain on a later page. pubrs does not add pages: with nowhere to go, the rest overflows and
  `text_overflow_status` reports it. Breaks draw nothing; the caret sits before and after them. Code points and the
  layout design are in ADR 0015.
- **Carets.** Caret positions are now frame-local like glyphs; before, the editing caret sat left of inset or
  indented text by that amount.
- **MCP.** `text_insert_break {kind: column|frame|page, offset}`; `text_apply_para_style` takes `keep_with_next`,
  `keep_together`, `widow_orphan` and `drop_cap`; paragraph styles take them through `style_create` /
  `style_update`.
- **GUI.** Text ▸ Paragraph… (Ctrl/Cmd+Alt+P) has two tabs: Indents and Spacing (spacing, indents, hyphenation,
  justification limits) and Keeps (Keep with next, Keep lines together, Widow and orphan control with its line
  count). Text ▸ Drop Cap… sets lines and letters with a preview. Insert ▸ Break ▸ Column, Frame or Page Break inserts
  at the caret (Ctrl/Cmd+Enter for a page break, Ctrl/Cmd+Shift+Enter for a column break); without a caret the items
  are greyed out and their tooltip says why. While a dialog is open over text being edited, Enter belongs to the
  dialog.
- **Schema 6.** `v5_to_v6` stamps the version (M21 and M22 took 4 and 5); `tests/fixtures/schema_v6.pubrs` holds every new field and one break
  of each kind; FILE_FORMAT.md lists the break code points.

Golden: `paragraph_controls` (a kept heading, a three-line drop cap and a column break). Run
`cargo test -p pubrs-layout --test flow`.

Try it:

```
claude mcp add pubrs -- pubrs-mcp
# text_apply_para_style {"frame_id": "...", "start": 0, "end": 0, "drop_cap": {"lines": 3}}
# text_apply_para_style {"frame_id": "...", "keep_together": true, "widow_orphan": true}
# text_insert_break {"frame_id": "...", "kind": "column", "offset": 120}
```

In the app: click in a paragraph, choose Text ▸ Drop Cap…, press Enter; Ctrl/Cmd+Enter in the middle of a story
sends the rest to the next frame on a later page.
