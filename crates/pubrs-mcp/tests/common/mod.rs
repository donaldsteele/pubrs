//! In-process MCP client harness shared by the integration tests.
#![allow(dead_code)]

use std::path::PathBuf;

use pubrs_api::{Session, SessionConfig};
use pubrs_mcp::PubrsServer;
use rmcp::model::{CallToolRequestParams, CallToolResult};
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};
use serde_json::Value;

pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

pub struct Harness {
    pub client: RunningService<RoleClient, ()>,
    pub out: PathBuf,
}

/// Start a server with deterministic ids and the test fonts, connected to a
/// client over an in-memory duplex pipe.
pub async fn start(name: &str) -> Harness {
    let out = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let session = Session::new(SessionConfig {
        id_seed: Some(2026),
        font_dir: Some(root().join("tests/fonts")),
        output_dir: Some(out.clone()),
        user_templates_dir: Some(out.join("templates")),
        ..Default::default()
    });
    let server = PubrsServer::new(session);
    let (server_io, client_io) = tokio::io::duplex(1 << 22);
    tokio::spawn(async move {
        let running = server.serve(server_io).await.expect("server starts");
        let _ = running.waiting().await;
    });
    let client = ().serve(client_io).await.expect("client connects");
    Harness { client, out }
}

impl Harness {
    pub async fn raw(&self, name: &str, args: Value) -> CallToolResult {
        let params = CallToolRequestParams::new(name.to_string())
            .with_arguments(args.as_object().cloned().unwrap_or_default());
        self.client.call_tool(params).await.expect("protocol ok")
    }

    /// The tool's JSON error `{code, message, hint}` if it failed.
    pub fn error_of(r: &CallToolResult) -> Option<Value> {
        if r.is_error != Some(true) {
            return None;
        }
        let text = r.content.first()?.as_text()?.text.clone();
        serde_json::from_str(&text).ok()
    }
}
