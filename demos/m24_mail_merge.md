# M24: Mail merge

What works:

- **Data.** `merge_set_source` reads a CSV (UTF-8 with or without a BOM, comma, semicolon or tab, quoted fields with
  commas, quotes and line breaks) or an XLSX workbook (first sheet or a named one; numbers and dates as displayed,
  dates as ISO) and copies it into the document: it reopens without the file, and `refresh: true` reads the file again
  as one undo step. Limits: 100,000 records, 1 MB per cell.
- **Fields.** `merge_insert_field` puts a field (one character) in a text box. It shows as «First» until
  `merge_preview` turns the preview on; then it shows the chosen record's value. Carets, selection and find treat a
  field as one character. An empty value takes one adjacent space with it, so "Dear Ann  Lee" does not happen.
- **Output.** `merge_export` writes one PDF with the page set once per record, or a new publication with the same pages
  and plain text instead of fields. On a label sheet (`doc_set_sheet`, fill `sequence`) every record is one label:
  30 records on Avery 5160 fill one sheet. `pubrs merge doc.pubrs --data list.csv -o out.pdf` does the same from the
  command line (`--as publication`, `--sheet NAME`).
- **Checks.** `validate` warns `merge_field_missing` when a field names a column the data lacks.
- **MCP.** `merge_set_source`, `merge_fields_list`, `merge_insert_field`, `merge_preview`, `merge_export`,
  `merge_clear`; prompts `make_mail_merge` and `make_labels`.
- **GUI.** Mailings ▸ Mail Merge… (choose the list, insert fields at the cursor, step through records with ◀ ▶ or the
  arrow keys, finish to PDF, a new publication or the printer), Mailings ▸ Refresh Data, Insert ▸ Merge Field…. Fields
  have a faint tint on the page.
- **Schema 12.** `v11_to_v12` stamps the version; `tests/fixtures/schema_v12.pubrs` holds data and fields. Design
  notes: ADR 0023.

Run `cargo test -p pubrs-io --test merge` (30 address labels, a postcard set from XLSX, the collapse rule, undo, save and
reopen without the source) and `cargo test -p pubrs-layout --test merge` (a field lays out like the same typed text, with
drop caps, tabs, caps and hyphenation; the cache follows the preview).

Try it:

  printf 'Name,City\nAnn,Oslo\nBo,Rome\n' > list.csv
  cargo run -p pubrs-cli -- merge examples/samples/bakery-trifold.pubrs --data list.csv -o merged.pdf   # needs fields in the text
  cargo run -p pubrs-app            # Mailings > Mail Merge..., choose list.csv, insert fields, step through the records
