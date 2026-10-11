# M14: Hyphenation and bounded justification

What works:

- **Hyphenation.** Paragraph attributes `hyphenate` (default off) and `language` (BCP 47, default `en-US`). Words
  are broken at TeX-pattern points (`hypher`; en, de, es, fr, pl, pt, licences in `ASSETS.md`) only when a line
  would otherwise be loose: justified lines that word spaces cannot absorb, ragged lines with a gap over 12% of the
  width. The line ends with the font's hyphen glyph, its width reserved before the break is chosen, so right and
  centre alignment and justification include it. Story offsets and carets stay in characters; nothing is inserted
  into the story. Design and parley limits are in ADR 0014.
- **Soft hyphens.** A typed U+00AD is a break opportunity (shown as a hyphen only at a line end) whether or not
  `hyphenate` is on.
- **Justify limits.** `justify_limits {max_word_space: 1.33, letter_spacing: 0.05}`: word spaces stretch up to 133%,
  then letter spacing up to 5% of the font size; a line needing more is left short. Unset means unbounded, exactly
  as v0.1 (LD2), so existing documents and goldens do not change. The last line is never stretched.
- **Validate.** `hyphenation_unavailable` for a paragraph that asks for a language without patterns (no hyphens).
- **MCP.** `text_apply_para_style` takes `hyphenate`, `language` and `justify_limits`; paragraph styles take them
  through `style_create` / `style_update` (`paragraph`).
- **GUI.** Text ▸ Paragraph… (Ctrl/Cmd+Alt+P): spacing and indents, Hyphenate, Language, and "Limit word and letter
  spacing" with its two percentages. Tab moves between controls, Enter applies, Escape cancels; one undo step.
- **Schema 3.** `v2_to_v3` stamps the version; fixtures `tests/fixtures/schema_v1..v3.pubrs` all open and round-trip.

Goldens: `newsletter_justified` and `newsletter_justified_hyphenated` (two justified columns, with and without
hyphenation plus limits). Run `cargo test -p pubrs-layout --test hyphenation`.

Try it:

```
claude mcp add pubrs -- pubrs-mcp
# text_apply_para_style {"frame_id": "...", "align": "justify", "hyphenate": true, "justify_limits": {}}
```
