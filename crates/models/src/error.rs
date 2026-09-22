// Model errors
use std::fmt;

/// Errors surfaced by the `models` crate's registry/download logic.
#[derive(Debug)]
pub enum ModelsError {
    /// `hf-hub` client construction failed (e.g. bad `HF_ENDPOINT`).
    ClientInit(String),
    /// `hf-hub` download failed (network, auth, missing repo/file/revision, etc.).
    Download(String),
    /// Registry lookup for an unknown model id.
    UnknownModel(String),
}

impl fmt::Display for ModelsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ModelsError::ClientInit(e) => write!(f, "hf-hub client init failed: {e}"),
            ModelsError::Download(e) => write!(f, "model download failed: {e}"),
            ModelsError::UnknownModel(id) => write!(f, "unknown model id: {id}"),
        }
    }
}

impl std::error::Error for ModelsError {}
