//! Runs every example in docs/mcp-examples.json through an MCP client.
//! Every tool must have at least one example and every example must behave
//! as documented. `PUBRS_WRITE_DOCS=1` regenerates MCP.md from the tool
//! registry and these real results.
#![allow(clippy::unwrap_used)]

mod common;

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use common::{Harness, root, start};

struct Ran {
    tool: String,
    note: Option<String>,
    args: Value,
    result: Value,
    is_error: bool,
}

/// The `.pub` corpus, for the doc_import examples.
fn corpus_dir() -> String {
    root().join("tests/corpus/pub").display().to_string()
}

fn clean(v: &Value, out: &str, assets: &str, depth: usize) -> Value {
    match v {
        Value::String(s) => {
            let s = s
                .replace('\\', "/")
                .replace(out, "<output_dir>")
                .replace(assets, "<assets>")
                .replace(&corpus_dir().replace('\\', "/"), "tests/corpus/pub");
            if s.len() > 160 {
                Value::String(format!(
                    "{}… ({} characters)",
                    &s[..s.char_indices().nth(60).map_or(s.len(), |c| c.0)],
                    s.len()
                ))
            } else {
                Value::String(s)
            }
        }
        Value::Array(a) => {
            let mut items: Vec<Value> = a
                .iter()
                .take(if depth > 1 { 2 } else { 4 })
                .map(|x| clean(x, out, assets, depth + 1))
                .collect();
            if a.len() > items.len() {
                items.push(json!(format!("… {} more", a.len() - items.len())));
            }
            Value::Array(items)
        }
        Value::Object(o) => {
            let limit = if depth > 1 { 4 } else { 40 };
            let mut m = serde_json::Map::new();
            for (k, x) in o.iter().take(limit) {
                m.insert(k.clone(), clean(x, out, assets, depth + 1));
            }
            if o.len() > limit {
                m.insert("…".into(), json!(format!("{} more keys", o.len() - limit)));
            }
            Value::Object(m)
        }
        other => other.clone(),
    }
}

fn type_of(s: &Value) -> String {
    if let Some(e) = s.get("enum").and_then(Value::as_array) {
        return e
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>()
            .join(" \\| ");
    }
    for key in ["oneOf", "anyOf"] {
        if let Some(alts) = s.get(key).and_then(Value::as_array) {
            let parts: Vec<String> = alts
                .iter()
                .filter(|a| a.get("type") != Some(&json!("null")))
                .map(|a| a.get("const").map_or_else(|| type_of(a), |c| c.to_string()))
                .collect();
            return parts.join(" \\| ");
        }
    }
    match s.get("type") {
        Some(Value::Array(ts)) => ts
            .iter()
            .filter(|t| *t != "null")
            .map(|t| t.as_str().unwrap_or("?").to_string())
            .collect::<Vec<_>>()
            .join(" \\| "),
        Some(Value::String(t)) if t == "array" => {
            format!("array of {}", s.get("items").map_or("any".into(), type_of))
        }
        Some(Value::String(t)) if t == "string" && s.get("format") == Some(&json!("uuid")) => {
            "uuid".into()
        }
        Some(Value::String(t)) => t.clone(),
        _ if s.get("properties").is_some() => "object".into(),
        _ => "any".into(),
    }
}

