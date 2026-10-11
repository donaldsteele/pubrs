//! Reading mail-merge data: CSV (an in-tree RFC 4180 parser) and XLSX
//! (`calamine`). The result is a [`MergeSource`] snapshot that is stored in
//! the document (ADR 0023).

use std::io::Cursor;
use std::path::Path;

use calamine::{Data, Reader, Xlsx};
use pubrs_core::merge::{MAX_CELL_BYTES, MAX_CELLS, MAX_FIELDS, MAX_RECORDS, MAX_STORED_BYTES};
use pubrs_core::{MergeKind, MergeSource};

/// Why merge data could not be read.
#[derive(Debug, thiserror::Error)]
pub enum MergeReadError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: String,
        source: std::io::Error,
    },
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Limit(String),
}

impl MergeReadError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io { source, .. } if source.kind() == std::io::ErrorKind::NotFound => {
                "file_not_found"
            }
            Self::Io { .. } => "io_error",
            Self::Invalid(_) | Self::Limit(_) => "invalid_file",
        }
    }

    pub fn hint(&self) -> &'static str {
        match self {
            Self::Io { .. } => "check the path and that the file is readable",
            Self::Invalid(_) => {
                "use a CSV (UTF-8, comma or semicolon separated) or an .xlsx file with a header row"
            }
            Self::Limit(_) => "split the data into smaller files",
        }
    }
}

/// Largest input file or text accepted (CSV or XLSX), in bytes.
pub const MAX_INPUT_BYTES: usize = 64 << 20;
/// Largest total uncompressed size of the parts of an XLSX file.
pub const MAX_XLSX_UNCOMPRESSED: u64 = 256 << 20;
/// Most parts in an XLSX file.
const MAX_XLSX_PARTS: usize = 4096;
/// A part bigger than 1 MiB that expands more than this many times is a bomb.
const MAX_XLSX_RATIO: u64 = 200;

fn too_big(what: &str, len: usize) -> MergeReadError {
    MergeReadError::Limit(format!(
        "{what} is {len} bytes; the limit is {MAX_INPUT_BYTES} bytes (64 MiB)"
    ))
}

fn too_many_cells(records: usize, fields: usize) -> MergeReadError {
    MergeReadError::Limit(format!(
        "the data has {records} records x {fields} fields; the limit is {MAX_CELLS} cells"
    ))
}

/// A header row and records, before they become a [`MergeSource`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Table {
    pub fields: Vec<String>,
    pub records: Vec<Vec<String>>,
}

fn limit_cell(row: usize) -> MergeReadError {
    MergeReadError::Limit(format!(
        "a cell in row {row} is longer than {MAX_CELL_BYTES} bytes"
    ))
}

/// Header names made usable: trimmed, a blank one becomes "Column N", and a
/// repeat gets " 2", " 3", ... after it.
fn clean_header(raw: Vec<String>) -> Result<Vec<String>, MergeReadError> {
    if raw.len() > MAX_FIELDS {
        return Err(MergeReadError::Limit(format!(
            "the data has {} columns; the limit is {MAX_FIELDS}",
            raw.len()
        )));
    }
    let mut out: Vec<String> = Vec::with_capacity(raw.len());
    for (i, h) in raw.into_iter().enumerate() {
        let h = h.trim().replace(['\u{ab}', '\u{bb}'], "");
        let base = if h.is_empty() {
            format!("Column {}", i + 1)
        } else {
            h
        };
        let mut name = base.clone();
        let mut n = 2;
        while out.contains(&name) {
            name = format!("{base} {n}");
            n += 1;
        }
        out.push(name);
    }
    Ok(out)
}

/// Whether a parsed row has no content at all (a blank line).
fn blank_row(r: &[String]) -> bool {
    r.iter().all(|c| c.trim().is_empty())
}

fn finish(rows: Vec<Vec<String>>) -> Result<Table, MergeReadError> {
    let mut it = rows.into_iter().filter(|r| !blank_row(r));
    let header = it
        .next()
        .ok_or_else(|| MergeReadError::Invalid("the data is empty (no header row)".into()))?;
    let fields = clean_header(header)?;
    let mut records: Vec<Vec<String>> = Vec::new();
    for mut r in it {
        // Before padding: a wide header with many short rows is refused.
        if (records.len() + 1).saturating_mul(fields.len()) > MAX_CELLS {
            return Err(too_many_cells(records.len() + 1, fields.len()));
        }
        if records.len() >= MAX_RECORDS {
            return Err(MergeReadError::Limit(format!(
                "the data has more than {MAX_RECORDS} records"
            )));
        }
        r.resize(fields.len(), String::new());
        records.push(r);
    }
    Ok(Table { fields, records })
}

