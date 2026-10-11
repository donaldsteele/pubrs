//! Mail merge (M24): data sources, fields, the collapse rule, output.
#![allow(clippy::unwrap_used)]

mod common;

use std::io::Write;

use pubrs_core::merge::{merge_to_publication, plan_paragraph, previewing, realise_story};
use pubrs_core::*;
use pubrs_io::merge_source::{from_bytes, from_inline_csv, parse_csv, xlsx_sheet_names};
use pubrs_io::{ExportContext, read_bytes, sheet_preset, write_bytes};
use pubrs_layout::{DocumentLayout, Layouter};
use pubrs_render::PdfOptions;

use common::test_fonts;

// ---------------------------------------------------------------- fixtures

enum Cell<'a> {
    S(&'a str),
    N(f64),
    /// An Excel serial date (formatted as a date).
    D(f64),
    Empty,
}

fn col_name(i: usize) -> String {
    ((b'A' + i as u8) as char).to_string()
}

/// A minimal .xlsx (a zip of XML parts) with the given sheets.
fn xlsx(sheets: &[(&str, Vec<Vec<Cell>>)]) -> Vec<u8> {
    let esc = |s: &str| s.replace('&', "&amp;").replace('<', "&lt;");
    let mut strings: Vec<String> = Vec::new();
    let mut sheet_xml: Vec<String> = Vec::new();
    for (_, rows) in sheets {
        let mut x = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\
             <worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>",
        );
        for (r, row) in rows.iter().enumerate() {
            x.push_str(&format!("<row r=\"{}\">", r + 1));
            for (c, cell) in row.iter().enumerate() {
                let at = format!("{}{}", col_name(c), r + 1);
                match cell {
                    Cell::S(s) => {
                        let i = strings.iter().position(|t| t == s).unwrap_or_else(|| {
                            strings.push((*s).to_string());
                            strings.len() - 1
                        });
                        x.push_str(&format!("<c r=\"{at}\" t=\"s\"><v>{i}</v></c>"));
                    }
                    Cell::N(n) => x.push_str(&format!("<c r=\"{at}\"><v>{n}</v></c>")),
                    Cell::D(n) => x.push_str(&format!("<c r=\"{at}\" s=\"1\"><v>{n}</v></c>")),
                    Cell::Empty => {}
                }
            }
            x.push_str("</row>");
        }
        x.push_str("</sheetData></worksheet>");
        sheet_xml.push(x);
    }
    let head = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>";
    let main = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";
    let rel = "http://schemas.openxmlformats.org/officeDocument/2006/relationships";
    let mut types = format!(
        "{head}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">\
         <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>\
         <Default Extension=\"xml\" ContentType=\"application/xml\"/>\
         <Override PartName=\"/xl/workbook.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml\"/>\
         <Override PartName=\"/xl/sharedStrings.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml\"/>\
         <Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/>"
    );
    let mut workbook = format!("{head}<workbook xmlns=\"{main}\" xmlns:r=\"{rel}\"><sheets>");
    let mut wb_rels = format!(
        "{head}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">"
    );
    for (i, (name, _)) in sheets.iter().enumerate() {
        let n = i + 1;
        types.push_str(&format!(
            "<Override PartName=\"/xl/worksheets/sheet{n}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>"
        ));
        workbook.push_str(&format!(
            "<sheet name=\"{}\" sheetId=\"{n}\" r:id=\"rId{n}\"/>",
            esc(name)
        ));
        wb_rels.push_str(&format!(
            "<Relationship Id=\"rId{n}\" Type=\"{rel}/worksheet\" Target=\"worksheets/sheet{n}.xml\"/>"
        ));
    }
    let k = sheets.len();
    wb_rels.push_str(&format!(
        "<Relationship Id=\"rId{}\" Type=\"{rel}/sharedStrings\" Target=\"sharedStrings.xml\"/>\
         <Relationship Id=\"rId{}\" Type=\"{rel}/styles\" Target=\"styles.xml\"/></Relationships>",
        k + 1,
        k + 2
    ));
    types.push_str("</Types>");
    workbook.push_str("</sheets></workbook>");
    let root_rels = format!(
        "{head}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">\
         <Relationship Id=\"rId1\" Type=\"{rel}/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>"
    );
    let mut sst = format!(
        "{head}<sst xmlns=\"{main}\" count=\"{0}\" uniqueCount=\"{0}\">",
        strings.len()
    );
    for s in &strings {
        sst.push_str(&format!("<si><t>{}</t></si>", esc(s)));
    }
    sst.push_str("</sst>");
    let styles = format!(
        "{head}<styleSheet xmlns=\"{main}\"><cellXfs count=\"2\">\
         <xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/>\
         <xf numFmtId=\"14\" fontId=\"0\" fillId=\"0\" borderId=\"0\" applyNumberFormat=\"1\"/>\
         </cellXfs></styleSheet>"
    );
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    let mut put = |name: &str, body: &str| {
        zip.start_file(name, opts).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    };
    put("[Content_Types].xml", &types);
    put("_rels/.rels", &root_rels);
    put("xl/workbook.xml", &workbook);
    put("xl/_rels/workbook.xml.rels", &wb_rels);
    put("xl/sharedStrings.xml", &sst);
    put("xl/styles.xml", &styles);
    for (i, x) in sheet_xml.iter().enumerate() {
        put(&format!("xl/worksheets/sheet{}.xml", i + 1), x);
    }
    zip.finish().unwrap().into_inner()
}

fn para(parts: &[&str]) -> Paragraph {
    // "{Name}" parts are fields, anything else is plain text.
    Paragraph {
        style: None,
        props: ParaProps::default(),
        runs: parts
            .iter()
            .map(
                |p| match p.strip_prefix('{').and_then(|s| s.strip_suffix('}')) {
                    Some(name) => Run::field(name, None, CharProps::default()),
                    None => Run::plain(p),
                },
            )
            .collect(),
    }
}

