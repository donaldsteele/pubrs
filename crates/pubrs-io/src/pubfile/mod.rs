//! Import of Microsoft Publisher (`.pub`) files.
//!
//! Part 1 (this module): the OLE container, page and text-frame geometry, story
//! text and its character and paragraph formatting. Pictures, shapes, colour
//! schemes and tables follow in part 2.
//!
//! The format is undocumented. The field layouts here were learned from the
//! corpus under `tests/corpus/pub/` and from libmspub and Apache POI HPBF read
//! as format references; no code is taken from either (ADR 0017).
//!
//! [`import`] never panics: a panic anywhere inside is caught and returned as
//! [`IoError::Format`].

pub mod container;
pub mod records;
pub mod report;
mod text;

use std::panic::{AssertUnwindSafe, catch_unwind};

use pubrs_core::model::Document;

use crate::IoError;

pub use records::Version;
pub use report::{Dropped, ImportReport};

/// Import a `.pub` file. Anything the importer does not understand is listed in
/// the returned [`ImportReport`] rather than failing the import.
pub fn import(bytes: &[u8]) -> Result<(Document, ImportReport), IoError> {
    match catch_unwind(AssertUnwindSafe(|| import_inner(bytes))) {
        Ok(r) => r,
        Err(_) => Err(IoError::Format(
            "pub: internal error while reading the file".into(),
        )),
    }
}

/// [`import`] without the panic boundary, so tests can see a panic rather than
/// have it converted into an error.
#[doc(hidden)]
pub fn import_unguarded(bytes: &[u8]) -> Result<(Document, ImportReport), IoError> {
    import_inner(bytes)
}

fn import_inner(bytes: &[u8]) -> Result<(Document, ImportReport), IoError> {
    let mut c = container::Container::open(bytes)?;
    let contents = c
        .stream("/Contents")
        .ok_or_else(|| IoError::Format("pub: no Contents stream; not a Publisher file".into()))?;
    let quill = c.stream("/Quill/QuillSub/CONTENTS");
    let escher = c.stream("/Escher/EscherStm");
    import_streams_unguarded(&contents, quill.as_deref(), escher.as_deref())
}

/// Import from the three streams directly (no container, no panic boundary).
/// Used by tests to fuzz the record parsers with mutated streams.
#[doc(hidden)]
pub fn import_streams_unguarded(
    contents: &[u8],
    quill: Option<&[u8]>,
    escher: Option<&[u8]>,
) -> Result<(Document, ImportReport), IoError> {
    let version = records::detect(contents, quill.is_some())?;
    if version == Version::Pub2002 && quill.is_none() {
        return Err(IoError::Format(
            "pub: Publisher 2002+ file without a Quill/QuillSub/CONTENTS stream".into(),
        ));
    }
    let escher = if version == Version::Pub2002 {
        escher
    } else {
        None
    };
    let mut rep = ImportReport::default();
    tracing::debug!(version = version.label(), "pub import");
    let data = records::parse(version, contents, quill, escher, &mut rep);
    let doc = text::build(&data, &mut rep)?;
    Ok((doc, rep))
}

/// The version of a `.pub` file, without importing it.
pub fn detect_version(bytes: &[u8]) -> Result<Version, IoError> {
    catch_unwind(AssertUnwindSafe(|| detect_version_inner(bytes))).unwrap_or_else(|_| {
        Err(IoError::Format(
            "pub: internal error while reading the file".into(),
        ))
    })
}

fn detect_version_inner(bytes: &[u8]) -> Result<Version, IoError> {
    let mut c = container::Container::open(bytes)?;
    let contents = c
        .stream("/Contents")
        .ok_or_else(|| IoError::Format("pub: no Contents stream; not a Publisher file".into()))?;
    records::detect(&contents, c.has_stream("/Quill/QuillSub/CONTENTS"))
}

/// Stories of a parsed file as plain text, one string per story, for tests and tools.
pub fn story_texts(doc: &Document) -> Vec<String> {
    doc.stories
        .values()
        .map(|s| {
            s.paragraphs
                .iter()
                .map(|p| p.text())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect()
}
