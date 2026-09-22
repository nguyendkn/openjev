// Laya pipeline: thin wrapper around `models::laya::score`, the 3rd comparison method alongside
// `readout`/`generate`.
use crate::error::PipelineError;
use models::LayaScoreResult;

/// Runs Laya's constrained choice-scoring for `prompt`/`options` via the already-warm `laya
/// serve` process (see `models::laya` for the HTTP client + response-shape doc comment). Unlike
/// `run_readout`/`run_generate`, this takes no `Engine`: Laya's own GGUF is loaded and kept warm
/// by the separate `laya serve` process (started out-of-band, see README/deployment docs), not
/// by this project's `crates/engine`.
///
/// `Timings::laya_model_load_ms` is deliberately NOT measured here and stays 0 for every call:
/// the model is loaded exactly once, at `laya serve` startup, not per-request. This is
/// intentional — do not mistake the always-0 value for a missing/broken timing. Callers should
/// time only the HTTP round-trip (this call) as `laya_inference_ms`.
///
/// Returns `Err` (not a panic/process exit) when `laya serve` is unreachable or returns an
/// unexpected response — callers (`apps/cli`, `apps/server`) are expected to degrade gracefully
/// (`laya: null` in the report) rather than fail the whole readout+generate request, per Loop 5's
/// design (Laya being down must not take down the other 2 methods' results).
pub fn run_laya(prompt: &str, options: &[String]) -> Result<LayaScoreResult, PipelineError> {
    models::laya_score(prompt, options).map_err(|e| PipelineError::Laya(e.to_string()))
}