/// A document with one text frame holding `paragraphs`.
fn doc_with(setup: PageSetup, rect: Rect, paragraphs: Vec<Paragraph>) -> Document {
    let mut b = DocBuilder::new(SeqIds::new(24), setup);
    let page = b.page(0);
    let fid = b.text(page, rect, "").unwrap();
    let sid = b.story_of(fid);
    b.exec(Command::SetStory {
        story: sid,
        paragraphs,
    })
    .unwrap();
    b.finish()
}

fn setup(w: f64, h: f64) -> PageSetup {
    PageSetup {
        size: Size::new(w, h),
        margins: Margins::uniform(0.0),
        ..PageSetup::default()
    }
}

fn story_id(doc: &Document) -> StoryId {
    *doc.stories.keys().next().unwrap()
}

/// What the layout draws for a story: one line of text per layout line.
fn drawn(layout: &DocumentLayout, story: StoryId) -> String {
    let mut out: Vec<String> = Vec::new();
    for ft in layout.stories[&story].frames.values() {
        for line in &ft.lines {
            let mut s = String::new();
            for r in &line.runs {
                for g in &r.glyphs {
                    s.push_str(&r.text[g.text.clone()]);
                }
            }
            out.push(s.trim_end().to_string());
        }
    }
    out.join("\n")
}

fn layout_of(doc: &Document) -> DocumentLayout {
    Layouter::new(test_fonts()).layout_document(doc)
}

fn data(fields: &[&str], rows: &[&[&str]]) -> MergeSource {
    MergeSource::new(
        "test.csv",
        MergeKind::Csv,
        None,
        fields.iter().map(|s| s.to_string()).collect(),
        rows.iter()
            .map(|r| r.iter().map(|s| s.to_string()).collect())
            .collect(),
    )
}

fn pdf_pages(bytes: &[u8]) -> usize {
    lopdf::Document::load_mem(bytes).unwrap().get_pages().len()
}

// ---------------------------------------------------------------- CSV

#[test]
fn csv_quotes_bom_semicolons_and_ragged_rows() {
    // Quotes, doubled quotes, a newline inside quotes, CRLF rows.
    let t = parse_csv("Name,Note\r\n\"Lee, Ann\",\"says \"\"hi\"\"\nthen leaves\"\r\nBo,x\r\n")
        .unwrap();
    assert_eq!(t.fields, ["Name", "Note"]);
    assert_eq!(t.records[0], ["Lee, Ann", "says \"hi\"\nthen leaves"]);
    assert_eq!(t.records[1], ["Bo", "x"]);
    // A byte order mark and semicolons (a header line decides).
    let t = parse_csv("\u{feff}Name;City\nAnn;Oslo\nBo;Rome\n").unwrap();
    assert_eq!(t.fields, ["Name", "City"]);
    assert_eq!(t.records, [["Ann", "Oslo"], ["Bo", "Rome"]]);
    // Commas inside quotes do not decide the delimiter.
    let t = parse_csv("\"a,b,c\";d\n1;2\n").unwrap();
    assert_eq!(t.fields, ["a,b,c", "d"]);
    // Ragged rows: short rows are padded, long ones cut, blank lines skipped.
    let t = parse_csv("a,b,c\n1\n\n1,2,3,4\n").unwrap();
    assert_eq!(t.records, [["1", "", ""], ["1", "2", "3"]]);
    // The bytes path takes UTF-8 with a BOM, and refuses anything else.
    let src = from_bytes("f.csv", b"\xEF\xBB\xBFa,b\n1,2\n", None).unwrap();
    assert_eq!(src.fields, ["a", "b"]);
    assert!(from_bytes("f.csv", b"a,b\n\xff,2\n", None).is_err());
    assert!(from_bytes("f.csv", b"", None).is_err());
}

#[test]
fn csv_limits_are_reported() {
    let big = format!("a\n{}\n", "x".repeat(pubrs_core::merge::MAX_CELL_BYTES + 1));
    let e = parse_csv(&big).unwrap_err().to_string();
    assert!(e.contains("1048576"), "{e}");
}

// ---------------------------------------------------------------- XLSX

#[test]
fn xlsx_first_and_named_sheet_with_dates_and_numbers() {
    let book = xlsx(&[
        (
            "People",
            vec![
                vec![
                    Cell::S("Name"),
                    Cell::S("Age"),
                    Cell::S("Joined"),
                    Cell::S("Note"),
                ],
                vec![Cell::S("Ann"), Cell::N(41.0), Cell::D(45292.0), Cell::Empty],
                vec![
                    Cell::S("Bo"),
                    Cell::N(2.5),
                    Cell::D(45293.75),
                    Cell::S("late"),
                ],
            ],
        ),
        (
            "Pets",
            vec![
                vec![Cell::S("Pet")],
                vec![Cell::S("Rex")],
                vec![Cell::S("Tom")],
                vec![Cell::S("Zed")],
            ],
        ),
    ]);
    assert_eq!(xlsx_sheet_names(&book).unwrap(), ["People", "Pets"]);
    let first = from_bytes("book.xlsx", &book, None).unwrap();
    assert_eq!(first.kind, MergeKind::Xlsx);
    assert_eq!(first.sheet.as_deref(), Some("People"));
    assert_eq!(first.fields, ["Name", "Age", "Joined", "Note"]);
    // Whole numbers lose ".0", dates are ISO (with the time when there is one).
    assert_eq!(first.records[0], ["Ann", "41", "2024-01-01", ""]);
    assert_eq!(
        first.records[1],
        ["Bo", "2.5", "2024-01-02T18:00:00", "late"]
    );
    let pets = from_bytes("book.xlsx", &book, Some("pets")).unwrap();
    assert_eq!(pets.sheet.as_deref(), Some("Pets"));
    assert_eq!(pets.record_count(), 3);
    let e = from_bytes("book.xlsx", &book, Some("Nope"))
        .unwrap_err()
        .to_string();
    assert!(e.contains("People, Pets"), "{e}");
    assert!(from_bytes("book.xlsx", b"not a zip", None).is_err());
}

// ---------------------------------------------------------------- fields

