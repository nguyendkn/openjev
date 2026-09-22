// apps/server HTTP layer: DTOs, AppState (lazy-load + cache Engine per model), router,
// bench_handler. Reuses crates::{engine,models,pipeline,timing} exactly as apps/cli does —
// same reset_context ordering, same field names in the response, so the two surfaces agree.
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use engine::{Engine, EngineConfig};
use pipeline::{run_generate, run_laya, run_readout, GenerateResult, ReadoutResult};
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
    /// When true, skips the Laya comparison method — `laya: null`, both `laya_*` timing fields
    /// stay 0. Defaults to `false` (Laya IS called) when omitted, matching `apps/cli`'s
    /// `--skip-laya` flag default.
    #[serde(default)]
    pub skip_laya: bool,
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
    pub laya: Option<models::LayaScoreResult>,
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
        let (tx, rx) = mpsc::channel::<WorkerJob>(32);
        std::thread::spawn(move || supervisor_loop(rx));
        Self { tx }
    }
}

/// G13 fix: owns the dedicated engine-worker thread's *outer* loop and recovers it from a
/// panic instead of letting the thread (and the `mpsc::Receiver` it owns) die permanently.
///
/// Design choice — respawn-with-empty-cache, not per-job `catch_unwind` around a
/// long-lived `HashMap<String, Engine>`: `Engine` (`crates/engine`) is a self-referential
/// struct built with `unsafe` lifetime-widening (`LlamaContext<'static>` borrowing from a
/// heap-boxed `LlamaModel` via `std::mem::transmute`), wrapping raw `llama-cpp-2`/FFI state.
/// `std::panic::catch_unwind` only guarantees the *Rust* stack unwinds safely and no memory
/// is freed twice/UB is triggered at the Rust level — it does NOT guarantee that whatever
/// `Engine`'s FFI calls (`ctx.decode`, which crosses into llama.cpp's C layer and mutates
/// its internal KV-cache/position bookkeeping) were doing at the moment of the panic left
/// that FFI-side state in a form that's still correct to keep using. There is no documented
/// `UnwindSafe`/panic-safety audit of `llama-cpp-2`'s internals to lean on here, and getting
/// this wrong would fail silently (a corrupted `Engine` could keep answering requests with
/// subtly wrong results instead of erroring), which is worse than the extra reload cost of
/// respawning. So: `engines` is created fresh *inside* `worker_loop` on every (re)entry: if
/// `worker_loop` panics, `catch_unwind` catches it here, the panicking stack frame's entire
/// `engines: HashMap<String, Engine>` is dropped as part of unwinding (discarding every
/// cached model, not just the implicated one), and the loop below immediately calls
/// `worker_loop` again with a brand-new empty cache — self-healing within the same process,
/// no operator restart needed, at the cost of one reload per model on next use. `rx` itself
/// is untouched by the panic (it only ever produces `Some(job)`/`None` before `run_bench` is
/// called, which is where a hypothetical panic would occur) and is safe to keep reusing
/// across respawns; it is threaded through via `AssertUnwindSafe` since `&mut Receiver` is
/// not `UnwindSafe` by default (that trait errs conservatively for any `&mut`, not because
/// this specific type is actually at risk here).
///
/// The panicking request itself still gets a clean error, not a hang: its `WorkerJob::resp`
/// (a `oneshot::Sender`) is a value local to the panicking `worker_loop` call and is dropped
/// during unwinding without ever calling `.send(..)`; `bench_handler`'s `resp_rx.await` then
/// resolves to `Err`, which it already maps to `ApiError::Internal("engine worker thread
/// dropped the response")` (HTTP 500) — no code change needed there, this was already
/// correct given how `oneshot` channels signal a dropped sender.
fn supervisor_loop(mut rx: mpsc::Receiver<WorkerJob>) {
    loop {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            worker_loop(&mut rx)
        }));
        match outcome {
            Ok(()) => return, // rx closed normally (all Senders/AppState clones dropped)
            Err(payload) => {
                let msg = panic_message(&*payload);
                eprintln!(
                    "engine worker thread panicked, respawning with an empty model cache: {msg}"
                );
                // loop continues: worker_loop is called again with a fresh HashMap below.
            }
        }
    }
}

/// Best-effort extraction of a panic's message for logging (`std::panic::PanicHookInfo`/the
/// `catch_unwind` payload is `Box<dyn Any + Send>`; the two conventional payload shapes are
/// `&str` for `panic!("literal")` and `String` for `panic!("{}", formatted)`).
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

/// One worker-thread lifetime's worth of job processing, from an empty model cache until the
/// channel closes or a panic unwinds out of here (see `supervisor_loop`).
fn worker_loop(rx: &mut mpsc::Receiver<WorkerJob>) {
    let mut engines: HashMap<String, Engine> = HashMap::new();
    while let Some(job) = rx.blocking_recv() {
        let result = run_bench(&mut engines, job.req);
        let _ = job.resp.send(result);
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
            // Not expected to ever reach here: `run_bench` handles `run_laya`'s `Result`
            // directly (matching `Ok`/`Err` inline, never using `?`) so a Laya failure degrades
            // to `laya: None` instead of becoming an `ApiError`. Mapped anyway for exhaustiveness
            // and defense-in-depth if `run_laya` is ever called with `?` elsewhere.
            pipeline::PipelineError::Laya(m) => ApiError::Internal(m),
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

    // Laya (3rd comparison method): calls the separately-running `laya serve` process (see
    // `pipeline::laya::run_laya`). `laya_model_load_ms` stays 0 (model loads once at `laya
    // serve` startup, not per-request). Unreachable/erroring Laya degrades gracefully — the
    // readout/generate results above already succeeded, so a Laya failure must not 500 the
    // whole request; `laya` stays `None` and the failure is only logged.
    let laya = if req.skip_laya {
        None
    } else {
        let t0 = Instant::now();
        let result = run_laya(&req.prompt, &req.options);
        timings.laya_inference_ms = t0.elapsed().as_millis();
        match result {
            Ok(laya) => Some(laya),
            Err(e) => {
                eprintln!("laya unavailable, continuing without it: {e}");
                None
            }
        }
    };

    Ok(BenchReport {
        model: req.model,
        prompt: req.prompt,
        options: req.options,
        timings,
        readout,
        generate,
        laya,
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
