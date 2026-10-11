# M27: Clipboard, input methods and right-to-left carets

What works:

- **Frames between publications.** Copy puts the selection on the OS clipboard as HTML that carries a pubrs payload
  (frames, stories, assets with their bytes, the styles in use, the colours of the swatches in use), a PNG of the
  selection and the frames' text as the plain-text fallback. Paste, in the same or another open publication, adds
  new frames with new ids as one undo step: linked frames stay linked, groups stay groups, a picture the target
  already holds (same bytes) is reused, styles are matched by name (missing ones added), swatches the target lacks
  become plain colours. The app also keeps the payload itself, so big selections still paste inside this run.
- **MCP.** `frame_copy_to_doc {from_doc_id, frame_ids, to_doc_id, page_index, offset?}` does the same between two
  open documents (`deny_unknown_fields`; example in `docs/mcp-examples.json`, MCP.md regenerated).
- **Pictures and text from other programs.** A clipboard picture becomes a picture frame (PNG, 96 dpi, centred,
  scaled down above 8,000 px a side with a status warning). HTML pasted while editing is reduced to bold, italic,
  underline, size and colour; plain text goes in as is, or into a new text frame when nothing is being edited.
- **Input methods.** `egui::Event::Ime`: preedit text is drawn at the caret with an underline (thicker for the part
  being converted) and is not in the document (no command, no layout, no undo step); commit goes through the
  coalesced typing path; delete-surrounding is handled; the window gets the caret rectangle every frame so the
  candidate window opens beside it.
- **Right-to-left.** `LineBox` knows each character's box on lines that can reorder and a second caret place where
  text changes direction. Arrows, Home/End, clicking, dragging and selection quads follow position on the line.
  Lines in reading order take the old code and carry no extra data.
- **Layout tests** (`crates/pubrs-layout/tests/flow.rs`): Hebrew and mixed lines, caret x order, hit-test round
  trip, arrow walks in visual order, selection pieces.

See it:

```
cargo run -p pubrs-app            # select a frame, Ctrl+C, File > New, Ctrl+V
cargo test -p pubrs-api --test clip -p pubrs-layout --test flow
xvfb-run -a -s "-screen 0 1600x1000x24" cargo run -p pubrs-app -- --self-test target/gui-selftest
```

The self-test (steps 185 to 194) copies a text frame and a picture, pastes them into a second publication (assets
checked byte for byte, a second paste reuses the picture, two undos empty the page), pastes a 9000 x 40 clipboard
picture (scaled to 8000 x 36 with a warning) and a 64 x 32 one, injects an IME preedit (no undo step, story
unchanged) and a commit of 日本 (one undo step, undone in one), and checks Right, Home and Left in a Hebrew word.

  frame_copy_to_doc {"from_doc_id": "...", "frame_ids": ["..."], "to_doc_id": "...", "page_index": 0, "offset": {"x": 0, "y": 36}}

Design notes: ADR 0025. Not done (TODO.md): native multi-format clipboard offers per OS, RTF, Ctrl+V for an
image-only clipboard, preedit in the document font, tab stops from the right edge in right-to-left paragraphs.
