# 0007 One parley layout per paragraph

- Context: parley applies one alignment per layout and we need per-paragraph alignment, indents and spacing, plus per-line widths for linked frames, columns and wrap.
- Decision: each paragraph is shaped as its own parley layout and broken line by line with `BreakLines`, choosing x/width per line from the current region (frame column) minus wrap obstacles; positions are emitted frame-local.
- Consequence: paragraphs re-shape independently (cheap); caret positions are precomputed per line, so hit testing does not rely on parley's monotonic-y search.
