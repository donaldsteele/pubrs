//! ARCHITECTURE.md: dependencies point strictly downward.
//! core <- layout <- render <- io <- api <- {mcp, cli, app}
//! (mcp, cli and app are peers and must not depend on each other).

use std::collections::BTreeMap;
use std::process::Command;

use serde_json::Value;

/// Layer number per workspace crate; a crate may depend only on lower layers.
const LAYERS: [(&str, u32); 8] = [
    ("pubrs-core", 0),
    ("pubrs-layout", 1),
    ("pubrs-render", 2),
    ("pubrs-io", 3),
    ("pubrs-api", 4),
    ("pubrs-mcp", 5),
    ("pubrs-cli", 5),
    ("pubrs-app", 5),
];

#[test]
fn workspace_dependencies_point_strictly_downward() {
    let out = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("run cargo metadata");
    assert!(out.status.success(), "cargo metadata failed");
    let meta: Value = serde_json::from_slice(&out.stdout).expect("metadata json");
    let layers: BTreeMap<&str, u32> = LAYERS.into_iter().collect();

    let packages = meta["packages"].as_array().expect("packages");
    let mut seen = 0;
    let mut violations = Vec::new();
    for p in packages {
        let name = p["name"].as_str().expect("name");
        let Some(&layer) = layers.get(name) else {
            continue;
        };
        seen += 1;
        for d in p["dependencies"].as_array().expect("dependencies") {
            // kind is null for normal dependencies; "dev" and "build" are not
            // part of the runtime layering.
            if !d["kind"].is_null() {
                continue;
            }
            let dep = d["name"].as_str().expect("dep name");
            if let Some(&dl) = layers.get(dep)
                && dl >= layer
            {
                violations.push(format!("{name} (layer {layer}) -> {dep} (layer {dl})"));
            }
        }
    }
    assert_eq!(
        seen,
        LAYERS.len(),
        "every layered crate is in the workspace"
    );
    assert!(
        violations.is_empty(),
        "upward or sideways dependencies: {violations:?}"
    );
}
