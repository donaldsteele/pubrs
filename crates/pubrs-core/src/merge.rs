//! Mail merge: what a field shows, the one-space collapse rule, and the
//! realised documents merge output is made of (ADR 0023).
//!
//! A merge field is one U+FFFC character in a run whose `field` names a
//! column of [`MergeSource::fields`]. Nothing here reads files; see
//! `pubrs-io::merge_source` for CSV and XLSX.

use std::collections::BTreeSet;
use std::sync::Arc;

use uuid::Uuid;

use crate::id::{FrameId, PageId, StoryId};
use crate::model::{Document, FrameContent, MergeSource, Page, Paragraph, Story};

/// Most records a merge source may hold.
pub const MAX_RECORDS: usize = 100_000;
/// Most bytes in one cell.
pub const MAX_CELL_BYTES: usize = 1 << 20;
/// Most fields (columns) a merge source may have.
pub const MAX_FIELDS: usize = 512;
/// Most cells (records times fields) a merge source may have. Checked before
/// any row is padded, so 512 fields times 100,000 records is refused.
pub const MAX_CELLS: usize = 5_000_000;
/// Most pages a merge may produce (records times pages of the design).
pub const MAX_OUTPUT_PAGES: usize = 20_000;
/// Most bytes of text (cells and field names) a merge source may hold.
pub const MAX_STORED_BYTES: usize = 64 << 20;

/// Check the shape and limits of a merge source (also run when a file is
/// loaded). Returns what is wrong.
pub fn check_source(m: &MergeSource) -> Result<(), String> {
    if m.fields.is_empty() || m.fields.len() > MAX_FIELDS {
        return Err(format!(
            "merge data needs 1 to {MAX_FIELDS} fields (columns)"
        ));
    }
    if m.fields.iter().any(String::is_empty) {
        return Err("a merge field name is empty".into());
    }
    let mut names: Vec<&String> = m.fields.iter().collect();
    names.sort();
    if names.windows(2).any(|w| w[0] == w[1]) {
        return Err("merge field names must be unique".into());
    }
    if m.records.len() > MAX_RECORDS {
        return Err(format!(
            "merge data has {} records; the limit is {MAX_RECORDS}",
            m.records.len()
        ));
    }
    if m.records.len().saturating_mul(m.fields.len()) > MAX_CELLS {
        return Err(format!(
            "merge data has {} records x {} fields; the limit is {MAX_CELLS} cells",
            m.records.len(),
            m.fields.len()
        ));
    }
    let stored: usize = m.fields.iter().map(String::len).sum::<usize>()
        + m.records
            .iter()
            .flatten()
            .map(String::len)
            .fold(0usize, usize::saturating_add);
    if stored > MAX_STORED_BYTES {
        return Err(format!(
            "merge data holds {stored} bytes of text; the limit is {MAX_STORED_BYTES} (64 MiB)"
        ));
    }
    if m.records
        .iter()
        .any(|r| r.len() != m.fields.len() || r.iter().any(|c| c.len() > MAX_CELL_BYTES))
    {
        return Err(format!(
            "every record needs exactly {} cells of at most {MAX_CELL_BYTES} bytes",
            m.fields.len()
        ));
    }
    Ok(())
}

/// The text a field with no value shows: «Name».
pub fn placeholder(name: &str) -> String {
    format!("\u{ab}{name}\u{bb}")
}

/// A value made safe to flow inline: line breaks, tabs and other control
/// characters become spaces, object-replacement characters are dropped, and
/// the ends are trimmed.
pub fn sanitize_value(raw: &str) -> String {
    let mapped: String = raw
        .chars()
        .filter(|c| *c != crate::model::FIELD_CHAR)
        .map(|c| {
            if c.is_control() || matches!(c, '\u{2028}' | '\u{2029}') {
                ' '
            } else {
                c
            }
        })
        .collect();
    mapped.trim().to_string()
}

impl MergeSource {
    /// A source with equal-length rows: short rows are padded with empty
    /// cells and long rows cut to the header.
    pub fn new(
        source_name: impl Into<String>,
        kind: crate::model::MergeKind,
        sheet: Option<String>,
        fields: Vec<String>,
        mut records: Vec<Vec<String>>,
    ) -> Self {
        for r in &mut records {
            r.resize(fields.len(), String::new());
        }
        Self {
            source_name: source_name.into(),
            kind,
            sheet,
            fields,
            records: Arc::new(records),
            preview_index: 0,
            preview: false,
        }
    }

    pub fn record_count(&self) -> usize {
        self.records.len()
    }

