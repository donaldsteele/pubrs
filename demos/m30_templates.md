# M30: Everyday templates

What works:

- 13 built-in templates (`crates/pubrs-io/src/templates.rs`): flyer, trifold brochure and business card as before; a
  4-page newsletter (boxed masthead, flowing lead story, pull quote, events and opening-hours tables, bulleted and
  numbered lists, a mailing panel); greeting card (quarter-fold, inside spread rotated 180 degrees), postcard,
  certificate (BorderArt), restaurant menu (right tab with dot leaders), letterhead, #10 envelope, annual calendar,
  address labels (Avery 5160) and business cards (Avery 5371). Each new one has its own colour scheme and named styles
  (Body justifies with limits and keeps widows and orphans in check).
- The calendar takes a year: `template_apply {name: "calendar", year: 2026}` (1600 to 9999; default 2027). Twelve pages,
  weeks start on Sunday, six week rows per month with the neighbouring months' days in grey. The GUI opens it on the
  current year.
- A table helper (`T::table`) and date helpers (`is_leap`, `days_in_month`, `weekday`), tested for every day from 1900
  to 2100.
- Every template renders to a golden, validates with zero warnings once its placeholders are filled (test helper fills
  text and pictures with alt text), and obeys the checker's 6 pt, 18 pt and 3:1 rules (tested directly).
- MCP: prompts `make_newsletter` and `make_greeting_card`; an example step per template, plus the calendar year and
  two error cases. The New From Template dialog builds its list once per opening.

See it:

```
cargo test -p pubrs-io --lib templates
cargo test -p pubrs-io --test render_golden template_goldens
cargo test -p pubrs-api --test templates
cargo run -p pubrs-app                          # Ctrl+N shows the gallery
```

Goldens: `tests/golden/render/template_<name>_p1.png` for all 13 (the calendar shows January 2027).
ADR: `docs/adr/0028-everyday-templates.md`.
