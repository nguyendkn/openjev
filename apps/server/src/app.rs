// apps/server HTTP layer: DTOs, AppState (a pool of engine workers), router, bench_handler.
// Reuses crates::{engine,models,pipeline,timing} exactly as apps/cli does — same
// reset_context ordering, same field names in the response, so the two surfaces agree.
// The worker pool itself (threads, G13 supervisors, routing policy) lives in `pool.rs`.
use crate::pool::Pool;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json, Response};
use axum::routing::{get, post};
use axum::Router;
use engine::{Engine, EngineConfig};
use pipeline::{run_generate, run_laya, run_readout, GenerateResult, ReadoutResult};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;
use timing::Timings;

/// The only model ids this server will run through `Engine`/llama.cpp.
///
/// Deliberately NOT the full `models::REGISTRY`: the registry also contains a `laya` entry,
/// which exists so `pipeline::laya` knows which GGUF to fetch for the separate `laya serve`
/// process. That file is a ggmlc-format model, not a llama.cpp-compatible LLM, so accepting
/// `{"model":"laya"}` here used to reach `Engine::load` and fail with a confusing HTTP 500
/// (`model load failed: null result from llama cpp`). Laya is never selected via `model`
/// anyway — it is always run as the 3rd comparison method and returned in the `laya` response
/// field unless `skip_laya` is set — so it is rejected up front with a 400 instead.
pub const SERVER_MODELS: [&str; 3] = ["qwen3-0.6b", "qwen3-4b", "minicpm5-2b"];

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
    /// Loop 21 Task 2: opt-in subset of `"readout"`/`"generate"`/`"laya"` to actually run.
    /// `None` (the field omitted entirely, the pre-Loop-21 shape) means "all three" — the
    /// exact behavior of every request before this field existed, byte-identical response.
    /// `Some(list)` skips the *engine/HTTP work itself* for any method not named, not just the
    /// response field (see [`MethodSet::resolve`]/`run_bench`) — this is what makes a
    /// `methods: ["laya"]` request return in ~100-200ms instead of paying the full sequential
    /// readout+generate tax first.
    #[serde(default)]
    pub methods: Option<Vec<String>>,
}

/// Resolved from [`BenchRequest::methods`] (and `skip_laya`) once per request — which of the
/// 3 comparison methods `run_bench` should actually execute.
struct MethodSet {
    readout: bool,
    generate: bool,
    laya: bool,
}

impl MethodSet {
    /// `methods: None` -> all 3 (today's exact behavior, the backward-compat default).
    /// `methods: Some([...])` -> only the named methods; unknown entries or an empty/
    /// all-excluded set are rejected as `400 InvalidOptions` rather than silently running
    /// nothing. `skip_laya: true` always wins over a `methods` list that names `"laya"` — one
    /// authoritative "don't call laya" switch instead of two that could disagree.
    fn resolve(methods: &Option<Vec<String>>, skip_laya: bool) -> Result<Self, ApiError> {
        let mut set = match methods {
            None => MethodSet {
                readout: true,
                generate: true,
                laya: true,
            },
            Some(list) => {
                let mut s = MethodSet {
                    readout: false,
                    generate: false,
                    laya: false,
                };
                for m in list {
                    match m.as_str() {
                        "readout" => s.readout = true,
                        "generate" => s.generate = true,
                        "laya" => s.laya = true,
                        other => {
                            return Err(ApiError::InvalidOptions(format!(
                                "unknown methods entry '{other}' — valid values: \
                                 readout, generate, laya"
                            )))
                        }
                    }
                }
                if !s.readout && !s.generate && !s.laya {
                    return Err(ApiError::InvalidOptions(
                        "methods, if provided, must name at least one of: readout, generate, \
                         laya"
                            .to_string(),
                    ));
                }
                s
            }
        };
        if skip_laya {
            set.laya = false;
        }
        Ok(set)
    }
}

