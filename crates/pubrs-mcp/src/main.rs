//! `pubrs-mcp`: MCP server for pubrs. stdio by default; `--http ADDR` for streamable HTTP.

use std::net::SocketAddr;
use std::path::PathBuf;

use clap::Parser;
use pubrs_api::{Session, SessionConfig};
use pubrs_mcp::{PubrsServer, serve_http, serve_stdio};
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(
    name = "pubrs-mcp",
    version,
    about = "MCP server for the pubrs desktop publishing engine"
)]
struct Args {
    /// Serve streamable HTTP at this address (e.g. 127.0.0.1:8765) instead of stdio.
    #[arg(long, value_name = "ADDR")]
    http: Option<SocketAddr>,
    /// Folder for saved and exported files given as relative paths
    /// (default: Documents/pubrs).
    #[arg(long, value_name = "DIR")]
    output_dir: Option<PathBuf>,
    /// Folder for user templates (default: the platform data folder).
    #[arg(long, value_name = "DIR")]
    templates_dir: Option<PathBuf>,
    /// Use only the bundled fonts.
    #[arg(long)]
    no_system_fonts: bool,
}

fn default_output_dir() -> Option<PathBuf> {
    dirs::document_dir()
        .or_else(dirs::home_dir)
        .map(|d| d.join("pubrs"))
}

fn default_templates_dir() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("pubrs").join("templates"))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // stdout carries the protocol in stdio mode: log to stderr only.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();
    let args = Args::parse();
    let config = SessionConfig {
        system_fonts: !args.no_system_fonts,
        output_dir: args.output_dir.or_else(default_output_dir),
        user_templates_dir: args.templates_dir.or_else(default_templates_dir),
        ..Default::default()
    };
    tracing::info!(
        output_dir = ?config.output_dir,
        templates = ?config.user_templates_dir,
        "starting pubrs-mcp {}",
        env!("CARGO_PKG_VERSION")
    );
    let server = PubrsServer::new(Session::new(config));
    match args.http {
        Some(addr) => {
            let ct = CancellationToken::new();
            let stop = ct.clone();
            tokio::spawn(async move {
                let _ = tokio::signal::ctrl_c().await;
                stop.cancel();
            });
            serve_http(server, addr, ct).await
        }
        None => serve_stdio(server).await,
    }
}