/// The separator of a CSV: the most frequent of `,` `;` and tab in the
/// header line (outside quotes); a comma when there is none.
pub fn detect_delimiter(text: &str) -> char {
    let (mut comma, mut semi, mut tab) = (0usize, 0usize, 0usize);
    let mut quoted = false;
    for c in text.chars() {
        match c {
            '"' => quoted = !quoted,
            '\n' | '\r' if !quoted => break,
            ',' if !quoted => comma += 1,
            ';' if !quoted => semi += 1,
            '\t' if !quoted => tab += 1,
            _ => {}
        }
    }
    if semi > comma && semi >= tab {
        ';'
    } else if tab > comma && tab > semi {
        '\t'
    } else {
        ','
    }
}

/// Parse CSV text (RFC 4180): quoted fields, doubled quotes, line breaks
/// inside quotes, CRLF or LF or CR between rows. A leading byte order mark
/// is ignored and the delimiter is detected from the header line.
pub fn parse_csv(text: &str) -> Result<Table, MergeReadError> {
    if text.len() > MAX_INPUT_BYTES {
        return Err(too_big("the CSV text", text.len()));
    }
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut cells = 0usize;
    let delim = detect_delimiter(text);
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    let mut quoted = false;
    // A quote closed and the field may only continue with text after it.
    let mut was_quoted = false;
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if quoted {
            if c == '"' {
                if it.peek() == Some(&'"') {
                    it.next();
                    cell.push('"');
                } else {
                    quoted = false;
                }
            } else {
                cell.push(c);
            }
        } else if c == '"' && cell.is_empty() && !was_quoted {
            quoted = true;
            was_quoted = true;
        } else if c == delim {
            row.push(std::mem::take(&mut cell));
            was_quoted = false;
            if row.len() > MAX_FIELDS {
                return Err(MergeReadError::Limit(format!(
                    "row {} has more than {MAX_FIELDS} cells",
                    rows.len() + 1
                )));
            }
            cells += 1;
        } else if c == '\n' || c == '\r' {
            if c == '\r' && it.peek() == Some(&'\n') {
                it.next();
            }
            row.push(std::mem::take(&mut cell));
            was_quoted = false;
            cells += 1;
            rows.push(std::mem::take(&mut row));
            if rows.len() > MAX_RECORDS + 1 {
                return Err(MergeReadError::Limit(format!(
                    "the data has more than {MAX_RECORDS} records"
                )));
            }
        } else {
            cell.push(c);
        }
        if cell.len() > MAX_CELL_BYTES {
            return Err(limit_cell(rows.len() + 1));
        }
        if cells > MAX_CELLS + MAX_FIELDS {
            return Err(too_many_cells(rows.len(), MAX_FIELDS));
        }
    }
    if quoted {
        return Err(MergeReadError::Invalid(
            "a quoted field is never closed (a \" without its partner)".into(),
        ));
    }
    if !cell.is_empty() || !row.is_empty() || was_quoted {
        row.push(cell);
        rows.push(row);
    }
    finish(rows)
}

/// CSV from bytes: UTF-8 with or without a byte order mark.
pub fn parse_csv_bytes(bytes: &[u8]) -> Result<Table, MergeReadError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(too_big("the file", bytes.len()));
    }
    let text = std::str::from_utf8(bytes).map_err(|e| {
        MergeReadError::Invalid(format!(
            "the file is not UTF-8 (invalid byte at {}); save it as CSV UTF-8",
            e.valid_up_to()
        ))
    })?;
    parse_csv(text)
}

