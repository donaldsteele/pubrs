//! Schema migration. `MIGRATIONS[n]` upgrades version `n` to `n + 1`;
//! loading applies every step from the file's version to the current one.

use pubrs_core::SCHEMA_VERSION;
use serde_json::{Value, json};

use crate::error::IoError;

type Step = fn(Value) -> Result<Value, IoError>;

/// One entry per past version, indexed by the version it upgrades from.
const MIGRATIONS: [Step; SCHEMA_VERSION as usize] = [
    v0_to_v1, v1_to_v2, v2_to_v3, v3_to_v4, v4_to_v5, v5_to_v6, v6_to_v7,
];

/// The schema version a document JSON declares (absent = 0).
pub fn version_of(v: &Value) -> u64 {
    v.get("schema_version").and_then(Value::as_u64).unwrap_or(0)
}

/// Upgrade document JSON to the current schema version.
pub fn migrate(mut v: Value) -> Result<Value, IoError> {
    if !v.is_object() {
        return Err(IoError::Format(
            "document.json must be a JSON object".into(),
        ));
    }
    let from = version_of(&v);
    if from > u64::from(SCHEMA_VERSION) {
        return Err(IoError::Format(format!(
            "written by a newer pubrs (schema {from}; this build reads up to {SCHEMA_VERSION})"
        )));
    }
    for step in &MIGRATIONS[from as usize..] {
        v = step(v)?;
    }
    Ok(v)
}

/// Version 0 is unversioned, hand-written or pre-release JSON: it may omit
/// `schema_version`, `id`, `meta` and `grid`.
fn v0_to_v1(mut v: Value) -> Result<Value, IoError> {
    let o = v.as_object_mut().expect("checked object");
    o.insert("schema_version".into(), json!(1));
    o.entry("id")
        .or_insert_with(|| json!("00000000-0000-0000-0000-000000000000"));
    o.entry("meta").or_insert_with(|| json!({}));
    o.entry("grid").or_insert_with(|| {
        serde_json::to_value(pubrs_core::GridSettings::default()).expect("serializable")
    });
    Ok(v)
}

/// Version 2 adds `setup.sheet` and `setup.booklet`, both optional with
/// defaults, so a version 1 document only needs its version stamped.
fn v1_to_v2(mut v: Value) -> Result<Value, IoError> {
    v.as_object_mut()
        .expect("checked object")
        .insert("schema_version".into(), json!(2));
    Ok(v)
}

/// Version 3 adds the paragraph fields `hyphenate`, `language` and
/// `justify_limits`, all optional; unset keeps v0.1 behaviour.
fn v2_to_v3(mut v: Value) -> Result<Value, IoError> {
    v.as_object_mut()
        .expect("checked object")
        .insert("schema_version".into(), json!(3));
    Ok(v)
}

/// Version 4 adds `swatches` and `scheme` to the document and lets fills,
/// strokes and text colours be a swatch reference or (fills) a gradient. All
/// are optional with defaults and the old hex strings stay valid, so a
/// version 3 document only needs its version stamped.
fn v3_to_v4(mut v: Value) -> Result<Value, IoError> {
    v.as_object_mut()
        .expect("checked object")
        .insert("schema_version".into(), json!(4));
    Ok(v)
}

/// Version 5 (commercial print output) adds the optional `alt_text` of picture
/// frames; absent means empty, so a version 4 document only needs its version
/// stamped.
fn v4_to_v5(mut v: Value) -> Result<Value, IoError> {
    v.as_object_mut()
        .expect("checked object")
        .insert("schema_version".into(), json!(5));
    Ok(v)
}

/// Version 6 adds the paragraph fields `keep_with_next`, `keep_together`,
/// `widow_orphan` and `drop_cap`, all optional (unset keeps v0.1 layout),
/// and gives meaning to the column, frame and page break characters
/// U+000E, U+001D and U+000C in story text (ADR 0015), which older files
/// cannot contain in practice. Only the version changes.
fn v5_to_v6(mut v: Value) -> Result<Value, IoError> {
    v.as_object_mut()
        .expect("checked object")
        .insert("schema_version".into(), json!(6));
    Ok(v)
}

/// Version 7 adds the paragraph fields `tab_stops` and `list` (tab stops with
/// leaders, bullets and numbering; ADR 0019), both optional: unset keeps
/// v0.1 layout. Only the version changes.
fn v6_to_v7(mut v: Value) -> Result<Value, IoError> {
    v.as_object_mut()
        .expect("checked object")
        .insert("schema_version".into(), json!(7));
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_newer_versions() {
        let err = migrate(json!({"schema_version": 999})).unwrap_err();
        assert!(err.to_string().contains("newer"));
    }

    #[test]
    fn current_version_is_untouched() {
        let v = json!({"schema_version": SCHEMA_VERSION, "x": 1});
        assert_eq!(migrate(v.clone()).unwrap(), v);
    }
}
