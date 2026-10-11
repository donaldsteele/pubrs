# M16: Tab stops, leaders, bullets and numbering

What works:

- **Tab stops.** Paragraph field `tab_stops`: `[{pos, align, leader}]` with `pos` in points from the left edge of the text
  area, `align` left (default), center, right or decimal, and `leader` none (default), dot, dash or line. A tab goes to
  the first stop past the text before it; past the last stop, or with no stops, to the next multiple of 36 pt (Publisher's
  default grid). Decimal stops put the first `.` (or `,` in languages that write a decimal comma) on the stop. Leaders
  fill the gap with repeated glyphs on a grid anchored at the column edge, so the dots of consecutive lines line up.
  Carets and clicks work across a tab gap. Tab segments are not stretched: a justified paragraph with a tab is set flush
  left. A segment pushed past the right edge overhangs (lines are broken before tabs are placed; ADR 0019).
- **Lists.** Paragraph field `list`: `{kind: bullet|number, bullet, format: "1."|"a."|"A."|"i."|"I.", level, indent,
  start}`. The label hangs left of the text: the text is at `left_indent`, the label `indent` to its left. Numbers
  continue across paragraphs and across linked frames, a deeper level restarts after a shallower item, paragraphs that
  are not list items do not break the count, and `start` restarts it. Labels are generated text: not in the story, so
  offsets, find and `text_get` ranges are unchanged (`text_get` shows `list_label`).
- **Tab key.** While editing, Tab inserts a tab. At the very start of a list item it makes the item one level deeper;
  Shift+Tab (anywhere in a list item) one level shallower. One undo step each; Cmd/Ctrl and Alt combinations are not list
  gestures.
- **MCP.** `text_set_tabs {stops}`, `text_apply_list {kind, format, bullet, level, indent, restart_at, left_indent}`,
  `text_clear_list {keep_indent}`; `text_apply_para_style` and the style tools take `tab_stops` and `list`.
- **GUI.** Text ▸ Tabs…: a ruler strip (click to add a stop, drag to move it, drag it off the strip to remove it, arrow
  keys and Delete when the strip is focused), plus position, alignment and leader for the selected stop, Add stop, Remove
  and Clear all. Text ▸ Bullets and Numbering…: None, Bullets or Numbers, bullet character, number format, level, indent
  and restart number, with a preview. Controls that do not apply say why in their tooltip.
- **Schema 7.** `v6_to_v7` stamps the version; `tests/fixtures/schema_v7.pubrs` holds a right tab with a dot leader and a
  nested numbered list.

Goldens: `tabs_menu` (right-aligned prices on dot leaders) and `nested_list` (1. a. b. 2. 3. and bullets). Run
`cargo test -p pubrs-layout --test tabs_lists`.

Try it:

```
claude mcp add pubrs -- pubrs-mcp
# text_set_tabs {"frame_id": "...", "stops": [{"pos": 240, "align": "right", "leader": "dot"}]}
# text_apply_list {"frame_id": "...", "kind": "number", "start": 1, "end": 40}
# text_apply_list {"frame_id": "...", "start": 20, "end": 30, "kind": "number", "format": "a.", "level": 1}
```

In the app: type `Soup`, press Tab, type `4.50`, choose Text ▸ Tabs…, click the ruler, pick Right and Dots, press Enter.
Select some paragraphs, choose Text ▸ Bullets and Numbering…, pick Numbers; click at the start of an item and press Tab.
