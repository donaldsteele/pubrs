//! Reading and writing `.pubrs` archives (see FILE_FORMAT.md).

use std::io::{Cursor, Read, Write};
use std::path::Path;
use std::sync::Arc;

use pubrs_core::{Document, validate::check_integrity};
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use crate::error::IoError;
use crate::migrate::migrate;

/// MIME type stored in the archive's first entry.
pub const MIMETYPE: &str = "application/vnd.pubrs+zip";
const DOCUMENT_JSON: &str = "document.json";
const THUMBNAIL: &str = "thumbnail.png";
/// Refuse to inflate entries larger than this (defends against zip bombs).
const MAX_ENTRY_BYTES: u64 = 512 * 1024 * 1024;

/// Archive entry name of an asset. Always uses `/`.
pub fn asset_entry_name(doc: &Document, id: pubrs_core::AssetId) -> Option<String> {
    doc.assets
        .get(&id)
        .map(|a| format!("assets/{}.{}", id, a.format.extension()))
}

/// Serialize the document model (without asset bytes) as pretty JSON.
pub fn document_json(doc: &Document) -> Result<String, IoError> {
    Ok(serde_json::to_string_pretty(doc)?)
}

/// Parse document JSON, migrating older schema versions. Asset bytes are not
/// loaded; integrity is not checked.
pub fn parse_document_json(text: &str) -> Result<Document, IoError> {
    let value: serde_json::Value = serde_json::from_str(text)?;
    let value = migrate(value)?;
    Ok(serde_json::from_value(value)?)
}

/// Write a document to `.pubrs` bytes. Output is deterministic: the same
/// document always produces the same bytes.
pub fn write_bytes(doc: &Document, thumbnail: Option<&[u8]>) -> Result<Vec<u8>, IoError> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    zip.start_file("mimetype", stored)?;
    zip.write_all(MIMETYPE.as_bytes())?;
    zip.start_file(DOCUMENT_JSON, deflated)?;
    zip.write_all(document_json(doc)?.as_bytes())?;
    for id in doc.assets.keys() {
        let data = doc
            .blobs
            .get(id)
            .ok_or_else(|| IoError::Integrity(vec![format!("asset {id} has no data")]))?;
        let name = asset_entry_name(doc, *id).expect("asset exists");
        // Already-compressed formats are stored as-is.
        let opts = if name.ends_with(".svg") {
            deflated
        } else {
            stored
        };
        zip.start_file(name, opts)?;
        zip.write_all(data)?;
    }
    if let Some(png) = thumbnail {
        zip.start_file(THUMBNAIL, stored)?;
        zip.write_all(png)?;
    }
    Ok(zip.finish()?.into_inner())
}

fn read_entry<R: Read + std::io::Seek>(
    zip: &mut zip::ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>, IoError> {
    let entry = zip
        .by_name(name)
        .map_err(|_| IoError::Format(format!("missing entry {name}")))?;
    if entry.size() > MAX_ENTRY_BYTES {
        return Err(IoError::Format(format!("entry {name} is too large")));
    }
    let mut buf = Vec::with_capacity(entry.size() as usize);
    entry.take(MAX_ENTRY_BYTES).read_to_end(&mut buf)?;
    Ok(buf)
}

/// Read a document from `.pubrs` bytes, migrating and checking integrity.
pub fn read_bytes(bytes: &[u8]) -> Result<Document, IoError> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))?;
    if let Ok(m) = read_entry(&mut zip, "mimetype")
        && m != MIMETYPE.as_bytes()
    {
        return Err(IoError::Format("wrong mimetype entry".into()));
    }
    let json = read_entry(&mut zip, DOCUMENT_JSON)?;
    let text = std::str::from_utf8(&json)
        .map_err(|_| IoError::Format("document.json is not UTF-8".into()))?;
    let mut doc = parse_document_json(text)?;
    let ids: Vec<_> = doc.assets.keys().copied().collect();
    for id in ids {
        let name = asset_entry_name(&doc, id).expect("asset exists");
        let data = read_entry(&mut zip, &name)?;
        doc.blobs.insert(id, Arc::from(data));
    }
    let errs = check_integrity(&doc);
    if !errs.is_empty() {
        return Err(IoError::Integrity(errs));
    }
    Ok(doc)
}

/// Read the thumbnail entry, if present.
pub fn read_thumbnail(bytes: &[u8]) -> Option<Vec<u8>> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).ok()?;
    read_entry(&mut zip, THUMBNAIL).ok()
}

/// Save atomically: write a temporary file next to `path`, then rename it
/// over the destination.
pub fn save(doc: &Document, path: &Path, thumbnail: Option<&[u8]>) -> Result<(), IoError> {
    let bytes = write_bytes(doc, thumbnail)?;
    write_atomic(path, &bytes)
}

/// Write bytes to `path` atomically (temp file + rename in the same directory).
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<(), IoError> {
    let dir = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
        _ => std::env::current_dir()?,
    };
    let name = path
        .file_name()
        .ok_or_else(|| IoError::Format(format!("not a file path: {}", path.display())))?;
    let mut tmp_name = std::ffi::OsString::from(".");
    tmp_name.push(name);
    tmp_name.push(format!(".{}.tmp", std::process::id()));
    let tmp = dir.join(tmp_name);
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e.into());
    }
    Ok(())
}

/// Load a `.pubrs` file.
pub fn load(path: &Path) -> Result<Document, IoError> {
    let bytes = std::fs::read(path)?;
    read_bytes(&bytes)
}
