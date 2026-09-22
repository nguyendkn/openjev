// Pipeline errors
use std::fmt;

/// Errors surfaced by the `pipeline` crate's readout/generate orchestration.
#[derive(Debug)]
pub enum PipelineError {
    Engine(String),
    InvalidOptions(String),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PipelineError::Engine(e) => write!(f, "engine error: {e}"),
            PipelineError::InvalidOptions(e) => write!(f, "invalid options: {e}"),
        }
    }
}

impl std::error::Error for PipelineError {}
