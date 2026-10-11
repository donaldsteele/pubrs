//! Structured errors: `{code, message, hint}`. Every API failure maps to one.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use pubrs_core::CommandError;
use pubrs_io::{ExportError, IoError};

/// A structured error returned by every tool. Never a panic.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, thiserror::Error)]
#[error("{code}: {message}")]
pub struct ApiError {
    /// Stable machine-readable code: not_found, invalid_argument, conflict,
    /// locked, confirmation_required, no_document, file_not_found, io_error,
    /// invalid_file, invalid_image, export_failed, unknown_tool, internal.
    pub code: String,
    /// What went wrong.
    pub message: String,
    /// How to fix it.
    pub hint: String,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(code: &str, message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self {
            code: code.to_string(),
            message: message.into(),
            hint: hint.into(),
        }
    }

    pub fn not_found(what: &str, id: &str, hint: &str) -> Self {
        Self::new("not_found", format!("{what} {id} not found"), hint)
    }

    pub fn invalid(message: impl Into<String>, hint: impl Into<String>) -> Self {
        Self::new("invalid_argument", message, hint)
    }

    pub fn confirm(action: &str) -> Self {
        Self::new(
            "confirmation_required",
            format!("{action} is destructive"),
            "repeat the call with \"confirm\": true",
        )
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(
            "internal",
            message,
            "this is a bug in pubrs; please report it",
        )
    }
}

impl From<CommandError> for ApiError {
    fn from(e: CommandError) -> Self {
        let hint = match &e {
            CommandError::NotFound { kind, .. } => match kind.as_str() {
                "frame" => "list frames with frame_list",
                "page" => "list pages with page_list",
                "story" => "get a frame's story with frame_get or text_get",
                "style" => "list styles with style_list",
                "swatch" => "list swatches with swatch_list",
                "scheme" => "list schemes with scheme_list",
                "asset" => "place an image with image_place first",
                "master" => "list masters with doc_info",
                _ => "check the id",
            },
            CommandError::Locked { .. } => "unlock it first with frame_lock (locked: false)",
            CommandError::Invalid { message } if message.contains("gradients are only allowed") => {
                "use a hex colour or {\"swatch\": id} for outlines and text; gradients are for frame fills"
            }
            CommandError::Invalid { message } if message.contains("swatch") => {
                "list swatches with swatch_list, or create one with swatch_create"
            }
            CommandError::Invalid { .. } => "check the argument ranges in the tool's input schema",
            CommandError::Conflict { message } if message.contains("in use") => {
                "pass replace_with (a colour or swatch) to recolour those places, or change them first"
            }
            CommandError::Conflict { .. } => "use a different name or id",
            CommandError::Internal { .. } => "this is a bug in pubrs; please report it",
        };
        ApiError::new(e.code(), e.to_string(), hint)
    }
}

impl From<IoError> for ApiError {
    fn from(e: IoError) -> Self {
        let hint = match e.code() {
            "file_not_found" => {
                "check the path; relative paths resolve against the server's working directory"
            }
            "invalid_file" => "the file is not a valid .pubrs document",
            "invalid_image" => "use a PNG, JPEG or SVG image",
            _ => "check permissions and free disk space",
        };
        ApiError::new(e.code(), e.to_string(), hint)
    }
}

impl From<ExportError> for ApiError {
    fn from(e: ExportError) -> Self {
        let hint = match e.code() {
            "not_found" => "list pages with page_list",
            "invalid_argument" => "use a lower dpi",
            _ => "run validate to look for problems in the document",
        };
        ApiError::new(e.code(), e.to_string(), hint)
    }
}

impl From<serde_json::Error> for ApiError {
    fn from(e: serde_json::Error) -> Self {
        ApiError::invalid(
            format!("invalid arguments: {e}"),
            "check the tool's input schema",
        )
    }
}
