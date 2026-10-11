//! Read-only resources:
//! - `pubrs://fonts` — available fonts
//! - `pubrs://templates/{name}` — template metadata and placeholders
//! - `pubrs://doc/{doc_id}` — current document JSON
//! - `pubrs://doc/{doc_id}/page/{n}.png` — page preview (n is 1-based)
//! - `pubrs://doc/{doc_id}/swatches` — the document's swatches and colour scheme

use pubrs_api::Session;
use pubrs_api::core::DocumentId;
use pubrs_api::render::RasterOptions;
use pubrs_api::tools::color::SwatchList;
use pubrs_api::tools::doc::DocRef;
use rmcp::ErrorData;
use rmcp::model::{ReadResourceResult, Resource, ResourceContents, ResourceTemplate};
use serde_json::json;

/// Preview resolution for page resources.
pub const PAGE_PREVIEW_DPI: f64 = 72.0;

pub fn templates() -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate::new("pubrs://templates/{name}", "template")
            .with_description(
                "Template metadata: page size and placeholders (name, kind, hint, size).",
            )
            .with_mime_type("application/json"),
        ResourceTemplate::new("pubrs://doc/{doc_id}", "document")
            .with_description("The complete document as JSON (same as doc_to_json).")
            .with_mime_type("application/json"),
        ResourceTemplate::new("pubrs://doc/{doc_id}/swatches", "swatches")
            .with_description(
                "The document's swatches (rgb, cmyk, spot) with screen colours and uses, and the applied colour scheme (same as swatch_list).",
            )
            .with_mime_type("application/json"),
        ResourceTemplate::new("pubrs://doc/{doc_id}/page/{n}.png", "page preview")
            .with_description("PNG preview of page n (1-based) at 72 dpi.")
            .with_mime_type("image/png"),
    ]
}

pub fn list(s: &mut Session) -> Vec<Resource> {
    let mut out = vec![
        Resource::new("pubrs://fonts", "fonts")
            .with_description("Available font families")
            .with_mime_type("application/json"),
    ];
    if let Ok(t) = s.template_list(Default::default()) {
        for t in t.templates {
            out.push(
                Resource::new(
                    format!("pubrs://templates/{}", t.name),
                    format!("template {}", t.name),
                )
                .with_description(t.description)
                .with_mime_type("application/json"),
            );
        }
    }
    let docs: Vec<(DocumentId, String, usize)> = s
        .documents()
        .map(|d| (d.doc_id, d.doc().meta.title.clone(), d.doc().pages.len()))
        .collect();
    for (id, title, pages) in docs {
        out.push(
            Resource::new(format!("pubrs://doc/{id}"), format!("document {title}"))
                .with_mime_type("application/json"),
        );
        out.push(
            Resource::new(
                format!("pubrs://doc/{id}/swatches"),
                format!("{title} swatches"),
            )
            .with_mime_type("application/json"),
        );
        for n in 1..=pages {
            out.push(
                Resource::new(
                    format!("pubrs://doc/{id}/page/{n}.png"),
                    format!("{title} page {n}"),
                )
                .with_mime_type("image/png"),
            );
        }
    }
    out
}

fn not_found(uri: &str, hint: &str) -> ErrorData {
    ErrorData::resource_not_found(
        format!("no resource {uri}"),
        Some(json!({"uri": uri, "hint": hint})),
    )
}

fn api(uri: &str, e: pubrs_api::ApiError) -> ErrorData {
    ErrorData::resource_not_found(
        format!("{uri}: {}", e.message),
        Some(json!({"uri": uri, "code": e.code, "hint": e.hint})),
    )
}

pub fn read(s: &mut Session, uri: &str) -> Result<ReadResourceResult, ErrorData> {
    let text = |v: serde_json::Value| {
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(v.to_string(), uri).with_mime_type("application/json"),
        ]))
    };
    let rest = uri
        .strip_prefix("pubrs://")
        .ok_or_else(|| not_found(uri, "pubrs resources start with pubrs://"))?;
    let parts: Vec<&str> = rest.split('/').collect();
    match parts.as_slice() {
        ["fonts"] => {
            let f = s.fonts_list(DocRef::default()).map_err(|e| api(uri, e))?;
            text(serde_json::to_value(f).unwrap_or_default())
        }
        ["templates", name] => {
            let list = s
                .template_list(Default::default())
                .map_err(|e| api(uri, e))?;
            let t = list
                .templates
                .into_iter()
                .find(|t| t.name == *name)
                .ok_or_else(|| not_found(uri, "list templates with template_list"))?;
            text(serde_json::to_value(t).unwrap_or_default())
        }
        ["doc", id] => {
            let doc_id = id
                .parse()
                .map_err(|_| not_found(uri, "doc ids are UUIDs (doc_list)"))?;
            let out = s
                .doc_to_json(DocRef {
                    doc_id: Some(doc_id),
                })
                .map_err(|e| api(uri, e))?;
            text(serde_json::to_value(out.document).unwrap_or_default())
        }
        ["doc", id, "swatches"] => {
            let doc_id: DocumentId = id
                .parse()
                .map_err(|_| not_found(uri, "doc ids are UUIDs (doc_list)"))?;
            let out = s
                .swatch_list(SwatchList {
                    doc_id: Some(doc_id),
                })
                .map_err(|e| api(uri, e))?;
            text(serde_json::to_value(out).unwrap_or_default())
        }
        ["doc", id, "page", file] => {
            let doc_id: DocumentId = id
                .parse()
                .map_err(|_| not_found(uri, "doc ids are UUIDs (doc_list)"))?;
            let n: usize = file
                .strip_suffix(".png")
                .and_then(|n| n.parse().ok())
                .filter(|n| *n >= 1)
                .ok_or_else(|| not_found(uri, "pages are numbered from 1: .../page/1.png"))?;
            let (_, _, png) = s
                .page_png(
                    Some(doc_id),
                    n - 1,
                    &RasterOptions {
                        dpi: PAGE_PREVIEW_DPI,
                        ..Default::default()
                    },
                    true,
                )
                .map_err(|e| api(uri, e))?;
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(png);
            Ok(ReadResourceResult::new(vec![
                ResourceContents::blob(b64, uri).with_mime_type("image/png"),
            ]))
        }
        _ => Err(not_found(uri, "see resources/templates/list")),
    }
}
