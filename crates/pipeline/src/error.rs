// Pipeline errors
use std::fmt;

/// Errors surfaced by the `pipeline` crate's readout/generate orchestration.
#[derive(Debug)]
pub enum PipelineError {
    Engine(String),
    InvalidOptions(String),
    /// Laya (`laya serve`) call failed — unreachable or an unexpected response. Distinct from
    /// `Engine` because a Laya failure is expected to degrade gracefully (readout/generate still
    /// succeed, `laya: null` in the report) rather than fail the whole request; callers decide
    /// whether to propagate or swallow this variant per that policy.
    Laya(String),
}

impl fmt::Display for PipelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PipelineError::Engine(e) => write!(f, "engine error: {e}"),
            PipelineError::InvalidOptions(e) => write!(f, "invalid options: {e}"),
            PipelineError::Laya(e) => write!(f, "laya error: {e}"),
        }
    }
}

impl std::error::Error for PipelineError {}