    pub fn field_index(&self, name: &str) -> Option<usize> {
        self.fields.iter().position(|f| f == name)
    }

    /// The previewed record's index, clamped to the data.
    pub fn current_index(&self) -> usize {
        self.preview_index.min(self.records.len().saturating_sub(1))
    }

    /// The value of field `name` in record `index`; `None` when there is no
    /// such field or record.
    pub fn value_in(&self, index: usize, name: &str) -> Option<&str> {
        let col = self.field_index(name)?;
        self.records
            .get(index)
            .and_then(|r| r.get(col))
            .map(String::as_str)
    }

    /// The value field `name` shows on screen: the previewed record's value,
    /// or `None` while preview is off, there are no records, or the field is
    /// not in the data (the «Name» placeholder shows instead).
    pub fn shown_value(&self, name: &str) -> Option<&str> {
        if !self.preview || self.records.is_empty() {
            return None;
        }
        self.value_in(self.current_index(), name)
    }
}

/// The same source with record `index` previewed.
pub fn previewing(src: &MergeSource, index: usize) -> MergeSource {
    let mut s = src.clone();
    s.preview_index = index;
    s.preview = true;
    s
}

/// One field of a paragraph and what replaces it when shown or realised.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldSub {
    /// Character range replaced, within the paragraph: the field itself,
    /// widened by one space when the value is empty (the collapse rule).
    pub start: usize,
    pub end: usize,
    /// The field's own character index (inside `start..end`).
    pub field_at: usize,
    pub name: String,
    /// The text shown in its place: the sanitised value, or «Name».
    pub display: String,
}

/// The substitutions for the fields of `para`, in order.
///
/// The collapse rule: a field whose value is empty takes one adjacent space
/// with it, preferring the space after the field and otherwise the one
/// before it. A space is never taken twice. A field without a value (no
/// preview, no data, or a name not in the data) shows «Name» and takes
/// nothing.
pub fn plan_paragraph(merge: Option<&MergeSource>, para: &Paragraph) -> Vec<FieldSub> {
    if !para.runs.iter().any(|r| r.field.is_some()) {
        return Vec::new();
    }
    let chars: Vec<char> = para.runs.iter().flat_map(|r| r.text.chars()).collect();
    let mut out: Vec<FieldSub> = Vec::new();
    let mut at = 0usize;
    for r in &para.runs {
        let len = r.text.chars().count();
        if let Some(name) = &r.field
            && len == 1
        {
            let value = merge.and_then(|m| m.shown_value(name)).map(sanitize_value);
            let (mut start, mut end, display) = match value {
                Some(v) => (at, at + 1, v),
                None => (at, at + 1, placeholder(name)),
            };
            if display.is_empty() {
                let taken = out.last().map_or(0, |s| s.end);
                if chars.get(end) == Some(&' ') {
                    end += 1;
                } else if start > taken && chars.get(start - 1) == Some(&' ') {
                    start -= 1;
                }
            }
            out.push(FieldSub {
                start,
                end,
                field_at: at,
                name: name.clone(),
                display,
            });
        }
        at += len;
    }
    out
}

/// Replace every field of `story` by what it shows under `merge`, keeping
/// the formatting of the field's run. The story then holds plain text.
pub fn realise_story(merge: Option<&MergeSource>, story: &mut Story) {
    for pi in (0..story.paragraphs.len()).rev() {
        let subs = plan_paragraph(merge, &story.paragraphs[pi]);
        if subs.is_empty() {
            continue;
        }
        let base = story.paragraph_start(pi);
        for s in subs.iter().rev() {
            if s.display.is_empty() {
                story.delete_range(base + s.start, base + s.end);
            } else {
                story.replace_range(base + s.field_at, base + s.field_at + 1, &s.display);
            }
        }
    }
}

/// Names of the fields used anywhere in `doc`'s stories, sorted.
pub fn fields_used(doc: &Document) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for s in doc.stories.values() {
        for p in &s.paragraphs {
            for r in &p.runs {
                if let Some(n) = &r.field {
                    out.insert(n.clone());
                }
            }
        }
    }
    out
}

/// Why a merge cannot run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MergeError {
    #[error("the document has no merge data")]
    NoData,
    #[error("the merge data has no records")]
    NoRecords,
    #[error("the document uses no merge fields")]
    NoFields,
    #[error("the merge would make {0} pages; the limit is {MAX_OUTPUT_PAGES}")]
    TooManyPages(usize),
}

