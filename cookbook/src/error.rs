//! The crate's error type. Extraction never panics on bad input: a malformed
//! EPUB, a refused request, or a cancelled run all surface here.

use crate::report::RunReport;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("not an EPUB: {0}")]
    NotAnEpub(String),
    #[error("zip: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("xml in {path}: {message}")]
    Xml { path: String, message: String },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unknown model {0:?} (not in the catalog)")]
    UnknownModel(String),
    #[error("{0}")]
    Config(String),
    /// The run was cancelled. The partial report is attached so hosts can
    /// still show what was spent.
    #[error("extraction cancelled")]
    Cancelled(Box<RunReport>),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