#[test]
fn a_field_shows_its_name_then_the_previewed_value() {
    let mut doc = doc_with(
        setup(300.0, 100.0),
        Rect::new(10.0, 10.0, 280.0, 80.0),
        vec![para(&["Dear ", "{First}", " ", "{Last}", ","])],
    );
    let sid = story_id(&doc);
    assert_eq!(
        drawn(&layout_of(&doc), sid),
        "Dear \u{ab}First\u{bb} \u{ab}Last\u{bb},"
    );
    let mut m = data(&["First", "Last"], &[&["Ann", "Lee"], &["Bo", "Kim"]]);
    doc.merge = Some(m.clone());
    // Preview off: still the names.
    assert!(drawn(&layout_of(&doc), sid).contains('\u{ab}'));
    m.preview = true;
    doc.merge = Some(m.clone());
    assert_eq!(drawn(&layout_of(&doc), sid), "Dear Ann Lee,");
    m.preview_index = 1;
    doc.merge = Some(m);
    assert_eq!(drawn(&layout_of(&doc), sid), "Dear Bo Kim,");
}

#[test]
fn carets_treat_a_field_as_one_character() {
    let mut doc = doc_with(
        setup(300.0, 100.0),
        Rect::new(10.0, 10.0, 280.0, 80.0),
        vec![para(&["ab", "{Name}", "cd"])],
    );
    let mut m = data(&["Name"], &[&["Wolfeschlegelstein"]]);
    m.preview = true;
    doc.merge = Some(m);
    let layout = layout_of(&doc);
    let sid = story_id(&doc);
    let line = &layout.stories[&sid].frames.values().next().unwrap().lines[0];
    // 5 story characters: 6 carets, one per boundary, strictly increasing.
    let offs: Vec<usize> = line.carets.iter().map(|c| c.0).collect();
    assert_eq!(offs, [0, 1, 2, 3, 4, 5]);
    assert!(line.carets.windows(2).all(|w| w[1].1 >= w[0].1));
    // The field is wide: the caret after it is far from the one before it.
    assert!(line.carets[3].1 - line.carets[2].1 > 50.0);
    assert_eq!(line.chars, 0..5);
}

#[test]
fn an_empty_value_collapses_one_adjacent_space() {
    let mut m = data(
        &["First", "Mid", "Last"],
        &[&["Ann", "", "Lee"], &["Bo", "Q", "Kim"]],
    );
    m.preview = true;
    // The space after the field goes: "Ann Lee", not "Ann  Lee".
    let story_of = |parts: &[&str], m: &MergeSource| {
        let mut doc = doc_with(
            setup(300.0, 100.0),
            Rect::new(10.0, 10.0, 280.0, 80.0),
            vec![para(parts)],
        );
        doc.merge = Some(m.clone());
        let sid = story_id(&doc);
        let shown = drawn(&layout_of(&doc), sid);
        let mut s = doc.story(sid).unwrap().clone();
        realise_story(Some(m), &mut s);
        (shown, s.plain_text())
    };
    let parts = ["{First}", " ", "{Mid}", " ", "{Last}"];
    let (shown, real) = story_of(&parts, &m);
    assert_eq!((shown.as_str(), real.as_str()), ("Ann Lee", "Ann Lee"));
    m.preview_index = 1;
    let (shown, real) = story_of(&parts, &m);
    assert_eq!((shown.as_str(), real.as_str()), ("Bo Q Kim", "Bo Q Kim"));
    // At the end there is no space after: the one before goes.
    m.preview_index = 0;
    let (shown, real) = story_of(&["{First}", " ", "{Mid}"], &m);
    assert_eq!((shown.as_str(), real.as_str()), ("Ann", "Ann"));
    // Only one space per empty field; a space is never taken twice.
    let (_, real) = story_of(&["a", " ", "{Mid}", " ", "{Mid}", " ", "b"], &m);
    assert_eq!(real, "a b");
    let (_, real) = story_of(&["a", "{Mid}", "{Mid}", "b"], &m);
    assert_eq!(real, "ab");
    let (shown, _) = story_of(&["a", " ", "{Mid}", " ", "b"], &m);
    assert_eq!(shown, "a b");
    // Fields show as «Name» (and take no space) while not previewed.
    let mut off = m.clone();
    off.preview = false;
    let (shown, real) = story_of(&["a ", "{Mid}", " b"], &off);
    assert_eq!(shown, "a \u{ab}Mid\u{bb} b");
    assert_eq!(real, "a \u{ab}Mid\u{bb} b");
    // The plan itself: the field's own character stays inside the range.
    let p = para(&["x ", "{Mid}", " y"]);
    let plan = plan_paragraph(Some(&m), &p);
    assert_eq!((plan[0].start, plan[0].end, plan[0].field_at), (2, 4, 2));
}

#[test]
fn a_missing_field_warns_and_shows_its_name() {
    let mut doc = doc_with(
        setup(300.0, 100.0),
        Rect::new(10.0, 10.0, 280.0, 80.0),
        vec![para(&["{Name}", " ", "{Nickname}"])],
    );
    let mut m = data(&["Name"], &[&["Ann"]]);
    m.preview = true;
    doc.merge = Some(m);
    let sid = story_id(&doc);
    assert_eq!(drawn(&layout_of(&doc), sid), "Ann \u{ab}Nickname\u{bb}");
    let w: Vec<_> = validate::validate(&doc)
        .into_iter()
        .filter(|w| w.code == WarningCode::MergeFieldMissing)
        .collect();
    assert_eq!(w.len(), 1);
    assert!(w[0].message.contains("Nickname"), "{}", w[0].message);
    assert!(w[0].hint.contains("Name"));
    // Without data there is nothing to compare with.
    doc.merge = None;
    assert!(
        validate::validate(&doc)
            .iter()
            .all(|w| w.code != WarningCode::MergeFieldMissing)
    );
}

