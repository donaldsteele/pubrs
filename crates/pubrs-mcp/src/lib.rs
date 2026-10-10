//! `pubrs-mcp`: an MCP server over `pubrs-api`.
//!
//! Tools map 1:1 onto [`pubrs_api::registry::TOOLS`]; this crate only adds
//! the MCP wire format (schemas, content blocks, error mapping), resources
//! and prompts.

pub mod prompts;
pub mod resources;

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};

use pubrs_api::{ApiError, Session, ToolDef};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, GetPromptRequestParams,
    GetPromptResponse, Implementation, JsonObject, ListPromptsResult, ListResourceTemplatesResult,
    ListResourcesResult, ListToolsResult, PaginatedRequestParams, ReadResourceRequestParams,
    ReadResourceResponse, ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

/// The MCP server: a shared [`Session`]. Cheap to clone (one per HTTP session).
#[derive(Clone)]
pub struct PubrsServer {
    session: Arc<Mutex<Session>>,
}

impl PubrsServer {
    pub fn new(session: Session) -> Self {
        Self {
            session: Arc::new(Mutex::new(session)),
        }
    }

    /// The shared session (tests inspect it directly).
    pub fn session(&self) -> Arc<Mutex<Session>> {
        self.session.clone()
    }

    /// Run `f` with the session on a blocking thread (rendering can take a while).
    pub async fn with_session<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Session) -> T + Send + 'static,
    ) -> Result<T, ErrorData> {
        let session = self.session.clone();
        tokio::task::spawn_blocking(move || {
            let mut guard = session.lock().unwrap_or_else(|p| p.into_inner());
            f(&mut guard)
        })
        .await
        .map_err(|e| ErrorData::internal_error(format!("worker failed: {e}"), None))
    }
}

fn object(v: Value) -> Arc<JsonObject> {
    Arc::new(v.as_object().cloned().unwrap_or_default())
}

/// The MCP description of one registry tool.
pub fn mcp_tool(t: &ToolDef) -> Tool {
    let annotations = ToolAnnotations::new()
        .read_only(t.read_only)
        .destructive(t.destructive)
        .idempotent(t.read_only)
        .open_world(false);
    Tool::new(t.name, t.description, object((t.input_schema)()))
        .with_raw_output_schema(object((t.output_schema)()))
        .with_annotations(annotations)
}

/// Tool result as MCP content: structured JSON plus a text copy; tools with
/// an image field also return the PNG as an image block.
pub fn success_result(t: &ToolDef, value: Value) -> CallToolResult {
    match t.image_field.and_then(|f| {
        value
            .get(f)
            .and_then(Value::as_str)
            .map(|b| (f, b.to_string()))
    }) {
        Some((field, b64)) => {
            let mut summary = value.clone();
            if let Some(o) = summary.as_object_mut() {
                o.remove(field);
            }
            let mut r = CallToolResult::success(vec![
                ContentBlock::image(b64, "image/png"),
                ContentBlock::text(summary.to_string()),
            ]);
            r.structured_content = Some(value);
            r
        }
        None => CallToolResult::structured(value),
    }
}

/// A tool-level error the model can read and act on: `{code, message, hint}`.
pub fn error_result(e: &ApiError) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(
        json!({"code": e.code, "message": e.message, "hint": e.hint}).to_string(),
    )])
}

/// Guidance sent to clients on connect.
pub fn instructions(output_dir: Option<&std::path::Path>) -> String {
    let out = output_dir.map_or_else(
        || "relative paths resolve against the server's working directory".to_string(),
        |d| {
            format!(
                "relative paths (and saves/exports without a path) go to {}",
                d.display()
            )
        },
    );
    format!(
        "pubrs creates print documents (flyers, brochures, cards, newsletters).\n\
         Units are points (72 per inch). Coordinates start at the top-left corner of the page, y grows down.\n\
         Every object has a stable id; address pages and frames by id. doc_id may be omitted (most recent document).\n\
         Typical workflow: template_list → template_apply → text_set on each text placeholder → image_place on each image \
         placeholder (file path, base64, or svg markup for drawn artwork) → text_overflow_status → validate (fix every warning) → \
         render_preview to look at each page → doc_save_as and export_pdf.\n\
         Prompts make_flyer, make_trifold and make_business_card walk through these steps. Files: {out}."
    )
}

impl ServerHandler for PubrsServer {
    fn get_info(&self) -> ServerConfig {
        let out = self
            .session
            .lock()
            .ok()
            .and_then(|s| s.config().output_dir.clone());
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
        .with_server_info(
            Implementation::new("pubrs", env!("CARGO_PKG_VERSION"))
                .with_title("pubrs desktop publishing"),
        )
        .with_instructions(instructions(out.as_deref()))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(
            pubrs_api::all_tools().iter().map(mcp_tool).collect(),
        ))
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        pubrs_api::tool(name).map(mcp_tool)
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let name = request.name.to_string();
        let Some(def) = pubrs_api::tool(&name) else {
            return Err(ErrorData::invalid_params(
                format!("unknown tool: {name}"),
                Some(json!({ "tool": name })),
            ));
        };
        let args = Value::Object(request.arguments.unwrap_or_default());
        tracing::debug!(tool = %name, "call");
        let result = self
            .with_session(move |s| pubrs_api::call_tool(s, &name, args))
            .await?;
        Ok(match result {
            Ok(v) => success_result(def, v),
            Err(e) => {
                tracing::debug!(tool = def.name, code = %e.code, "tool error: {}", e.message);
                error_result(&e)
            }
        }
        .into())
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let list = self.with_session(resources::list).await?;
        Ok(ListResourcesResult::with_all_items(list))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        Ok(ListResourceTemplatesResult::with_all_items(
            resources::templates(),
        ))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let uri = request.uri.clone();
        let result = self
            .with_session(move |s| resources::read(s, &uri))
            .await??;
        Ok(result.into())
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, ErrorData> {
        Ok(ListPromptsResult::with_all_items(prompts::list()))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, ErrorData> {
        Ok(prompts::get(&request.name, request.arguments.as_ref())?.into())
    }
}

/// Serve over stdin/stdout until the client disconnects.
pub async fn serve_stdio(server: PubrsServer) -> anyhow::Result<()> {
    let running = server.serve(rmcp::transport::stdio()).await?;
    let reason = running.waiting().await?;
    tracing::info!("stdio client disconnected: {reason:?}");
    Ok(())
}

/// The streamable-HTTP MCP endpoint, mounted at `/mcp`.
pub fn http_router(server: PubrsServer, ct: CancellationToken) -> axum::Router {
    let config = StreamableHttpServerConfig::default().with_cancellation_token(ct);
    let service: StreamableHttpService<PubrsServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(server.clone()),
            Arc::new(LocalSessionManager::default()),
            config,
        );
    axum::Router::new().nest_service("/mcp", service)
}

/// Serve streamable HTTP at `addr` until `ct` is cancelled.
pub async fn serve_http(
    server: PubrsServer,
    addr: SocketAddr,
    ct: CancellationToken,
) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("MCP endpoint: http://{}/mcp", listener.local_addr()?);
    eprintln!(
        "pubrs-mcp listening on http://{}/mcp",
        listener.local_addr()?
    );
    let router = http_router(server, ct.child_token());
    axum::serve(listener, router)
        .with_graceful_shutdown(async move { ct.cancelled_owned().await })
        .await?;
    Ok(())
}