/// ISO text of an Excel date/time: `YYYY-MM-DD`, with `THH:MM:SS` when the
/// cell has a time of day.
fn iso_datetime(d: &calamine::ExcelDateTime) -> String {
    if d.is_duration() {
        let secs = (d.as_f64() * 86_400.0).round() as i64;
        return format!("{:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60);
    }
    let (y, mo, da, h, mi, s, _) = d.to_ymd_hms_milli();
    if (h, mi, s) == (0, 0, 0) {
        format!("{y:04}-{mo:02}-{da:02}")
    } else {
        format!("{y:04}-{mo:02}-{da:02}T{h:02}:{mi:02}:{s:02}")
    }
}

/// A cell as the text a person sees: whole numbers without ".0", dates as
/// ISO, errors and empty cells as "".
fn cell_text(c: &Data) -> String {
    match c {
        Data::String(s) | Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Int(i) => i.to_string(),
        Data::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e15 {
                format!("{}", *f as i64)
            } else {
                format!("{f}")
            }
        }
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Data::DateTime(d) => iso_datetime(d),
        Data::Error(_) | Data::Empty => String::new(),
    }
}

/// Most bytes of inflated shared strings accepted.
const MAX_SST_BYTES: u64 = 64 << 20;
/// Most `<si>` strings accepted.
const MAX_SST_STRINGS: usize = MAX_CELLS;

fn unreadable(e: &dyn std::fmt::Display) -> MergeReadError {
    MergeReadError::Invalid(format!("not a readable .xlsx workbook: {e}"))
}

/// Make an XLSX safe to hand to calamine, returning a new zip.
///
/// Every part is really inflated (declared sizes are not trusted), counting
/// the bytes that come out against 256 MiB in total and a 200x ratio per part.
/// Part names are compared the way calamine does (`\` read as `/`, ASCII case
/// ignored); two parts with the same name are refused. Any part named
/// `...sharedStrings.xml` (whatever its folder) is parsed with quick-xml
/// (at most 64 MiB, at most 5,000,000 `<si>`) and written again with its
/// `<sst>` attributes replaced by `count` and `uniqueCount` equal to the real
/// number of strings, so calamine never reserves memory for a number the file
/// made up. Every other part is copied unchanged.
fn sanitize_xlsx(bytes: &[u8]) -> Result<Vec<u8>, MergeReadError> {
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(too_big("the workbook", bytes.len()));
    }
    let mut z = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| unreadable(&e))?;
    if z.len() > MAX_XLSX_PARTS {
        return Err(MergeReadError::Limit(format!(
            "the workbook has {} parts; the limit is {MAX_XLSX_PARTS}",
            z.len()
        )));
    }
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    let mut total = 0u64;
    for i in 0..z.len() {
        let mut f = z.by_index(i).map_err(|e| unreadable(&e))?;
        let name = f.name().map_err(|e| unreadable(&e))?.into_owned();
        let norm = name.replace('\\', "/").to_ascii_lowercase();
        if !seen.insert(norm.clone()) {
            return Err(MergeReadError::Invalid(format!(
                "the workbook has two parts named {name:?} (ignoring case): refused"
            )));
        }
        let packed = f.compressed_size().max(1);
        let is_sst = norm.ends_with("sharedstrings.xml");
        let room = if is_sst {
            MAX_SST_BYTES.min(MAX_XLSX_UNCOMPRESSED.saturating_sub(total))
        } else {
            MAX_XLSX_UNCOMPRESSED.saturating_sub(total)
        };
        let mut data: Vec<u8> = Vec::new();
        let mut limited = std::io::Read::take(&mut f, room.saturating_add(1));
        std::io::Read::read_to_end(&mut limited, &mut data).map_err(|e| unreadable(&e))?;
        let real = data.len() as u64;
        total = total.saturating_add(real);
        if is_sst && real > MAX_SST_BYTES {
            return Err(MergeReadError::Limit(
                "the shared strings inflate to more than 64 MiB".into(),
            ));
        }
        if total > MAX_XLSX_UNCOMPRESSED {
            return Err(MergeReadError::Limit(format!(
                "the workbook expands to more than {MAX_XLSX_UNCOMPRESSED} bytes (256 MiB)"
            )));
        }
        if real > (1 << 20) && real / packed > MAX_XLSX_RATIO {
            return Err(MergeReadError::Limit(format!(
                "the part {name:?} has an inflated size of {real} bytes, {}x its packed size: refused as a compression bomb",
                real / packed
            )));
        }
        if is_sst {
            data = rewrite_sst(&data)?;
        }
        out.start_file(&name, opts).map_err(|e| unreadable(&e))?;
        std::io::Write::write_all(&mut out, &data).map_err(|e| unreadable(&e))?;
    }
    Ok(out.finish().map_err(|e| unreadable(&e))?.into_inner())
}

