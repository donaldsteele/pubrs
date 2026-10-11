//! Schema migration. `MIGRATIONS[n]` upgrades version `n` to `n + 1`;
//! loading applies every step from the file's version to the current one.

use pubrs_core::SCHEMA_VERSION;
use serde_json::{Value, json};

use crate::error::IoError;

type Step = fn(Value) -> Result<Value, IoError>;

/// One entry per past version, indexed by the version it upgrades from.
const MIGRATIONS: [Step; SCHEMA_VERSION as usize] = [v0_to_v1, v1_to_v2, v2_to_v3];

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