impl MergeError {
    /// How to fix it.
    pub fn hint(&self) -> &'static str {
        match self {
            Self::NoData => "load data with merge_set_source (a CSV or XLSX file) first",
            Self::NoRecords => "the data needs a header row and at least one record row",
            Self::NoFields => "insert fields with merge_insert_field first",
            Self::TooManyPages(_) => "merge fewer records, or a design with fewer pages",
        }
    }
}

/// Check that `doc` can be merged and return its data.
pub fn merge_data(doc: &Document) -> Result<&MergeSource, MergeError> {
    let m = doc.merge.as_ref().ok_or(MergeError::NoData)?;
    if m.records.is_empty() {
        return Err(MergeError::NoRecords);
    }
    if fields_used(doc).is_empty() {
        return Err(MergeError::NoFields);
    }
    let pages = m.records.len().saturating_mul(doc.pages.len());
    if pages > MAX_OUTPUT_PAGES {
        return Err(MergeError::TooManyPages(pages));
    }
    Ok(m)
}

fn remap_uuid(id: Uuid, record: usize) -> Uuid {
    if record == 0 {
        return id;
    }
    let mut b = *id.as_bytes();
    let k = (record as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let lo = u64::from_be_bytes([b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]]) ^ k;
    b[8..].copy_from_slice(&lo.to_be_bytes());
    Uuid::from_bytes(b)
}

/// A new publication with the pages of `doc` once per record, every field
/// replaced by that record's text. Record 0 keeps the original ids; later
/// records get derived ids (deterministic, so output is reproducible).
/// Master pages are shared: fields on a master stay as «Name».
pub fn merge_to_publication(doc: &Document) -> Result<Document, MergeError> {
    let data = merge_data(doc)?.clone();
    let mut out = doc.clone();
    out.merge = None;
    let mut pages: Vec<Page> = Vec::with_capacity(doc.pages.len() * data.record_count());
    // Frames and stories used by the pages, in the original document.
    let mut page_frames: Vec<FrameId> = Vec::new();
    for p in doc.pages.iter() {
        collect_frames(doc, &p.frames, &mut page_frames);
    }
    let mut page_stories: BTreeSet<StoryId> = BTreeSet::new();
    for f in &page_frames {
        match doc.frame(*f).map(|f| &f.content) {
            Some(FrameContent::Text(t)) => {
                page_stories.insert(t.story);
            }
            // A table's cell text is one story per cell.
            Some(FrameContent::Table(t)) => page_stories.extend(t.cells.iter().map(|c| c.story)),
            _ => {}
        }
    }
    // Drop the originals; each record re-adds its own copy.
    for f in &page_frames {
        out.frames.remove(f);
    }
    for s in &page_stories {
        out.stories.remove(s);
    }
    for rec in 0..data.record_count() {
        let src = previewing(&data, rec);
        let fid = |id: FrameId| FrameId(remap_uuid(id.0, rec));
        let sid = |id: StoryId| StoryId(remap_uuid(id.0, rec));
        for f in &page_frames {
            let Some(orig) = doc.frame(*f) else { continue };
            let mut nf = orig.clone();
            nf.id = fid(*f);
            match &mut nf.content {
                FrameContent::Text(t) => t.story = sid(t.story),
                FrameContent::Table(t) => {
                    for c in &mut t.cells {
                        c.story = sid(c.story);
                    }
                }
                FrameContent::Group(g) => {
                    g.children = g.children.iter().map(|c| fid(*c)).collect();
                }
                _ => {}
            }
            out.frames.insert(nf.id, Arc::new(nf));
        }
        for s in &page_stories {
            let Some(orig) = doc.story(*s) else { continue };
            let mut ns = orig.clone();
            ns.id = sid(*s);
            ns.frames = ns.frames.iter().map(|f| fid(*f)).collect();
            realise_story(Some(&src), &mut ns);
            out.stories.insert(ns.id, Arc::new(ns));
        }
        for p in doc.pages.iter() {
            let mut np = p.clone();
            np.id = PageId(remap_uuid(p.id.0, rec));
            np.frames = p.frames.iter().map(|f| fid(*f)).collect();
            pages.push(np);
        }
    }
    out.pages = Arc::new(pages);
    Ok(out)
}

/// `roots` and all their group descendants.
fn collect_frames(doc: &Document, roots: &[FrameId], out: &mut Vec<FrameId>) {
    for id in roots {
        out.push(*id);
        if let Some(FrameContent::Group(g)) = doc.frame(*id).map(|f| &f.content) {
            collect_frames(doc, &g.children, out);
        }
    }
}
