# M26: Rulers, guides from rulers, find and replace

What works:

- **Rulers** along the top and left of the canvas (View > Rulers, on by default, remembered). They use the chosen
  unit, put zero at the page's top-left corner, space ticks and labels by zoom in screen points and snap lines to
  device pixels (so they are sharp on HiDPI), and show the selection's bounds as a band.
- **Guides from rulers**: drag from the top ruler for a horizontal guide, the left ruler for a vertical one, with a
  live preview. Drag a guide to move it (`MoveGuide`); drag it off the page or onto a ruler to delete it
  (`RemoveGuide`, shown in red while armed). Esc cancels. Each is one undo step. Master-page guides are drawn but
  not draggable on pages.
- **Find and replace** in core (`text::find_matches`, `Story::find`): char-offset ranges, case-insensitive
  (Unicode lowercase, default) or case-sensitive, whole-word (Unicode alphanumerics and `_`), optional range
  limit. Matches never cross a paragraph break unless the query has a `\n`.
- **MCP tools**: `text_find` (whole document or one story; returns story, the frame showing the match, offsets and
  a context snippet), `text_replace_all` (one `Batch`, so one undo step; zero matches is a no-op) and
  `guide_move`.
- **GUI**: Ctrl/Cmd+F opens a find bar (query, Match case, Whole word, count, Previous/Next, Enter and Shift+Enter,
  F3 and Shift+F3). Next scrolls to the match's page and frame and selects it as a text selection. Ctrl/Cmd+H
  opens Find and Replace (Replace, Replace All, Selection only). Esc closes. Unavailable buttons say why.

See it:

```
cargo run -p pubrs-app
xvfb-run -a -s "-screen 0 1600x1000x24" cargo run -p pubrs-app -- --self-test target/gui-selftest
```

The self-test (steps 30 to 49) drags guides in and out of the rulers with injected mouse input, checks each is one
undo step, then finds a word with Ctrl+F and Enter, asserts the selection equals the match, and runs Replace All
followed by a single undo. Screenshots are in `target/gui-selftest/`.

Try the tools: the call shapes are in `docs/mcp-examples.json` (`text_find`, `text_replace_all`, `guide_move`) and in MCP.md.

Known limits: WordArt text (M25) is not searched yet; guides on a master can't be dragged from a page.