/// Same field names/shape as `apps/cli`'s `CliOutput` JSON so both surfaces agree.
///
/// `readout`/`generate` became `Option` in Loop 21 (Task 2, method selection) — when a method
/// is skipped its field serializes as JSON `null`, exactly like `laya` already did for
/// `skip_laya`. When every method runs (the default, `methods` omitted), `Option::Some(x)`
/// serializes identically to bare `x`, so the default response shape is unchanged.
#[derive(Debug, Serialize)]
pub struct BenchReport {
    pub model: String,
    pub prompt: String,
    pub options: Vec<String>,
    pub timings: Timings,
    pub readout: Option<ReadoutResult>,
    pub generate: Option<GenerateResult>,
    pub laya: Option<models::LayaScoreResult>,
}

/// `Engine` wraps raw `llama-cpp-2`/FFI pointers (`NonNull<llama_context>`, raw sampler
/// pointers) and is therefore `!Send` — confirmed by a real compile error when this was first
/// attempted as `Arc<Mutex<HashMap<String, Engine>>>` shared across `tokio::task::spawn_blocking`
/// closures (spawn_blocking requires `F: Send + 'static`, which a `HashMap<String, Engine>`
/// can never satisfy). Since `Engine` can never cross a thread boundary, model caching+access is
/// confined to dedicated OS threads ("engine workers") for their entire lifetime — the
/// actor/channel pattern the reference plan itself names as the fallback design (R2 §2
/// alternative) when Mutex+spawn_blocking isn't viable.
///
/// Loop 14: there is now a *pool* of N such worker threads instead of exactly one, so
/// independent concurrent requests run in parallel on the 32-core box instead of queueing
/// behind a single serialized worker (measured: 1 worker x 28 threads = 0.47 req/s at every
/// concurrency level, i.e. perfectly flat/serialized; 4 workers x 7 threads = 0.90 req/s at
/// concurrency 4). `AppState` still only holds `Send + Sync + Clone` handles (channel senders
/// behind an `Arc<Pool>`), so the async handler never touches `Engine` directly and never
/// blocks inline — it routes a job to a worker and awaits a `oneshot` reply while that worker
/// thread does the actual (slow) inference work. See `pool.rs` for the routing policy and the
/// per-worker G13 respawn supervisors.
#[derive(Clone)]
pub struct AppState {
    pool: Arc<Pool>,
}

