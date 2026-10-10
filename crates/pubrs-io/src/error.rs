//! I/O errors.

/// Errors reading or writing documents and assets.
#[derive(Debug, thiserror::Error)]
pub enum IoError {
    #[error("file error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a valid .pubrs archive: {0}")]
    Zip(String),
    #[error("invalid document JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported document: {0}")]
    Format(String),
    #[error("document is inconsistent: {}", .0.join("; "))]
    Integrity(Vec<String>),
    #[error("unsupported or corrupt image: {0}")]
    Image(String),
}

impl From<zip::result::ZipError> for IoError {
    fn from(e: zip::result::ZipError) -> Self {
        IoError::Zip(e.to_string())
    }
}

impl IoError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            IoError::Io(e) if e.kind() == std::io::ErrorKind::NotFound => "file_not_found",
            IoError::Io(_) => "io_error",
            IoError::Zip(_) | IoError::Json(_) | IoError::Format(_) | IoError::Integrity(_) => {
                "invalid_file"
            }
            IoError::Image(_) => "invalid_image",
        }
    }
}