fn field_table(schema: &Value) -> String {
    let props = schema.get("properties").and_then(Value::as_object);
    let required: BTreeSet<&str> = schema
        .get("required")
        .and_then(Value::as_array)
        .map(|r| r.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let Some(props) = props.filter(|p| !p.is_empty()) else {
        return "_No arguments._\n".into();
    };
    let mut s = String::from("| Field | Type | Required | Description |\n|---|---|---|---|\n");
    for (k, v) in props {
        let desc = v
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .replace('\n', " ")
            .replace('|', "\\|");
        s.push_str(&format!(
            "| `{k}` | {} | {} | {desc} |\n",
            type_of(v),
            if required.contains(k.as_str()) {
                "yes"
            } else {
                ""
            }
        ));
    }
    s
}

fn write_mcp_md(ran: &[Ran]) {
    let mut by_tool: BTreeMap<&str, Vec<&Ran>> = BTreeMap::new();
    for r in ran {
        by_tool.entry(r.tool.as_str()).or_default().push(r);
    }
    let mut md = String::new();
    md.push_str(include_str!("../../../docs/mcp-intro.md"));
    let mut group = "";
    for t in pubrs_api::all_tools() {
        if t.group != group {
            group = t.group;
            md.push_str(&format!("\n## {group}\n"));
        }
        md.push_str(&format!("\n### `{}`\n\n{}\n\n", t.name, t.description));
        let mut flags = Vec::new();
        if t.read_only {
            flags.push("read-only");
        }
        if t.destructive {
            flags.push("destructive (needs `confirm: true`)");
        }
        if t.image_field.is_some() {
            flags.push("returns an image");
        }
        if !flags.is_empty() {
            md.push_str(&format!("_{}_\n\n", flags.join(", ")));
        }
        md.push_str("**Arguments**\n\n");
        md.push_str(&field_table(&(t.input_schema)()));
        md.push_str("\n**Result fields:** ");
        let out = (t.output_schema)();
        let names: Vec<String> = out
            .get("properties")
            .and_then(Value::as_object)
            .map(|p| p.keys().map(|k| format!("`{k}`")).collect())
            .unwrap_or_default();
        md.push_str(&names.join(", "));
        md.push('\n');
        for r in by_tool.get(t.name).into_iter().flatten().take(2) {
            md.push_str("\n**Example**");
            if let Some(n) = &r.note {
                md.push_str(&format!(" — {n}"));
            }
            md.push_str("\n\n```json\n");
            md.push_str(
                &serde_json::to_string_pretty(&json!({"name": r.tool, "arguments": r.args}))
                    .unwrap(),
            );
            md.push_str(&format!(
                "\n```\n\n{}:\n\n```json\n",
                if r.is_error { "Error" } else { "Result" }
            ));
            md.push_str(&serde_json::to_string_pretty(&r.result).unwrap());
            md.push_str("\n```\n");
        }
    }
    std::fs::write(root().join("MCP.md"), md).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_tool_example_runs_through_mcp() {
    let h: Harness = start("mcp-examples").await;
    let steps: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(root().join("docs/mcp-examples.json")).unwrap(),
    )
    .unwrap();
    let assets = root().join("examples/assets").canonicalize().unwrap();
    let assets_s = assets.display().to_string().replace('\\', "/");
    let out_s = h.out.display().to_string().replace('\\', "/");
    let mut vars = BTreeMap::new();
    vars.insert("assets".to_string(), json!(assets.display().to_string()));
    vars.insert("corpus".to_string(), json!(corpus_dir()));
    let mut ran = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        let tool = step["tool"].as_str().unwrap().to_string();
        let args = pubrs_api::script::substitute(&step["args"], &vars).unwrap();
        let r = h.raw(&tool, args.clone()).await;
        let error = Harness::error_of(&r);
        match (step.get("expect_error").and_then(Value::as_str), &error) {
            (None, Some(e)) => panic!("example {i} ({tool}) failed: {e}"),
            (Some(code), Some(e)) => assert_eq!(e["code"], code, "example {i} ({tool})"),
            (Some(code), None) => panic!("example {i} ({tool}) should fail with {code}"),
            (None, None) => {}
        }
        let result = error
            .clone()
            .unwrap_or_else(|| r.structured_content.clone().unwrap_or(Value::Null));
        if let Some(name) = step.get("as").and_then(Value::as_str) {
            vars.insert(name.to_string(), result.clone());
        }
        ran.push(Ran {
            tool,
            note: step.get("note").and_then(Value::as_str).map(String::from),
            args: clean(&args, &out_s, &assets_s, 0),
            result: clean(&result, &out_s, &assets_s, 0),
            is_error: error.is_some(),
        });
    }
    let covered: BTreeSet<&str> = ran.iter().map(|r| r.tool.as_str()).collect();
    let missing: Vec<&str> = pubrs_api::all_tools()
        .iter()
        .map(|t| t.name)
        .filter(|n| !covered.contains(n))
        .collect();
    assert!(missing.is_empty(), "tools without an example: {missing:?}");
    if std::env::var_os("PUBRS_WRITE_DOCS").is_some() {
        write_mcp_md(&ran);
    }
    let md = std::fs::read_to_string(root().join("MCP.md"))
        .expect("MCP.md exists (PUBRS_WRITE_DOCS=1 to generate)");
    for t in pubrs_api::all_tools() {
        assert!(
            md.contains(&format!("### `{}`", t.name)),
            "MCP.md is missing {} (regenerate with PUBRS_WRITE_DOCS=1)",
            t.name
        );
    }
    h.client.cancel().await.unwrap();
}
