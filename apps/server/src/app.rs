// apps/server HTTP layer: DTOs, AppState (lazy-load + cache Engine per model), router,
// bench_handler. Reuses crates::{engine,models,pipeline,timing} exactly as apps/cli does —
// same reset_context ordering, same field names in the response, so the two surfaces agree.
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use engine::{Engine, EngineConfig};
use pipeline::{run_generate, run_readout, GenerateResult, ReadoutResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;
use timing::Timings;
use tokio::sync::{mpsc, oneshot};

/// Mirrors `apps/cli`'s `--options` semantics: a flat list of option labels, e.g. `["A","B"]`.
#[derive(Debug, Deserialize)]
pub struct BenchRequest {
    pub model: String,
    pub prompt: String,
    pub options: Vec<String>,
}

/// Same field names/shape as `apps/cli`'s `CliOutput` JSON so both surfaces agree.
#[derive(Debug, Serialize)]
pub struct BenchReport {
    pub model: String,
    pub prompt: String,
    pub options: Vec<String>,
    pub timings: Timings,
    pub readout: ReadoutResult,
    pub generate: GenerateResult,
}

/// One benchmark job handed to the dedicated engine-worker thread.
struct WorkerJob {
    req: BenchRequest,
    resp: oneshot::Sender<Result<BenchReport, ApiError>>,
}

/// `Engine` wraps raw `llama-cpp-2`/FFI pointers (`NonNull<llama_context>`, raw sampler
/// pointers) and is therefore `!Send` — confirmed by a real compile error when this was first
/// attempted as `Arc<Mutex<HashMap<String, Engine>>>` shared across `tokio::task::spawn_blocking`
/// closures (spawn_blocking requires `F: Send + 'static`, which a `HashMap<String, Engine>`
/// can never satisfy). Since `Engine` can never cross a thread boundary, model caching+access is
/// instead confined to one dedicated OS thread ("engine worker") for its entire lifetime — the
/// actor/channel pattern the reference plan itself names as the fallback design (R2 §2
/// alternative) when Mutex+spawn_blocking isn't viable. `AppState` only holds an
/// `mpsc::Sender<WorkerJob>` (Send+Sync+Clone), so the async handler never touches `Engine`
/// directly and never blocks inline — it sends a job and awaits a `oneshot` reply while the
/// worker thread does the actual (possibly slow) inference work. Requests still fully serialize
/// (one worker thread, one job at a time), same effective guarantee as the Mutex design.
#[derive(Clone)]
pub struct AppState {
    tx: mpsc::Sender<WorkerJob>,
}

impl AppState {
    pub fn new() -> Self {
        let (tx, mut rx) = mpsc::channel::<WorkerJob>(32);
        std::thread::spawn(move || {
            let mut engines: HashMap<String, Engine> = HashMap::new();
            while let Some(job) = rx.blocking_recv() {
                let result = run_bench(&mut engines, job.req);
                let _ = job.resp.send(result);
            }
        });
        Self { tx }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Errors surfaced to HTTP clients as non-2xx JSON bodies. Never a raw panic/500-with-no-body.
#[derive(Debug)]
pub enum ApiError {
    UnknownModel(String),
    InvalidOptions(String),
    Engine(String),
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            ApiError::UnknownModel(m) => (StatusCode::BAD_REQUEST, format!("unknown model: {m}")),
            ApiError::InvalidOptions(m) => (StatusCode::BAD_REQUEST, m),
            ApiError::Engine(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
            ApiError::Internal(m) => (StatusCode::INTERNAL_SERVER_ERROR, m),
        };
        (status, Json(serde_json::json!({ "error": message }))).into_response()
    }
}

impl From<models::ModelsError> for ApiError {
    fn from(e: models::ModelsError) -> Self {
        match e {
            models::ModelsError::UnknownModel(id) => ApiError::UnknownModel(id),
            other => ApiError::Internal(other.to_string()),
        }
    }
}

impl From<engine::error::EngineError> for ApiError {
    fn from(e: engine::error::EngineError) -> Self {
        ApiError::Engine(e.to_string())
    }
}

impl From<pipeline::PipelineError> for ApiError {
    fn from(e: pipeline::PipelineError) -> Self {
        match e {
            pipeline::PipelineError::InvalidOptions(m) => ApiError::InvalidOptions(m),
            pipeline::PipelineError::Engine(m) => ApiError::Engine(m),
        }
    }
}

async fn health() -> StatusCode {
    StatusCode::OK
}

/// Runs one full readout+generate benchmark pass for `req` against `engines` (the engine
/// worker's local, thread-confined model cache), ensuring the model is loaded+cached first.
/// Pure blocking work — only ever called on the dedicated engine-worker thread, never inline in
/// an async handler (see `AppState` doc comment for why this replaces `spawn_blocking` here).
fn run_bench(engines: &mut HashMap<String, Engine>, req: BenchRequest) -> Result<BenchReport, ApiError> {
    if req.options.is_empty() {
        return Err(ApiError::InvalidOptions(
            "options must list at least one option, e.g. [\"A\",\"B\"]".to_string(),
        ));
    }

    let mut timings = Timings::default();

    if !engines.contains_key(&req.model) {
        let t0 = Instant::now();
        let entry = models::find(&req.model)?;
        let model_path = models::ensure_downloaded(entry)?;
        let mut engine = Engine::load(&model_path, EngineConfig::default())?;
        timings.model_load_ms = t0.elapsed().as_millis();

        // Warmup, same as apps/cli: one throwaway decode + reset so it isn't baked into
        // constrained_readout_ms on this model's first-ever request.
        let t0 = Instant::now();
        let warmup_tokens = engine.tokenize("Hello")?;
        engine.decode_prompt(&warmup_tokens)?;
        engine.reset_context();
        timings.warmup_ms = t0.elapsed().as_millis();

        engines.insert(req.model.clone(), engine);
    } else {
        // Model already cached: no model-load/warmup timing charged to this request.
        let _ = models::find(&req.model)?; // still validate the id even on cache hit
    }

    let engine = engines.get_mut(&req.model).expect("just inserted or present");

    // Unlike apps/cli (one process per run), this Engine is cached and reused across separate
    // HTTP requests. Reset the KV cache before starting this request's work so a previous
    // request's leftover generate-loop state (which is never reset after its own last decode)
    // can't contaminate this request's readout — same isolation requirement as the
    // readout/generate reset below, just applied at the request boundary too.
    engine.reset_context();

    let t0 = Instant::now();
    let _ = engine.tokenize(&req.prompt)?;
    timings.tokenize_ms = t0.elapsed().as_millis();

    let t0 = Instant::now();
    let readout = run_readout(engine, &req.prompt, &req.options)?;
    timings.constrained_readout_ms = t0.elapsed().as_millis();

    // MUST reset between the two independent pipeline runs, same as apps/cli.
    engine.reset_context();

    let t0 = Instant::now();
    let generate = run_generate(engine, &req.prompt, &req.options)?;
    timings.generation_ms = t0.elapsed().as_millis();

    Ok(BenchReport {
        model: req.model,
        prompt: req.prompt,
        options: req.options,
        timings,
        readout,
        generate,
    })
}

pub async fn bench_handler(
    State(state): State<AppState>,
    Json(req): Json<BenchRequest>,
) -> Result<Json<BenchReport>, ApiError> {
    let (resp_tx, resp_rx) = oneshot::channel();
    state
        .tx
        .send(WorkerJob { req, resp: resp_tx })
        .await
        .map_err(|_| ApiError::Internal("engine worker thread unavailable".to_string()))?;
    let result = resp_rx
        .await
        .map_err(|_| ApiError::Internal("engine worker thread dropped the response".to_string()))?;
    result.map(Json)
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/bench", post(bench_handler))
        .with_state(state)
}
