// Engine errors
use std::fmt;

/// Errors surfaced by the `engine` crate's `llama-cpp-2`-backed inference wrapper.
#[derive(Debug)]
pub enum EngineError {
    BackendInit(String),
    ModelLoad(String),
    ContextInit(String),
    Tokenize(String),
    ChatTemplate(String),
    Decode(String),
    BatchAdd(String),
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::BackendInit(e) => write!(f, "llama backend init failed: {e}"),
            EngineError::ModelLoad(e) => write!(f, "model load failed: {e}"),
            EngineError::ContextInit(e) => write!(f, "context init failed: {e}"),
            EngineError::Tokenize(e) => write!(f, "tokenize failed: {e}"),
            EngineError::ChatTemplate(e) => write!(f, "chat template failed: {e}"),
            EngineError::Decode(e) => write!(f, "decode failed: {e}"),
            EngineError::BatchAdd(e) => write!(f, "batch add failed: {e}"),
        }
    }
}

impl std::error::Error for EngineError {}