/// The `<sst>` start tag with only its `xmlns*` attributes and the real count.
fn truthful_sst(
    e: &quick_xml::events::BytesStart<'_>,
    count: usize,
) -> quick_xml::events::BytesStart<'static> {
    let mut s =
        quick_xml::events::BytesStart::new(String::from_utf8_lossy(e.name().as_ref()).into_owned());
    for a in e.attributes().with_checks(false).flatten() {
        if a.key.as_ref().starts_with(b"xmlns") {
            s.push_attribute(a);
        }
    }
    s.push_attribute(("count", count.to_string().as_str()));
    s.push_attribute(("uniqueCount", count.to_string().as_str()));
    s
}

/// The shared strings part written again with truthful counts (see
/// [`sanitize_xlsx`]). Elements are matched by local name, so prefixes do not
/// matter; the `<sst>` keeps only its `xmlns` declarations.
fn rewrite_sst(xml: &[u8]) -> Result<Vec<u8>, MergeReadError> {
    use quick_xml::events::Event;
    let xml_err = |e: &dyn std::fmt::Display| {
        MergeReadError::Invalid(format!("the shared strings are not valid XML: {e}"))
    };
    let is = |name: &[u8], want: &[u8]| name.rsplit(|b| *b == b':').next() == Some(want);
    // Pass 1: count the strings.
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut buf = Vec::new();
    let mut count = 0usize;
    let mut has_sst = false;
    loop {
        match reader.read_event_into(&mut buf).map_err(|e| xml_err(&e))? {
            Event::Start(e) | Event::Empty(e) => {
                let name = e.name();
                if is(name.as_ref(), b"si") {
                    count += 1;
                    if count > MAX_SST_STRINGS {
                        return Err(MergeReadError::Limit(format!(
                            "the shared strings hold more than {MAX_SST_STRINGS} strings"
                        )));
                    }
                } else if is(name.as_ref(), b"sst") {
                    has_sst = true;
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    if !has_sst {
        return Err(MergeReadError::Invalid(
            "the shared strings part has no <sst> element".into(),
        ));
    }
    // Pass 2: copy everything, replacing the <sst> attributes.
    let mut reader = quick_xml::Reader::from_reader(xml);
    let mut writer = quick_xml::Writer::new(Vec::with_capacity(xml.len() + 64));
    let mut done = false;
    loop {
        let ev = reader.read_event_into(&mut buf).map_err(|e| xml_err(&e))?;
        let ev = match ev {
            Event::Eof => break,
            Event::Start(e) if !done && is(e.name().as_ref(), b"sst") => {
                done = true;
                Event::Start(truthful_sst(&e, count))
            }
            // calamine expands empty elements, so `<sst uniqueCount=".."/>` is
            // a start tag to it: rewrite it the same way.
            Event::Empty(e) if !done && is(e.name().as_ref(), b"sst") => {
                done = true;
                Event::Empty(truthful_sst(&e, count))
            }
            other => other.into_owned(),
        };
        writer.write_event(ev).map_err(|e| xml_err(&e))?;
        buf.clear();
    }
    Ok(writer.into_inner())
}

/// Sheet names of an XLSX workbook, in order.
pub fn xlsx_sheet_names(bytes: &[u8]) -> Result<Vec<String>, MergeReadError> {
    let clean = sanitize_xlsx(bytes)?;
    let wb: Xlsx<_> = calamine::open_workbook_from_rs(Cursor::new(clean))
        .map_err(|e| MergeReadError::Invalid(format!("not a readable .xlsx workbook: {e}")))?;
    Ok(wb.sheet_names())
}

/// Read an XLSX workbook: `sheet` by name (the first sheet when `None`).
/// Returns the table and the name of the sheet used.
///
/// The sheet is read cell by cell (calamine's streaming reader) and rows are
/// built sparsely, so a file whose cells sit at A1 and XFD1048576 costs two
/// cells, not a 17-billion-cell grid. The first non-empty row is the header;
/// its first non-empty cell is the first column. Reading stops with an error
/// at 100,000 records, 512 columns, 5,000,000 cells or a 1 MB cell.
pub fn parse_xlsx(bytes: &[u8], sheet: Option<&str>) -> Result<(Table, String), MergeReadError> {
    let clean = sanitize_xlsx(bytes)?;
    let mut wb: Xlsx<_> = calamine::open_workbook_from_rs(Cursor::new(clean))
        .map_err(|e| MergeReadError::Invalid(format!("not a readable .xlsx workbook: {e}")))?;
    let names = wb.sheet_names();
    let name = match sheet {
        None => names
            .first()
            .cloned()
            .ok_or_else(|| MergeReadError::Invalid("the workbook has no sheets".into()))?,
        Some(s) => names
            .iter()
            .find(|n| n.as_str() == s)
            .or_else(|| names.iter().find(|n| n.eq_ignore_ascii_case(s)))
            .cloned()
            .ok_or_else(|| {
                MergeReadError::Invalid(format!(
                    "the workbook has no sheet {s:?}; its sheets are: {}",
                    names.join(", ")
                ))
            })?,
    };
    let bad = |e: calamine::XlsxError| {
        MergeReadError::Invalid(format!("cannot read sheet {name:?}: {e}"))
    };
    let mut reader = wb.worksheet_cells_reader(&name).map_err(bad)?;
    let mut head_row: Option<u32> = None;
    let mut col0 = 0u32;
    let mut head: Vec<(u32, String)> = Vec::new();
    let mut width: Option<usize> = None;
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut cur_row: Option<u32> = None;
    let mut cur: Vec<String> = Vec::new();
    let mut cells = 0usize;
    let mut stored = 0usize;
    while let Some(cell) = reader.next_cell().map_err(bad)? {
        let (r, c) = cell.get_position();
        if let calamine::DataRef::String(s) = cell.get_value() {
            if s.len() > MAX_CELL_BYTES {
                return Err(limit_cell(r as usize + 1));
            }
        } else if let calamine::DataRef::SharedString(s) = cell.get_value()
            && s.len() > MAX_CELL_BYTES
        {
            return Err(limit_cell(r as usize + 1));
        }
        let text = cell_text(&Data::from(cell.get_value().clone()));
        if text.is_empty() {
            continue;
        }
        if text.len() > MAX_CELL_BYTES {
            return Err(limit_cell(r as usize + 1));
        }
        stored += text.len();
        if stored > MAX_STORED_BYTES {
            return Err(MergeReadError::Limit(format!(
                "the sheet holds more than {MAX_STORED_BYTES} bytes of text (64 MiB)"
            )));
        }
        cells += 1;
        if cells > MAX_CELLS {
            return Err(too_many_cells(rows.len(), MAX_FIELDS));
        }
        let h = *head_row.get_or_insert_with(|| {
            col0 = c;
            r
        });
        if c.saturating_sub(col0) as usize >= MAX_FIELDS {
            return Err(MergeReadError::Limit(format!(
                "the sheet has a cell in column {}; the limit is {MAX_FIELDS} columns",
                c + 1
            )));
        }
        let (Some(dr), Some(dc)) = (r.checked_sub(h), c.checked_sub(col0)) else {
            return Err(MergeReadError::Invalid(
                "the sheet's cells are out of order (a cell above or left of the header)".into(),
            ));
        };
        let _ = dc;
        if r == h {
            head.push((c, text));
            continue;
        }
        if dr as usize > MAX_RECORDS {
            return Err(MergeReadError::Limit(format!(
                "the sheet has a cell in row {}; the limit is {MAX_RECORDS} records below the header",
                r + 1
            )));
        }
        let w = *width.get_or_insert_with(|| {
            let w = head
                .iter()
                .map(|(c, _)| (c - col0) as usize + 1)
                .max()
                .unwrap_or(1);
            let mut hv = vec![String::new(); w];
            for (c, t) in head.drain(..) {
                hv[(c - col0) as usize] = t;
            }
            rows.push(hv);
            w
        });
        if cur_row != Some(r) {
            if cur_row.is_some() {
                rows.push(std::mem::take(&mut cur));
            }
            if rows.len() > MAX_RECORDS {
                return Err(MergeReadError::Limit(format!(
                    "the sheet has more than {MAX_RECORDS} records"
                )));
            }
            if rows.len().saturating_mul(w) > MAX_CELLS {
                return Err(too_many_cells(rows.len(), w));
            }
            cur_row = Some(r);
            cur = vec![String::new(); w];
        }
        if c >= col0
            && let Some(slot) = cur.get_mut((c - col0) as usize)
        {
            *slot = text;
        }
    }
    if width.is_none() && !head.is_empty() {
        let w = head
            .iter()
            .map(|(c, _)| (c - col0) as usize + 1)
            .max()
            .unwrap_or(1);
        let mut hv = vec![String::new(); w];
        for (c, t) in head.drain(..) {
            hv[(c - col0) as usize] = t;
        }
        rows.push(hv);
    }
    if cur_row.is_some() {
        rows.push(cur);
    }
    Ok((finish(rows)?, name))
}
fn into_source(name: &str, kind: MergeKind, sheet: Option<String>, t: Table) -> MergeSource {
    MergeSource::new(name, kind, sheet, t.fields, t.records)
}

/// A source from CSV text passed inline.
pub fn from_inline_csv(text: &str) -> Result<MergeSource, MergeReadError> {
    Ok(into_source(
        "inline",
        MergeKind::Csv,
        None,
        parse_csv(text)?,
    ))
}

/// A source from file bytes; `file_name` decides the kind by its extension
/// (`.xlsx`/`.xlsm` are workbooks, anything else CSV).
pub fn from_bytes(
    file_name: &str,
    bytes: &[u8],
    sheet: Option<&str>,
) -> Result<MergeSource, MergeReadError> {
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".xlsx") || lower.ends_with(".xlsm") {
        let (t, used) = parse_xlsx(bytes, sheet)?;
        Ok(into_source(file_name, MergeKind::Xlsx, Some(used), t))
    } else {
        if sheet.is_some() {
            return Err(MergeReadError::Invalid(
                "a sheet only applies to .xlsx files".into(),
            ));
        }
        Ok(into_source(
            file_name,
            MergeKind::Csv,
            None,
            parse_csv_bytes(bytes)?,
        ))
    }
}

/// Read a file, refusing one over [`MAX_INPUT_BYTES`] before reading it.
fn read_capped(path: &Path) -> Result<Vec<u8>, MergeReadError> {
    let io = |source| MergeReadError::Io {
        path: path.display().to_string(),
        source,
    };
    let len = std::fs::metadata(path).map_err(io)?.len();
    if len > MAX_INPUT_BYTES as u64 {
        return Err(too_big("the file", len as usize));
    }
    std::fs::read(path).map_err(io)
}

/// Read a CSV or XLSX file into a [`MergeSource`].
pub fn load_file(path: &Path, sheet: Option<&str>) -> Result<MergeSource, MergeReadError> {
    let bytes = read_capped(path)?;
    let name = path
        .file_name()
        .map_or_else(|| "data".into(), |n| n.to_string_lossy().into_owned());
    from_bytes(&name, &bytes, sheet)
}

/// Sheet names of an `.xlsx` file; empty for any other kind of file.
pub fn sheet_names_of(path: &Path) -> Result<Vec<String>, MergeReadError> {
    let lower = path.to_string_lossy().to_ascii_lowercase();
    if !(lower.ends_with(".xlsx") || lower.ends_with(".xlsm")) {
        return Ok(Vec::new());
    }
    let bytes = read_capped(path)?;
    xlsx_sheet_names(&bytes)
}

/// `new` with the preview state of `old` kept (clamped to the new data), for
/// "Refresh data".
pub fn keep_preview(mut new: MergeSource, old: Option<&MergeSource>) -> MergeSource {
    if let Some(o) = old {
        new.preview = o.preview;
        new.preview_index = o.preview_index.min(new.records.len().saturating_sub(1));
    }
    new
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_and_newlines() {
        let t = parse_csv("a,b\r\n\"x, \"\"y\"\"\",\"l1\nl2\"\r\n").unwrap();
        assert_eq!(t.fields, ["a", "b"]);
        assert_eq!(t.records, [["x, \"y\"", "l1\nl2"]]);
    }

    #[test]
    fn semicolons_and_bom() {
        let t = parse_csv("\u{feff}Name;City\nAnn;Oslo\nBo;Rome").unwrap();
        assert_eq!(t.fields, ["Name", "City"]);
        assert_eq!(t.records.len(), 2);
    }

    #[test]
    fn short_rows_are_padded() {
        let t = parse_csv("a,b,c\n1\n1,2,3,4\n").unwrap();
        assert_eq!(t.records[0], ["1", "", ""]);
        assert_eq!(t.records[1], ["1", "2", "3"]);
    }

    #[test]
    fn unclosed_quote_is_an_error() {
        assert!(parse_csv("a,b\n\"x,y\n").is_err());
    }

    #[test]
    fn header_is_cleaned() {
        let t = parse_csv("a,,a\n1,2,3").unwrap();
        assert_eq!(t.fields, ["a", "Column 2", "a 2"]);
    }
}
