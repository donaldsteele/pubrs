//! Script replay: a list of tool calls (the same calls MCP clients make).
//!
//! ```json
//! [
//!   {"tool": "doc_new", "args": {"title": "Flyer"}, "as": "doc"},
//!   {"tool": "frame_add_text", "args": {"doc_id": "${doc.doc_id}", "x": 36, "y": 36,
//!     "width": 300, "height": 80, "text": "Hello"}, "as": "title"},
//!   {"tool": "export_pdf", "args": {"path": "${out_dir}/flyer.pdf"}}
//! ]
//! ```
//!
//! `"as"` stores a step's result; `${name.field.0}` in later arguments is
//! replaced by that value (a string that is exactly one reference becomes
//! the referenced JSON value, otherwise the reference is interpolated).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ApiError;
use crate::registry::call_tool;
use crate::session::Session;

/// One step of a script.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub tool: String,
    #[serde(default)]
    pub args: Value,
    /// Variable name to store the result under.
    #[serde(default, rename = "as", skip_serializing_if = "Option::is_none")]
    pub save_as: Option<String>,
    /// The step is expected to fail with this error code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expect_error: Option<String>,
}

/// A script file: either a bare array of steps or `{"steps": [...]}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ScriptFile {
    Steps(Vec<Step>),
    Wrapped { steps: Vec<Step> },
}

impl ScriptFile {
    pub fn steps(self) -> Vec<Step> {
        match self {
            ScriptFile::Steps(s) | ScriptFile::Wrapped { steps: s } => s,
        }
    }
}

/// Result of one step.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StepResult {
    pub index: usize,
    pub tool: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ApiError>,
}

/// Outcome of a whole script.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ScriptReport {
    pub results: Vec<StepResult>,
    /// True if every step behaved as expected.
    pub ok: bool,
}

fn lookup<'a>(vars: &'a BTreeMap<String, Value>, path: &str) -> Option<&'a Value> {
    let mut parts = path.split('.');
    let mut cur = vars.get(parts.next()?)?;
    for p in parts {
        cur = match cur {
            Value::Object(o) => o.get(p)?,
            Value::Array(a) => a.get(p.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

fn substitute(v: &Value, vars: &BTreeMap<String, Value>) -> Result<Value, ApiError> {
    Ok(match v {
        Value::String(s) => {
            if let Some(inner) = s.strip_prefix("${").and_then(|r| r.strip_suffix('}'))
                && !inner.contains("${")
                && !inner.contains('}')
            {
                return lookup(vars, inner).cloned().ok_or_else(|| unknown(inner));
            }
            let mut out = String::new();
            let mut rest = s.as_str();
            while let Some(start) = rest.find("${") {
                out.push_str(&rest[..start]);
                let after = &rest[start + 2..];
                let end = after
                    .find('}')
                    .ok_or_else(|| ApiError::invalid("unclosed ${ in script", "close it with }"))?;
                let name = &after[..end];
                let val = lookup(vars, name).ok_or_else(|| unknown(name))?;
                match val {
                    Value::String(s) => out.push_str(s),
                    other => out.push_str(&other.to_string()),
                }
                rest = &after[end + 1..];
            }
            out.push_str(rest);
            Value::String(out)
        }
        Value::Array(a) => Value::Array(
            a.iter()
                .map(|x| substitute(x, vars))
                .collect::<Result<_, _>>()?,
        ),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, x)| substitute(x, vars).map(|x| (k.clone(), x)))
                .collect::<Result<_, _>>()?,
        ),
        other => other.clone(),
    })
}

fn unknown(name: &str) -> ApiError {
    ApiError::invalid(
        format!("unknown script variable ${{{name}}}"),
        "store results with \"as\" before referring to them",
    )
}

/// Run steps in order. Stops at the first unexpected failure.
pub fn run_script(
    session: &mut Session,
    steps: &[Step],
    mut vars: BTreeMap<String, Value>,
) -> ScriptReport {
    let mut results = Vec::new();
    let mut ok = true;
    for (index, step) in steps.iter().enumerate() {
        let outcome =
            substitute(&step.args, &vars).and_then(|args| call_tool(session, &step.tool, args));
        let (good, output, error) = match (outcome, &step.expect_error) {
            (Ok(v), None) => (true, Some(v), None),
            (Ok(v), Some(_)) => (false, Some(v), None),
            (Err(e), Some(code)) if &e.code == code => (true, None, Some(e)),
            (Err(e), _) => (false, None, Some(e)),
        };
        if let (Some(name), Some(v)) = (&step.save_as, &output) {
            vars.insert(name.clone(), v.clone());
        }
        results.push(StepResult {
            index,
            tool: step.tool.clone(),
            ok: good,
            output,
            error,
        });
        if !good {
            ok = false;
            break;
        }
    }
    ScriptReport { results, ok }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn substitution() {
        let mut vars = BTreeMap::new();
        vars.insert(
            "doc".into(),
            json!({"doc_id": "abc", "page_ids": ["p1", "p2"], "n": 3}),
        );
        let v = substitute(
            &json!({"a": "${doc.doc_id}", "b": "${doc.page_ids.1}", "c": "x-${doc.n}-y", "d": "${doc.page_ids}"}),
            &vars,
        )
        .unwrap();
        assert_eq!(
            v,
            json!({"a": "abc", "b": "p2", "c": "x-3-y", "d": ["p1", "p2"]})
        );
        assert!(substitute(&json!("${nope}"), &vars).is_err());
    }
}