#[test]
fn inserting_a_field_is_one_undo_step() {
    let doc = doc_with(
        setup(300.0, 100.0),
        Rect::new(10.0, 10.0, 280.0, 80.0),
        vec![Paragraph::plain("Dear ,")],
    );
    let sid = story_id(&doc);
    let mut ed = Editor::new(doc.clone());
    ed.execute(Command::SetMergeSource {
        merge: Some(data(&["Name"], &[&["Ann"]])),
    })
    .unwrap();
    let before = ed.doc().clone();
    ed.execute(Command::InsertMergeField {
        story: sid,
        at: 5,
        field: "Name".into(),
    })
    .unwrap();
    let s = ed.doc().story(sid).unwrap();
    assert_eq!(s.char_len(), 7, "a field is one character");
    assert_eq!(
        s.paragraphs[0].runs.len(),
        3,
        "the field is a run of its own"
    );
    assert_eq!(s.paragraphs[0].runs[1].field.as_deref(), Some("Name"));
    // Typing next to it does not merge with it.
    ed.execute(Command::InsertText {
        story: sid,
        at: 6,
        text: "!".into(),
        format: None,
    })
    .unwrap();
    let runs = &ed.doc().story(sid).unwrap().paragraphs[0].runs;
    assert_eq!(
        runs.len(),
        3,
        "the typed text joins the text, not the field"
    );
    assert_eq!(runs[1].text, "\u{fffc}");
    assert_eq!(runs[2].text, "!,");
    ed.undo().unwrap();
    ed.undo().unwrap();
    assert_eq!(*ed.doc(), before);
    ed.redo().unwrap();
    assert_eq!(ed.doc().story(sid).unwrap().char_len(), 7);
    // A field the data does not have is refused, and says what it has.
    let err = ed
        .execute(Command::InsertMergeField {
            story: sid,
            at: 0,
            field: "Nope".into(),
        })
        .unwrap_err()
        .to_string();
    assert!(err.contains("Name"), "{err}");
    // Preview index and switch are commands too.
    ed.execute(Command::SetMergePreview {
        index: Some(0),
        preview: Some(true),
    })
    .unwrap();
    assert!(ed.doc().merge.as_ref().unwrap().preview);
    assert!(
        ed.execute(Command::SetMergePreview {
            index: Some(5),
            preview: None
        })
        .is_err()
    );
}

#[test]
fn the_data_and_fields_survive_save_and_reopen_without_the_source() {
    let mut doc = doc_with(
        setup(300.0, 100.0),
        Rect::new(10.0, 10.0, 280.0, 80.0),
        vec![para(&["Hi ", "{Name}"])],
    );
    let src = from_inline_csv("Name,City\nAnn,Oslo\n\"Bo \"\"B\"\"\",Rome\n").unwrap();
    assert_eq!(src.source_name, "inline");
    doc.merge = Some(MergeSource {
        preview: true,
        preview_index: 1,
        ..src
    });
    let bytes = write_bytes(&doc, None).unwrap();
    let json = String::from_utf8_lossy(&bytes);
    assert!(json.len() > 100);
    let back = read_bytes(&bytes).unwrap();
    assert_eq!(back.merge, doc.merge);
    assert_eq!(back.schema_version, SCHEMA_VERSION);
    let s = back.story(story_id(&back)).unwrap();
    assert_eq!(s.paragraphs[0].runs[1].field.as_deref(), Some("Name"));
    assert_eq!(s.paragraphs[0].runs[1].text, "\u{fffc}");
    assert_eq!(drawn(&layout_of(&back), story_id(&back)), "Hi Bo \"B\"");
    // Deterministic bytes.
    assert_eq!(write_bytes(&back, None).unwrap(), bytes);
}

// ---------------------------------------------------------------- output

fn address_labels(n: usize) -> (Document, Vec<String>) {
    let preset = sheet_preset("avery-5160").unwrap();
    let sheet = preset.layout(SheetFill::Sequence);
    let size = sheet.label_size;
    let mut doc = doc_with(
        PageSetup {
            size,
            margins: Margins::uniform(0.0),
            sheet: Some(sheet),
            ..PageSetup::default()
        },
        Rect::new(4.0, 4.0, size.width - 8.0, size.height - 8.0),
        vec![
            para(&["{Name}"]),
            para(&["{Street}"]),
            para(&["{City}", ", ", "{State}"]),
        ],
    );
    let mut csv = String::from("Name,Street,City,State\n");
    let mut want = Vec::new();
    for i in 1..=n {
        csv.push_str(&format!("Person {i},{i} Main St,Town {i},TX\n"));
        want.push(format!("Person {i}\n{i} Main St\nTown {i}, TX"));
    }
    doc.merge = Some(from_inline_csv(&csv).unwrap());
    (doc, want)
}

#[test]
fn thirty_address_labels_fill_one_avery_5160_sheet() {
    let (doc, want) = address_labels(30);
    assert_eq!(want.len(), 30);
    let mut ctx = ExportContext::new(test_fonts());
    let pdf = ctx.export_merged_pdf(&doc, &PdfOptions::default()).unwrap();
    // One Letter sheet with 30 cells.
    let parsed = lopdf::Document::load_mem(&pdf).unwrap();
    assert_eq!(parsed.get_pages().len(), 1);
    let sheet = doc.setup.sheet.clone().unwrap();
    assert_eq!(sheet.cells(), 30);
    // Each cell's text is its record: the page text, in order.
    let text = parsed.extract_text(&[1]).unwrap();
    let mut at = 0;
    for (i, w) in want.iter().enumerate() {
        let name = format!("Person {}", i + 1);
        let found = text[at..].find(&name);
        assert!(
            found.is_some(),
            "{name} missing or out of order in {text:?}"
        );
        at += found.unwrap() + name.len();
        let town = format!("Town {}", i + 1);
        assert!(text.contains(&town), "{town} missing: {w}");
    }
    // The realised publication has the same text, one label per page.
    let merged = merge_to_publication(&doc).unwrap();
    assert_eq!(merged.pages.len(), 30);
    assert!(merged.merge.is_none());
    for (i, w) in want.iter().enumerate() {
        let f = merged.pages[i].frames[0];
        let s = merged.story(merged.story_of(f).unwrap()).unwrap();
        assert_eq!(&s.plain_text(), w, "label {i}");
    }
    // 31 labels need a second sheet; a repeat sheet gets one sheet per record.
    let (mut doc31, _) = address_labels(31);
    let pdf = ctx
        .export_merged_pdf(&doc31, &PdfOptions::default())
        .unwrap();
    assert_eq!(pdf_pages(&pdf), 2);
    doc31.setup.sheet.as_mut().unwrap().fill = SheetFill::Repeat;
    let pdf = ctx
        .export_merged_pdf(&doc31, &PdfOptions::default())
        .unwrap();
    assert_eq!(pdf_pages(&pdf), 31);
}

