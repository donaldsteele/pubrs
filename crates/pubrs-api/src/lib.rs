//! `pubrs-api`: the `Session` façade. The single entry point used by the
//! GUI, the CLI and the MCP server. If an operation is not here, it does
//! not exist for any client.
//!
//! - Typed methods on [`Session`] (one per tool) for Rust clients.
//! - [`registry::TOOLS`]: the same operations by name with JSON schemas,
//!   used 1:1 by the MCP server and by [`script`] replay.

pub mod error;
pub mod markup;
pub mod perf;
pub mod registry;
pub mod script;
pub mod session;
pub mod tools;

pub use error::{ApiError, ApiResult};
pub use registry::{ToolDef, all_tools, call_tool, schema_of, tool};
pub use script::{ScriptFile, ScriptReport, Step, run_script};
pub use session::{Changed, OpenDoc, Session, SessionConfig};

pub use pubrs_core as core;
pub use pubrs_io as io;
pub use pubrs_layout as layout;
pub use pubrs_render as render;
