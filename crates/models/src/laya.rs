// Laya HTTP client.
//
// CORRECTED (Loop 5): Laya is NOT invoked via `std::process::Command` shell-out to a one-shot
// `laya decide` CLI (that was Loop 1-4's placeholder plan). The real tool is a separate binary
// named `laya` (built from `github.com/monatis/ggmlc/tree/main/examples/laya`, distinct from
// the generic `ggmlc-run`), run once as a persistent `laya serve <model.gguf> --port 8090
// --device cpu` background process (same lifetime pattern as `apps/server`, started/managed
// outside this crate — see `docs/deployment-guide.md`/README for the exact command). This
// module is just a blocking HTTP client hitting that already-warm server, avoiding a
// process-spawn + model-reload cost on every single benchmark call.
//
// `laya serve` speaks the TypeSafe System One protocol: `POST /v1/decide` with body
// `{"state": <text>, "model": <family>, "questions": {"<id>": {"type": "choice"|"score"|"noul",
// "instructions": ..., "criteria": {<option>: null, ...}}}}`. Response field names below
// (`answers.<id>.choice`, `.probabilities`, `.confidence`) were confirmed by a REAL curl against
// a live `laya serve` instance on the target server (Loop 5 D_5 Task 4), not guessed from docs:
//
// ```
// curl -X POST http://127.0.0.1:8090/v1/decide -d '{"state":"The capital of France is: A)
// London B) Paris","model":"laya-english","questions":{"answer":{"type":"choice",
// "instructions":"Pick the correct option.","criteria":{"A":null,"B":null}}}}'
// -> {"model":"laya","family":"english","route":"model english","answers":{"answer":
//     {"type":"choice","action":{"act_probability":1},"confidence":0.0095,"choice":"B",
//     "probabilities":{"A":0.4426,"B":0.5574}}},
//     "usage":{"input_tokens":28,"output_tokens":0,"latency_ms":8875.27}}
// ```
//
// `family`, `route`, `action`, `usage.latency_ms` are additive Laya-specific fields we ignore
// (this project's own `Timings::laya_inference_ms` measures the HTTP round-trip directly
// instead, see `pipeline::laya::run_laya`).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::time::Duration;

/// `laya serve`'s fixed internal address (localhost-only, started by an out-of-band process —
/// see module doc comment). Not configurable yet; hardcoding matches this project's current
/// no-config-file scope (same as the port-80 default for `apps/server`, see G14).
const LAYA_SERVE_URL: &str = "http://127.0.0.1:8090/v1/decide";

/// Mirrors `pipeline::readout::ReadoutResult`'s shape (`probs` + `best_option`) as closely as
/// sensible so the two "methods" compare cleanly in `BenchReport`, plus Laya's own `confidence`
/// score (not present in the constrained-readout softmax path).
#[derive(Debug, Clone, Serialize)]
pub struct LayaScoreResult {
    pub probs: HashMap<String, f32>,
    pub best_option: String,
    pub confidence: f32,
}

/// Errors from the `laya serve` HTTP call. Deliberately distinct from `ModelsError`: Laya being
/// unreachable is an expected, gracefully-degradable condition (`laya serve` may be down or not
/// yet started), not a fatal model-registry/download error like the rest of this crate.
#[derive(Debug)]
pub enum LayaError {
    /// Connection refused/timeout/DNS failure reaching `laya serve` — the server process is down
    /// or hasn't finished starting.
    Unreachable(String),
    /// `laya serve` responded but the body didn't parse as expected (unexpected shape/missing
    /// `answers.answer`).
    BadResponse(String),
}

impl fmt::Display for LayaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LayaError::Unreachable(e) => write!(f, "laya serve unreachable: {e}"),
            LayaError::BadResponse(e) => write!(f, "laya serve returned an unexpected response: {e}"),
        }
    }
}

impl std::error::Error for LayaError {}

#[derive(Deserialize)]
struct DecideResponse {
    answers: HashMap<String, AnswerEntry>,
}

#[derive(Deserialize)]
struct AnswerEntry {
    choice: Option<String>,
    probabilities: Option<HashMap<String, f32>>,
    confidence: Option<f32>,
}

/// Scores `prompt` against `options` via a single `choice`-type question sent to the already-
/// running `laya serve` process. Blocking (matches the rest of this crate/`pipeline`, which are
/// synchronous). Timeout is generous (30s) since the first request after `laya serve` starts up
/// can include one-time graph-allocation overhead (`ggml_gallocr_needs_realloc`, observed on the
/// real server); steady-state warm calls are much faster.
pub fn score(prompt: &str, options: &[String]) -> Result<LayaScoreResult, LayaError> {
    let mut _s = timing::perf_span!("models::laya::score");
    _s.set("n_options", options.len().to_string());
    let mut criteria = serde_json::Map::new();
    for opt in options {
        criteria.insert(opt.clone(), serde_json::Value::Null);
    }
    let body = serde_json::json!({
        "state": prompt,
        "model": "laya-english",
        "questions": {
            "answer": {
                "type": "choice",
                "instructions": "Pick the correct option.",
                "criteria": criteria,
            }
        }
    });

    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| LayaError::Unreachable(e.to_string()))?;

    let resp = client
        .post(LAYA_SERVE_URL)
        .json(&body)
        .send()
        .map_err(|e| LayaError::Unreachable(e.to_string()))?;

    if !resp.status().is_success() {
        return Err(LayaError::BadResponse(format!(
            "HTTP {}",
            resp.status()
        )));
    }

    let parsed: DecideResponse = resp
        .json()
        .map_err(|e| LayaError::BadResponse(e.to_string()))?;

    let answer = parsed
        .answers
        .get("answer")
        .ok_or_else(|| LayaError::BadResponse("missing 'answers.answer' in laya response".to_string()))?;

    let probs = answer.probabilities.clone().unwrap_or_default();
    let best_option = answer
        .choice
        .clone()
        .ok_or_else(|| LayaError::BadResponse("missing 'answers.answer.choice'".to_string()))?;
    let confidence = answer.confidence.unwrap_or(0.0);

    Ok(LayaScoreResult {
        probs,
        best_option,
        confidence,
    })
}