#[test]
fn a_postcard_set_from_xlsx_has_a_page_per_record() {
    let book = xlsx(&[(
        "Guests",
        vec![
            vec![Cell::S("Guest"), Cell::S("Table")],
            vec![Cell::S("Ann Lee"), Cell::N(4.0)],
            vec![Cell::S("Bo Kim"), Cell::N(7.0)],
            vec![Cell::S("Cy Fox"), Cell::N(12.0)],
        ],
    )]);
    let mut doc = doc_with(
        setup(432.0, 288.0),
        Rect::new(20.0, 20.0, 392.0, 200.0),
        vec![
            para(&["Dear ", "{Guest}", ","]),
            para(&["Table ", "{Table}"]),
        ],
    );
    // A second page per record: the page set repeats.
    let mut b = DocBuilder::new(SeqIds::new(99), setup(432.0, 288.0));
    let _ = &mut b;
    doc.merge = Some(from_bytes("guests.xlsx", &book, None).unwrap());
    let mut ctx = ExportContext::new(test_fonts());
    let pdf = ctx.export_merged_pdf(&doc, &PdfOptions::default()).unwrap();
    let parsed = lopdf::Document::load_mem(&pdf).unwrap();
    assert_eq!(parsed.get_pages().len(), 3);
    for (i, who) in ["Ann Lee", "Bo Kim", "Cy Fox"].iter().enumerate() {
        let t = parsed.extract_text(&[i as u32 + 1]).unwrap();
        assert!(t.contains(who), "page {i}: {t:?}");
    }
    // As a publication: a new document with the same pages, plain text.
    let merged = merge_to_publication(&doc).unwrap();
    assert_eq!(merged.pages.len(), 3);
    let texts: Vec<String> = (0..3)
        .map(|i| {
            let f = merged.pages[i].frames[0];
            merged
                .story(merged.story_of(f).unwrap())
                .unwrap()
                .plain_text()
        })
        .collect();
    assert_eq!(texts[2], "Dear Cy Fox,\nTable 12");
    // Ids are unique and the document is sound.
    assert!(validate::check_integrity(&merged).is_empty());
    // And it reopens.
    let again = read_bytes(&write_bytes(&merged, None).unwrap()).unwrap();
    assert_eq!(again.pages.len(), 3);
}

#[test]
fn a_merge_without_data_is_an_error_with_a_hint() {
    let doc = doc_with(
        setup(300.0, 100.0),
        Rect::new(10.0, 10.0, 280.0, 80.0),
        vec![para(&["{Name}"])],
    );
    let mut ctx = ExportContext::new(test_fonts());
    let e = ctx
        .export_merged_pdf(&doc, &PdfOptions::default())
        .unwrap_err();
    assert!(e.to_string().contains("no merge data"), "{e}");
    let e = merge_to_publication(&doc).unwrap_err();
    assert!(e.hint().contains("merge_set_source"));
    // Data but no records.
    let mut d = doc.clone();
    d.merge = Some(data(&["Name"], &[]));
    assert!(merge_to_publication(&d).is_err());
    // Data and records but no fields in the text.
    let mut d = doc_with(
        setup(300.0, 100.0),
        Rect::new(10.0, 10.0, 280.0, 80.0),
        vec![Paragraph::plain("plain")],
    );
    d.merge = Some(data(&["Name"], &[&["Ann"]]));
    assert!(
        merge_to_publication(&d)
            .unwrap_err()
            .to_string()
            .contains("no merge fields")
    );
    let _ = previewing(d.merge.as_ref().unwrap(), 0);
}

// ---------------------------------------------------------------- limits