impl AppState {
    /// `n_workers` engine-worker threads, each with its own model cache and its own
    /// `EngineConfig { n_threads, n_batch, n_gpu_layers }` (so total CPU demand is
    /// `n_workers * n_threads` — keep that at/below the core count). `n_threads`/`n_batch`/
    /// `n_gpu_layers` are startup-time values (Loop 6/7: `--threads`/`--batch`; `--gpu-layers`
    /// for GPU offload, only has an effect on a `cuda`-feature build), `n_workers` is Loop
    /// 14's `--workers`.
    pub fn new(n_workers: usize, n_threads: i32, n_batch: u32, n_gpu_layers: i32) -> Self {
        Self {
            pool: Arc::new(Pool::new(n_workers, n_threads, n_batch, n_gpu_layers)),
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        let defaults = EngineConfig::default();
        Self::new(
            crate::pool::DEFAULT_WORKERS,
            defaults.n_threads,
            defaults.n_batch,
            defaults.n_gpu_layers,
        )
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
            ApiError::UnknownModel(m) => (
                StatusCode::BAD_REQUEST,
                format!(
                    "unknown model '{m}' — valid models: {}. Laya is always included \
                     automatically in the 'laya' field of the response unless skip_laya is set.",
                    SERVER_MODELS.join(", ")
                ),
            ),
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

/// Rejects anything that isn't one of the three llama.cpp-compatible LLMs — notably `laya`
/// (see [`SERVER_MODELS`]) — with a 400 *before* the request is ever routed to a worker, so a
/// bad id can never occupy a worker or reach `Engine::load`.
pub fn validate_model(model: &str) -> Result<(), ApiError> {
    if SERVER_MODELS.contains(&model) {
        Ok(())
    } else {
        Err(ApiError::UnknownModel(model.to_string()))
    }
}

async fn health() -> StatusCode {
    StatusCode::OK
}

/// Runs `req`'s requested subset of the 3 comparison methods (readout/generate/laya, all 3 by
/// default — see [`MethodSet::resolve`]) against `engines` (one engine worker's local,
/// thread-confined model cache), ensuring the model is loaded+cached first if the LLM engine is
/// needed at all. Pure blocking work — only ever called on an engine-worker thread, never
/// inline in an async handler (see `AppState`'s doc comment for why this replaces
/// `spawn_blocking` here).
pub(crate) fn run_bench(
    engines: &mut HashMap<String, Engine>,
    req: BenchRequest,
    n_threads: i32,
    n_batch: u32,
    n_gpu_layers: i32,
) -> Result<BenchReport, ApiError> {
    let mut _s = timing::perf_span!("server::run_bench");
    _s.set("model", req.model.clone());
    // Defense in depth: `bench_handler` already validated this before dispatch, but
    // `run_bench` is the only thing that can reach `Engine::load`, so it re-checks rather
    // than trusting its caller.
    validate_model(&req.model)?;
    if req.options.is_empty() {
        return Err(ApiError::InvalidOptions(
            "options must list at least one option, e.g. [\"A\",\"B\"]".to_string(),
        ));
    }
    let methods = MethodSet::resolve(&req.methods, req.skip_laya)?;

    let mut timings = Timings::default();

    // Hoisted above the cache check (Loop 19): `run_generate` below needs
    // `entry.suppress_think` regardless of whether this is a cache hit or miss, whereas
    // before Loop 19 only the cache-miss branch needed `entry` at all.
    let entry = models::find(&req.model)?;

    // Loop 21 Task 3: spawn Laya's HTTP round-trip on its own OS thread as early as possible
    // (before any of the LLM engine work below), so it overlaps with readout+generate instead
    // of paying its ~100-150ms strictly after them. Safe under G13's actor model: `Engine` is
    // the only `!Send` state here and never leaves its home worker thread (nothing below
    // touches `engine` from this spawned thread) — `run_laya` is a plain, stateless blocking
    // HTTP client call (`models::laya::score`, confirmed synchronous, not already async) with
    // no shared state, so handing it to an ad hoc `std::thread` is safe. Cloning `prompt`/
    // `options` here (cheap, small strings/labels) is simpler and less risky than threading
    // borrows across the spawn boundary.
    // The closure times its own `run_laya` call and returns the elapsed duration alongside the
    // result — NOT timed as spawn-to-join, since join can now happen well after the HTTP call
    // itself finished (it overlaps the LLM work below) and a spawn-to-join wall-clock would
    // wrongly inflate `laya_inference_ms` to include however long readout+generate took.
    let laya_job = methods.laya.then(|| {
        let prompt = req.prompt.clone();
        let options = req.options.clone();
        std::thread::spawn(move || {
            let t0 = Instant::now();
            let result = run_laya(&prompt, &options);
            (t0.elapsed(), result)
        })
    });

    let readout = if methods.readout || methods.generate {
        if !engines.contains_key(&req.model) {
            let _s = timing::perf_span!("server::ensure_model_loaded");
            let t0 = Instant::now();
            let model_path = models::ensure_downloaded(entry)?;
            let engine_config = EngineConfig {
                n_threads,
                n_batch,
                n_gpu_layers,
                ..EngineConfig::default()
            };
            let mut engine = Engine::load(&model_path, engine_config)?;
            timings.model_load_ms = t0.elapsed().as_millis();

            // Warmup, same as apps/cli: one throwaway decode + reset so it isn't baked into
            // constrained_readout_ms on this model's first-ever request.
            let t0 = Instant::now();
            let warmup_tokens = engine.tokenize("Hello")?;
            engine.decode_prompt(&warmup_tokens)?;
            engine.reset_context();
            timings.warmup_ms = t0.elapsed().as_millis();

            engines.insert(req.model.clone(), engine);
        }

        let engine = engines
            .get_mut(&req.model)
            .expect("just inserted or present");

        // Unlike apps/cli (one process per run), this Engine is cached and reused across
        // separate HTTP requests. Reset the KV cache before starting this request's work so a
        // previous request's leftover generate-loop state (which is never reset after its own
        // last decode) can't contaminate this request's readout — same isolation requirement
        // as the readout/generate reset below, just applied at the request boundary too.
        engine.reset_context();

        let t0 = Instant::now();
        let _ = engine.tokenize(&req.prompt)?;
        timings.tokenize_ms = t0.elapsed().as_millis();

        let readout = if methods.readout {
            let t0 = Instant::now();
            let readout = run_readout(engine, &req.prompt, &req.options)?;
            timings.constrained_readout_ms = t0.elapsed().as_millis();
            Some(readout)
        } else {
            None
        };

        if methods.generate {
            // MUST reset between the two independent pipeline runs, same as apps/cli.
            engine.reset_context();
        }

        readout
    } else {
        // Loop 21 Task 2: a request that wants neither readout nor generate (e.g. a
        // laya-only `methods: ["laya"]` call) never touches `Engine` at all — no model load,
        // no warmup, no tokenize, no decode. This is the real time saved, not just a hidden
        // field: without this branch a laya-only request would still pay the full LLM-engine
        // tax before reaching the (already-running) Laya thread below.
        None
    };

    let generate = if methods.generate {
        let engine = engines
            .get_mut(&req.model)
            .expect("present: the readout||generate branch above just inserted/loaded it");
        let t0 = Instant::now();
        let generate = run_generate(
            engine,
            &req.prompt,
            &req.options,
            entry.suppress_think,
            entry.think_budget,
        )?;
        timings.generation_ms = t0.elapsed().as_millis();
        Some(generate)
    } else {
        None
    };

    // Laya (3rd comparison method): join the thread spawned at the top of this function (see
    // above) instead of calling `run_laya` here — by this point its HTTP round-trip has
    // already been running concurrently with the readout+generate work above, so joining is
    // usually near-instant (the thread finished long ago and is just waiting to be reaped).
    // `laya_inference_ms` is the duration the closure measured around its own `run_laya` call
    // (NOT spawn-to-join wall-clock, which would wrongly include however long readout+generate
    // took) — same real HTTP-round-trip number as before this loop, just no longer serialized
    // onto the critical path. `laya_model_load_ms` stays 0 (model loads once at `laya serve`
    // startup, not per-request). Unreachable/erroring Laya degrades gracefully — the
    // readout/generate results above already succeeded (or were never requested), so a Laya
    // failure must not 500 the whole request; `laya` stays `None` and the failure is only
    // logged.
    let laya = if let Some(handle) = laya_job {
        let (elapsed, result) = handle.join().unwrap_or_else(|_| {
            (
                std::time::Duration::ZERO,
                Err(pipeline::PipelineError::Laya(
                    "laya worker thread panicked".to_string(),
                )),
            )
        });
        timings.laya_inference_ms = elapsed.as_millis();
        match result {
            Ok(laya) => Some(laya),
            Err(e) => {
                eprintln!("laya unavailable, continuing without it: {e}");
                None
            }
        }
    } else {
        None
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
    // Validate before routing: an unknown/non-LLM model id must not consume a worker slot.
    validate_model(&req.model)?;
    state.pool.dispatch(req).await.map(Json)
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/bench", post(bench_handler))
        .with_state(state)
}