/// `book` with the part `name` replaced by `body`.
fn with_part(book: &[u8], name: &str, body: &[u8]) -> Vec<u8> {
    let mut src = zip::ZipArchive::new(std::io::Cursor::new(book)).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for i in 0..src.len() {
        let mut f = src.by_index(i).unwrap();
        let n = f.name().unwrap().to_string();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut data).unwrap();
        out.start_file(&n, opts).unwrap();
        out.write_all(if n == name { body } else { &data }).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn limit_msg(r: Result<MergeSource, pubrs_io::merge_source::MergeReadError>) -> String {
    r.expect_err("must be refused").to_string()
}

#[test]
fn a_sparse_xlsx_with_a_far_corner_is_refused_fast() {
    let book = xlsx(&[("S", vec![vec![Cell::S("Name")], vec![Cell::S("Ann")]])]);
    let sheet = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
        <worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\
        <dimension ref=\"A1:XFD1048576\"/><sheetData>\
        <row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>Name</t></is></c></row>\
        <row r=\"1048576\"><c r=\"XFD1048576\" t=\"inlineStr\"><is><t>x</t></is></c></row>\
        </sheetData></worksheet>";
    let book = with_part(&book, "xl/worksheets/sheet1.xml", sheet.as_bytes());
    assert!(book.len() < 4096, "a tiny file");
    let t = std::time::Instant::now();
    let e = limit_msg(from_bytes("far.xlsx", &book, None));
    assert!(t.elapsed().as_secs_f64() < 1.0, "took {:?}", t.elapsed());
    assert!(e.contains("limit"), "{e}");
    // The same file without the far cell reads as one record, sparsely.
    let ok = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
        <worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\">\
        <sheetData><row r=\"2\"><c r=\"C2\" t=\"inlineStr\"><is><t>H</t></is></c><c r=\"D2\" t=\"inlineStr\"><is><t>I</t></is></c></row>\
        <row r=\"900\"><c r=\"D900\" t=\"inlineStr\"><is><t>v</t></is></c></row></sheetData></worksheet>";
    let book = with_part(&book, "xl/worksheets/sheet1.xml", ok.as_bytes());
    let s = from_bytes("ok.xlsx", &book, None).unwrap();
    assert_eq!(s.fields, ["H", "I"]);
    assert_eq!(s.records[0], ["", "v"]);
}

#[test]
fn a_zip_bomb_and_a_broken_xlsx_are_refused() {
    let book = xlsx(&[("S", vec![vec![Cell::S("Name")], vec![Cell::S("Ann")]])]);
    // 8 MiB of zeros in the shared strings: a ratio over 1000.
    let mut sst =
        b"<sst xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><si><t>"
            .to_vec();
    sst.extend(std::iter::repeat_n(b' ', 8 << 20));
    sst.extend(b"</t></si></sst>");
    let bomb = with_part(&book, "xl/sharedStrings.xml", &sst);
    assert!(bomb.len() < 100_000);
    let e = limit_msg(from_bytes("bomb.xlsx", &bomb, None));
    assert!(e.contains("inflated size"), "{e}");
    assert!(xlsx_sheet_names(&bomb).is_err());
    // A truncated zip.
    let cut = &book[..book.len() / 2];
    assert!(from_bytes("cut.xlsx", cut, None).is_err());
    assert!(from_bytes("junk.xlsx", b"PK\x03\x04 not really", None).is_err());
}

#[test]
fn too_many_cells_and_oversized_input_are_refused() {
    // 512 columns by 20,000 one-cell rows: 10 million cells, over the cap,
    // refused before any row is padded.
    let header: Vec<String> = (0..512).map(|i| format!("c{i}")).collect();
    let mut csv = header.join(",");
    csv.push('\n');
    for _ in 0..20_000 {
        csv.push_str("x\n");
    }
    let e = limit_msg(from_inline_csv(&csv));
    assert!(e.contains("5000000 cells"), "{e}");
    // Data of that shape arriving as a command is refused too.
    let mut m = MergeSource::new(
        "x",
        MergeKind::Csv,
        None,
        (0..512).map(|i| format!("c{i}")).collect(),
        vec![],
    );
    m.records = std::sync::Arc::new(vec![vec![String::new(); 512]; 10_000]);
    assert!(
        Command::SetMergeSource {
            merge: Some(m.clone())
        }
        .apply(&mut doc_with(
            setup(300.0, 100.0),
            Rect::new(10.0, 10.0, 280.0, 80.0),
            vec![Paragraph::plain("x")],
        ))
        .is_err()
    );
    // ...and a file that holds it does not load.
    let mut doc = doc_with(
        setup(300.0, 100.0),
        Rect::new(10.0, 10.0, 280.0, 80.0),
        vec![Paragraph::plain("x")],
    );
    doc.merge = Some(m);
    assert!(!validate::check_integrity(&doc).is_empty());
    // A comma bomb: millions of empty cells in a small file.
    let commas = ",".repeat(5_100_000);
    let wide = format!("a\n{commas}\n");
    assert!(from_inline_csv(&wide).is_err());
    // Oversized input.
    let big = vec![b'a'; (64 << 20) + 1];
    let e = limit_msg(from_bytes("big.csv", &big, None));
    assert!(e.contains("64 MiB"), "{e}");
    assert!(from_bytes("big.xlsx", &big, None).is_err());
    // Too many output pages.
    let mut d = doc_with(
        setup(300.0, 100.0),
        Rect::new(10.0, 10.0, 280.0, 80.0),
        vec![para(&["{A}"])],
    );
    let rows: Vec<Vec<String>> = (0..20_001).map(|i| vec![i.to_string()]).collect();
    d.merge = Some(MergeSource::new(
        "x",
        MergeKind::Csv,
        None,
        vec!["A".into()],
        rows,
    ));
    assert!(
        merge_to_publication(&d)
            .unwrap_err()
            .to_string()
            .contains("20000")
    );
}

// ------------------------------------------------- crafted workbooks (round 2)

const SHEET_HEAD: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
    <worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><sheetData>";

fn base_book() -> Vec<u8> {
    xlsx(&[("S", vec![vec![Cell::S("Name")], vec![Cell::S("Ann")]])])
}

fn timed(book: &[u8]) -> String {
    let t = std::time::Instant::now();
    let e = from_bytes("c.xlsx", book, None).expect_err("must be refused");
    assert!(t.elapsed().as_secs_f64() < 2.0, "took {:?}", t.elapsed());
    e.to_string()
}

#[test]
fn a_part_that_declares_a_small_size_is_still_counted_by_what_it_inflates_to() {
    let mut sst =
        b"<sst xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><si><t>"
            .to_vec();
    sst.extend(std::iter::repeat_n(b' ', 8 << 20));
    sst.extend(b"</t></si></sst>");
    let mut book = with_part(&base_book(), "xl/sharedStrings.xml", &sst);
    // Lie in the central directory: uncompressed size 1000.
    let mut i = 0;
    let mut patched = false;
    while i + 46 < book.len() {
        if &book[i..i + 4] == b"PK\x01\x02" {
            let n = u16::from_le_bytes([book[i + 28], book[i + 29]]) as usize;
            if &book[i + 46..i + 46 + n] == b"xl/sharedStrings.xml" {
                book[i + 24..i + 28].copy_from_slice(&1000u32.to_le_bytes());
                patched = true;
            }
        }
        i += 1;
    }
    assert!(patched);
    let e = timed(&book);
    assert!(
        e.contains("larger than its declared") || e.contains("inflated size"),
        "{e}"
    );
}

#[test]
fn a_big_shared_string_referenced_many_times_is_stopped_by_the_text_total() {
    let big = "x".repeat(1_000_000);
    let sst = format!(
        "<sst xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" count=\"100\" uniqueCount=\"1\"><si><t>{big}</t></si></sst>"
    );
    let mut rows = String::from(SHEET_HEAD);
    rows.push_str("<row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>H</t></is></c></row>");
    for r in 2..100 {
        rows.push_str(&format!(
            "<row r=\"{r}\"><c r=\"A{r}\" t=\"s\"><v>0</v></c></row>"
        ));
    }
    rows.push_str("</sheetData></worksheet>");
    let book = with_part(&base_book(), "xl/sharedStrings.xml", sst.as_bytes());
    let book = with_part(&book, "xl/worksheets/sheet1.xml", rows.as_bytes());
    assert!(book.len() < 20_000);
    let e = timed(&book);
    assert!(e.contains("64 MiB"), "{e}");
    // The same total through the command is refused as well.
    let mut m = data(&["A"], &[]);
    m.records = std::sync::Arc::new(vec![vec![big.clone()]; 70]);
    assert!(pubrs_core::merge::check_source(&m).is_err());
}

#[test]
fn out_of_order_cells_are_refused_not_wrapped() {
    let cell = |r: &str| format!("<c r=\"{r}\" t=\"inlineStr\"><is><t>v</t></is></c>");
    // A header cell left of the first one.
    let left = format!(
        "{SHEET_HEAD}<row r=\"1\">{}{}</row></sheetData></worksheet>",
        cell("C1"),
        cell("A1")
    );
    let book = with_part(&base_book(), "xl/worksheets/sheet1.xml", left.as_bytes());
    let e = from_bytes("c.xlsx", &book, None);
    // Either refused as out of order, or read without a panic.
    assert!(e.is_err(), "{:?}", e.map(|s| s.fields));
    // A row above the header row.
    let above = format!(
        "{SHEET_HEAD}<row r=\"5\">{}</row><row r=\"2\">{}</row></sheetData></worksheet>",
        cell("A5"),
        cell("A2")
    );
    let book = with_part(&base_book(), "xl/worksheets/sheet1.xml", above.as_bytes());
    let t = std::time::Instant::now();
    let r = from_bytes("c.xlsx", &book, None);
    assert!(t.elapsed().as_secs_f64() < 2.0);
    assert!(r.is_err(), "{:?}", r.map(|s| s.fields));
}

#[test]
fn a_csv_row_over_512_cells_is_refused() {
    let csv = format!("a\n{}\n", ",".repeat(600));
    let e = from_inline_csv(&csv).unwrap_err().to_string();
    assert!(e.contains("512"), "{e}");
}

// ------------------------------------------------- the shared strings are rewritten

/// `book` with the part `old` renamed to `new` (same bytes).
fn rename_part(book: &[u8], old: &str, new: &str) -> Vec<u8> {
    let mut src = zip::ZipArchive::new(std::io::Cursor::new(book)).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for i in 0..src.len() {
        let mut f = src.by_index(i).unwrap();
        let n = f.name().unwrap().to_string();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut data).unwrap();
        out.start_file(if n == old { new } else { &n }, opts)
            .unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

fn add_part(book: &[u8], name: &str, body: &[u8]) -> Vec<u8> {
    let tmp = with_part(book, "[Content_Types].xml", &{
        let mut z = zip::ZipArchive::new(std::io::Cursor::new(book)).unwrap();
        let mut d = Vec::new();
        std::io::Read::read_to_end(&mut z.by_name("[Content_Types].xml").unwrap(), &mut d).unwrap();
        d
    });
    let mut src = zip::ZipArchive::new(std::io::Cursor::new(tmp)).unwrap();
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for i in 0..src.len() {
        let mut f = src.by_index(i).unwrap();
        let n = f.name().unwrap().to_string();
        let mut data = Vec::new();
        std::io::Read::read_to_end(&mut f, &mut data).unwrap();
        out.start_file(&n, opts).unwrap();
        out.write_all(&data).unwrap();
    }
    out.start_file(name, opts).unwrap();
    out.write_all(body).unwrap();
    out.finish().unwrap().into_inner()
}

const MAIN: &str = "http://schemas.openxmlformats.org/spreadsheetml/2006/main";

/// Whatever the `<sst>` claims, the workbook reads with its real values (the
/// strings are "Name" and "Ann") or is refused: never a panic, never slow.
fn reads_name_ann(sst: &str) {
    let book = with_part(&base_book(), "xl/sharedStrings.xml", sst.as_bytes());
    let t = std::time::Instant::now();
    let s = from_bytes("c.xlsx", &book, None).unwrap();
    assert!(t.elapsed().as_secs_f64() < 2.0, "took {:?}", t.elapsed());
    assert_eq!(s.fields, ["Name"]);
    assert_eq!(s.records[0], ["Ann"]);
}

#[test]
fn a_lying_unique_count_is_rewritten_however_it_is_written() {
    let si = "<si><t>Name</t></si><si><t>Ann</t></si>";
    let huge = "900000000";
    reads_name_ann(&format!(
        "<sst xmlns=\"{MAIN}\" count=\"2\" uniqueCount=\"{huge}\">{si}</sst>"
    ));
    // Spaces around the equals sign.
    reads_name_ann(&format!(
        "<sst xmlns=\"{MAIN}\" uniqueCount = \"{huge}\">{si}</sst>"
    ));
    // Single quotes, a decoy attribute first.
    reads_name_ann(&format!(
        "<sst xmlns=\"{MAIN}\" xuniqueCount=\"1\" uniqueCount='{huge}'>{si}</sst>"
    ));
    // A prefixed element.
    reads_name_ann(&format!(
        "<x:sst xmlns:x=\"{MAIN}\" uniqueCount=\"{huge}\"><x:si><x:t>Name</x:t></x:si><x:si><x:t>Ann</x:t></x:si></x:sst>"
    ));
    // The start tag far past 4 KB of comments.
    let pad = "<!-- padding -->".repeat(600);
    reads_name_ann(&format!(
        "<?xml version=\"1.0\"?>{pad}<sst xmlns=\"{MAIN}\" uniqueCount=\"{huge}\">{si}</sst>"
    ));
    // Rich text runs read as before.
    reads_name_ann(&format!(
        "<sst xmlns=\"{MAIN}\" uniqueCount=\"{huge}\"><si><r><t>Na</t></r><r><rPr><b/></rPr><t xml:space=\"preserve\">me</t></r></si><si><t>Ann</t></si></sst>"
    ));
}

#[test]
fn a_multibyte_quote_never_panics() {
    let sst = format!(
        "<sst xmlns=\"{MAIN}\" uniqueCount=\u{201c}900000000\u{201d}><si><t>Name</t></si><si><t>Ann</t></si></sst>"
    );
    let book = with_part(&base_book(), "xl/sharedStrings.xml", sst.as_bytes());
    let t = std::time::Instant::now();
    let r = from_bytes("c.xlsx", &book, None);
    assert!(t.elapsed().as_secs_f64() < 2.0);
    assert!(r.map_or(true, |s| s.records[0] == ["Ann"]));
}

#[test]
fn a_case_variant_shared_strings_part_is_rewritten_and_a_duplicate_is_refused() {
    let sst = format!(
        "<sst xmlns=\"{MAIN}\" uniqueCount=\"900000000\"><si><t>Name</t></si><si><t>Ann</t></si></sst>"
    );
    let book = with_part(&base_book(), "xl/sharedStrings.xml", sst.as_bytes());
    let upper = rename_part(&book, "xl/sharedStrings.xml", "XL/SHAREDSTRINGS.XML");
    let t = std::time::Instant::now();
    let s = from_bytes("c.xlsx", &upper, None).unwrap();
    assert!(t.elapsed().as_secs_f64() < 2.0);
    assert_eq!(s.records[0], ["Ann"]);
    // Backslashes read as slashes, as calamine does.
    let back = rename_part(&book, "xl/sharedStrings.xml", "xl\\sharedStrings.xml");
    let t = std::time::Instant::now();
    let _ = from_bytes("c.xlsx", &back, None);
    assert!(t.elapsed().as_secs_f64() < 2.0);
    // Two parts with one name ignoring case.
    let dup = add_part(&book, "xl/SHAREDSTRINGS.xml", sst.as_bytes());
    let e = timed(&dup);
    assert!(e.contains("two parts"), "{e}");
}

#[test]
fn millions_of_empty_strings_in_a_small_file_are_refused() {
    let mut sst = format!("<sst xmlns=\"{MAIN}\">");
    sst.push_str(&"<si/>".repeat(5_000_001));
    sst.push_str("</sst>");
    let book = with_part(&base_book(), "xl/sharedStrings.xml", sst.as_bytes());
    assert!(book.len() < 200_000);
    // Refused as a bomb (the ratio) or by the string count: either way.
    let e = timed(&book);
    assert!(e.contains("bomb") || e.contains("strings"), "{e}");
}

#[test]
fn an_empty_sst_element_with_a_huge_unique_count_is_rewritten_too() {
    // calamine expands empty elements, so these are start tags to it.
    let inline = format!(
        "{SHEET_HEAD}<row r=\"1\"><c r=\"A1\" t=\"inlineStr\"><is><t>H</t></is></c></row>\
         <row r=\"2\"><c r=\"A2\" t=\"inlineStr\"><is><t>v</t></is></c></row></sheetData></worksheet>"
    );
    for sst in [
        format!("<sst xmlns=\"{MAIN}\" uniqueCount=\"900000000\"/>"),
        format!("<x:sst xmlns:x=\"{MAIN}\" uniqueCount=\"900000000\" count = '9'/>"),
        format!("<?xml version=\"1.0\"?><sst xmlns=\"{MAIN}\" uniqueCount=\"900000000\"/>"),
    ] {
        let book = with_part(&base_book(), "xl/sharedStrings.xml", sst.as_bytes());
        let book = with_part(&book, "xl/worksheets/sheet1.xml", inline.as_bytes());
        let t = std::time::Instant::now();
        let s = from_bytes("c.xlsx", &book, None).unwrap();
        assert!(t.elapsed().as_secs_f64() < 2.0, "took {:?}", t.elapsed());
        assert_eq!(
            (s.fields.as_slice(), s.records[0].as_slice()),
            (&["H".to_string()][..], &["v".to_string()][..])
        );
    }
}

// ---------------------------------------------------------------- tables

#[test]
fn fields_in_table_cells_preview_and_merge() {
    let mut b = DocBuilder::new(SeqIds::new(77), setup(300.0, 200.0));
    let page = b.page(0);
    let (frame, stories) = pubrs_core::table::new_table(
        &mut b.ids,
        Rect::new(10.0, 10.0, 280.0, 60.0),
        1,
        2,
        &pubrs_core::table::CellStyle::default(),
    )
    .unwrap();
    let (tid, cell0) = (frame.id, stories[0].id);
    b.exec(Command::AddTable {
        parent: Parent::Page(page),
        index: None,
        frame,
        stories,
    })
    .unwrap();
    b.exec(Command::SetStory {
        story: cell0,
        paragraphs: vec![para(&["Hi ", "{First}"])],
    })
    .unwrap();
    let mut doc = b.finish();
    let mut m = data(&["First"], &[&["Ann"], &["Bo"]]);
    m.preview = true;
    doc.merge = Some(m);
    // The cell shows the previewed value.
    assert_eq!(drawn(&layout_of(&doc), cell0), "Hi Ann");
    // Merge to a publication: per record fresh table, cell stories, ids.
    let merged = merge_to_publication(&doc).unwrap();
    assert_eq!(merged.pages.len(), 2);
    assert!(validate::check_integrity(&merged).is_empty());
    let texts: Vec<String> = (0..2)
        .map(|i| {
            let f = merged.pages[i].frames[0];
            let t = merged.frame(f).unwrap().as_table().unwrap();
            merged.story(t.cells[0].story).unwrap().plain_text()
        })
        .collect();
    assert_eq!(texts, ["Hi Ann", "Hi Bo"]);
    let ids: std::collections::BTreeSet<_> = merged.stories.keys().collect();
    assert_eq!(ids.len(), 4, "two cells per record, all distinct");
    let _ = tid;
    // And the PDF.
    let mut ctx = ExportContext::new(test_fonts());
    let pdf = ctx.export_merged_pdf(&doc, &PdfOptions::default()).unwrap();
    assert_eq!(pdf_pages(&pdf), 2);
}
