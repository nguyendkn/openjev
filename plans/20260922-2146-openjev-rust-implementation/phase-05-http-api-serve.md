---
phase: 5
name: apps/server (openjev-server binary)
status: pending
depends_on: [2, 3]
---

## Context Links
- `../plan.md` § Architecture (`apps/server`)
- `research/researcher-02-cli-http-architecture.md` §2 (axum + blocking pattern, context sharing)
- `phase-04-cli-bench.md` (parallel phase, shares `pipeline`/`engine`/`models`/`timing` crate logic
  but NO files — fully separate binary)

## Overview
- Priority: P1, parallel with Phase 4 — **fully disjoint now** (`apps/server` vs `apps/cli` are
  separate packages, no shared files at all; both depend on Phase 2+3).
- Status: pending.
- Implement `openjev-server` — a standalone axum HTTP server binary exposing the same 3-way
  benchmark (constrained readout, JSON generation, Laya scoring) as `apps/cli`, per plan.md
  Decisions Locked #1 (final): `cli` and `server` are two fully separate binaries sharing library
  crates, not one binary with subcommands.

## Key Insights
- `LlamaContext`/model handle is `!Send`/`!Sync` (R2 §2, github.com/utilityai/llama-cpp-rs #483) —
  cannot share a bare `Arc<Engine>` across axum's async tasks. Chosen pattern (per plan.md
  Architecture, R2 §2 recommendation): `Arc<Mutex<Engine>>` singleton, cloned `Arc` into each
  `spawn_blocking` closure, lock before use. Simpler than a worker+channel actor and correct for
  this use case since benchmark runs are inherently sequential (one model context, one run at a
  time) — R2 explicitly calls out Mutex+spawn_blocking as sufficient here.
- Never call blocking inference code directly inside an async handler — always via
  `tokio::task::spawn_blocking` (R2 §2 canonical pattern); mixing causes thread-pool starvation.
- Watch the caveat from R2 §2 (users.rust-lang.org thread): naive `spawn_blocking` alone can still
  starve axum's worker threads under load if overused — for v1's single-context-serialized-by-Mutex
  design this is a lesser risk (only one inference runs at a time anyway), but still route every
  inference call through `spawn_blocking`, never inline in the handler.
- Because this is now a standalone binary (no CLI subcommand dispatch to coordinate with), `main.rs`
  is purely: parse `--port`, build `AppState`, build the router, `axum::serve`. No `Command` enum,
  no shared-file coordination with Phase 4 at all — a genuine simplification vs the earlier
  single-binary design.

## Requirements
### Functional
- `openjev-server --port <p>`: starts an axum server. At minimum one endpoint (`POST /bench`)
  accepting `{model, prompt, options, threads?, batch_size?, skip_laya?}` JSON (`skip_laya`
  defaults to `false` — Laya runs by default, same as `apps/cli`), returning the same combined
  `Timings` + `ReadoutResult` + `GenerateResult` + `laya: Option<Result<LayaResult, String>>` report
  shape as `apps/cli`'s `--format json` output (keep the two response shapes consistent — same
  underlying report type, different transport).
- Server holds one `Arc<Mutex<Engine>>` per loaded model (or a single active model swapped on
  request — decide at implementation time based on whether concurrent multi-model requests are
  needed; default to lazy-load-on-first-request per model, cached thereafter, since v1 doesn't
  require concurrent multi-model serving per R2 Unresolved #3).
- Non-2xx JSON error responses for invalid input (unknown model, malformed request body) —
  never a raw panic/500 with no body. A Laya-specific failure (e.g. `ggmlc-run` missing) with
  `skip_laya: false` is NOT a request failure — the response is still 200 with `readout`/`generate`
  populated and `laya: {"Err": "..."}`, same isolated-failure pattern as `apps/cli`.

### Non-functional
- Every inference call wrapped in `spawn_blocking`; no blocking call inline in an async fn.
- Server starts and binds the requested port; graceful error if port already in use.

## Architecture
```
apps/server/src/args.rs
  #[derive(Parser)] pub struct Cli { pub port: u16, ... }
apps/server/src/app.rs
  pub struct BenchRequest { pub model: ModelChoice, pub prompt: String, pub options: Vec<String>, pub threads: Option<usize>, pub batch_size: Option<usize>, #[serde(default)] pub skip_laya: bool }
  pub struct BenchReport { pub timings: timing::Timings, pub readout: pipeline::readout::ReadoutResult,
      pub generate: pipeline::generate::GenerateResult, pub laya: Option<Result<pipeline::laya::LayaResult, String>> }
  pub struct AppState { pub engines: Arc<Mutex<HashMap<ModelChoice, engine::Engine>>> }  // lazy per-model load+cache
  pub fn router(state: AppState) -> axum::Router
  pub async fn bench_handler(State(state): State<AppState>, Json(req): Json<BenchRequest>) -> Result<Json<BenchReport>, ApiError>
      // body: tokio::task::spawn_blocking(move || { lock state.engines; ensure loaded; run_readout; reset_context; run_generate;
      //   if !req.skip_laya { run_laya isolated, error -> BenchReport.laya = Some(Err(..)) } }).await
apps/server/src/main.rs
  #[tokio::main] async fn main() { let cli = Cli::parse(); let state = AppState::new(); let app = app::router(state); axum::serve(listener, app).await }
```

## Related Code Files
- CREATE `apps/server/src/args.rs` — flat `Cli { port: u16 }` clap struct (no subcommand — this
  binary's only job is "serve").
- CREATE `apps/server/src/app.rs` — `AppState`, `router`, `bench_handler`, `BenchRequest`/
  `BenchReport` DTOs, `ApiError` (implements `IntoResponse`).
- MODIFY `apps/server/src/main.rs` — `#[tokio::main] async fn main()`: parse `Cli`, build
  `AppState`, build router via `app::router`, bind `TcpListener`, `axum::serve` (fills in Phase 0's
  trivial stub).
- MODIFY `apps/server/Cargo.toml` — confirm `engine`/`models`/`pipeline`/`timing` path deps +
  `axum`/`tokio` (`rt-multi-thread`, `macros` features)/`clap`/`serde_json` present (scaffolded by
  Phase 0, verify not missing).

## Implementation Steps
1. Define `BenchRequest { model: ModelChoice, prompt: String, options: Vec<String>, threads:
   Option<usize>, batch_size: Option<usize>, #[serde(default)] skip_laya: bool }` and `BenchReport
   { timings: timing::Timings, readout: pipeline::readout::ReadoutResult, generate:
   pipeline::generate::GenerateResult, laya: Option<Result<pipeline::laya::LayaResult, String>> }`
   in `apps/server/src/app.rs` (reuse Phase 2/3's types directly, don't duplicate).
2. Define `ApiError` enum implementing `axum::response::IntoResponse` — maps `EngineError`/
   `PipelineError`/`ModelError` to appropriate HTTP status + JSON error body (e.g. 400 for bad
   model/request shape, 500 for engine/decode failure). Note: a Laya-specific failure does NOT map
   to an `ApiError` — it's captured into `BenchReport.laya` instead (see step 4), the request still
   succeeds.
3. Implement `AppState` with `Arc<Mutex<HashMap<ModelChoice, engine::Engine>>>` —
   lazy-load-on-first-use per model (mirrors Phase 1's `models::ensure_downloaded`/
   `engine::Engine::load_from_spec`, cached in the map after first load to avoid reloading per
   request).
4. Implement `bench_handler`: clone `Arc`, `spawn_blocking` a closure that locks the mutex, ensures
   the requested model is loaded (load-and-insert if not cached, using `EngineConfig` built from
   the request's `threads`/`batch_size`), runs warmup only on first load, runs `run_readout`, then
   `Engine::reset_context` (Phase 1 primitive — same KV-cache-isolation requirement as Phase 4's
   CLI path), then `run_generate`. If `!req.skip_laya`: resolve the Laya-en spec,
   `models::laya::LayaRunner::load_from_spec` + `pipeline::laya::run_laya`, wrapped so any `Err`
   becomes `BenchReport.laya = Some(Err(err.to_string()))` rather than propagating to `ApiError`
   (same isolated-failure pattern as Phase 4's CLI — Laya's optionality means its failure is data,
   not a request error). Builds `Timings`, returns `BenchReport`; `.await` the `spawn_blocking`
   handle, map `JoinError`/inner `Result` to `ApiError` (only for the readout/generate/model-load
   path, never for Laya).
5. Implement `router(state)` wiring `POST /bench` to `bench_handler`, plus a basic `GET /health` for
   liveness (small addition, matches standard practice, not scope creep — health checks are
   near-universal for any HTTP service and trivial to add).
6. Implement `main.rs`: `#[tokio::main] async fn main()` — parse `Cli` (`--port`), build `AppState`
   (empty engine cache), build `router`, bind `TcpListener` on the requested port,
   `axum::serve(listener, router).await`; graceful error message + non-zero exit on bind failure.

## Todo List
- [ ] `BenchRequest`/`BenchReport`/`ApiError` DTOs defined, reusing Phase 2/3 types, incl.
      `skip_laya`/`laya` fields
- [ ] `AppState` with `Arc<Mutex<HashMap<ModelChoice, engine::Engine>>>`, lazy-load-and-cache
- [ ] `bench_handler` wraps all inference in `spawn_blocking`, no inline blocking call, calls
      `Engine::reset_context` between `run_readout` and `run_generate`, isolates Laya failures into
      `BenchReport.laya` instead of `ApiError`
- [ ] `router` wires `POST /bench` + `GET /health`
- [ ] `main.rs` parses `--port`, starts server, handles bind failure gracefully

## Success Criteria
- `openjev-server --port 8080` starts; `curl -X POST localhost:8080/bench -d '{"model":
  "qwen3-0.6b", "prompt": "...", "options": ["A","B"]}'` returns a `BenchReport` JSON (with `laya`
  populated by default) matching `apps/cli`'s `--format json` output shape (same fields, same
  semantics).
- Same request with `"skip_laya": true` returns `laya: null` and both Laya timing fields `null`.
- `GET /health` returns 200 while the server is up.
- Two concurrent requests do not corrupt shared state or crash the server (Mutex serializes
  correctly — verify via 2 near-simultaneous curl calls, second should simply wait for the lock,
  not error).
- Invalid model name in the request body returns a 4xx JSON error, not a 500/panic.
- A missing/broken `ggmlc-run` binary does NOT turn a request into a 4xx/5xx when `skip_laya` is
  false — response is still 200 with `laya` reporting the error.

## Test Strategy & Quality Gate
Lane: normal.
- Spec: Goal = expose the same 3-way benchmark as an HTTP JSON API, safely serializing concurrent
  access to the shared `!Send`/`!Sync` engine, and degrading gracefully when only the optional
  Laya path fails. AC: (1) Given a valid `POST /bench` body and a cached/loadable model, When
  handled, Then 200 + `BenchReport` JSON with all `Timings` fields and all 3 results. (2) Given an
  unknown model name, When handled, Then 4xx JSON error, no panic. (3) Given 2 concurrent requests,
  When both hit `/bench`, Then both complete successfully in sequence (no deadlock, no data race)
  — verified via a test issuing 2 requests via `tokio::join!` against a test server instance. (4)
  `GET /health` returns 200. (5) Given `"skip_laya": true`, When handled, Then `laya` is `null` and
  both Laya timing fields are `null`. (6) Given a Laya-scoring failure (mocked) and `skip_laya:
  false`, When handled, Then the response is still 200 with `laya` reporting the error, not a
  4xx/5xx. I/O contract: JSON request/response schemas per Architecture DTOs. Out-of-scope:
  authentication/rate-limiting (not requested, no auth flag in this project), horizontal
  scaling/multi-instance state, `models::laya`'s own subprocess correctness (Phase 1).
- Pyramid ~60/25/15 (HTTP layer needs meaningfully more integration coverage than CLI given
  concurrency correctness is the main risk): unit tests for `ApiError` status-code mapping, DTO
  (de)serialization (incl. `skip_laya` default), and the Laya-failure-isolation behavior (AC #6,
  mockable); integration tests using axum's `tower::ServiceExt::oneshot` or a real bound
  `TestServer` hitting `/health` and `/bench` (feature-gated where `/bench` needs a real model);
  e2e scenario (mandatory per quality-gate §4): "start `openjev-server`, POST /bench with a real
  0.6B model request, receive valid BenchReport JSON with all 3 results" — plus the concurrency
  scenario (2 near-simultaneous requests both succeed) as a second integration case since
  concurrent-safety is this phase's specific new risk surface beyond Phase 4's CLI.
- Coverage target: ≥90% line / ≥75% branch on `apps/server/**` new code.
- Evidence commands: `cargo test -p server` (DTO/error-mapping/Laya-isolation unit tests +
  `/health` integration, no model needed), `cargo test -p server --features integration`
  (real-model e2e + concurrency scenario), diff-coverage tool report.

## Risk Assessment
- Risk: lazy-load-per-model-on-first-request means the FIRST request against any given model pays
  full model-load+warmup latency inline — acceptable for v1 (benchmark tool, not low-latency
  production API) but document clearly in API response/docs so callers aren't surprised; do not
  silently pre-load all 3 models eagerly (that's a larger memory footprint and slower server start
  — not requested, would be unrequested scope).
- Risk: `Mutex` lock held across the full blocking inference call means concurrent requests fully
  serialize (by design, matches R2's recommendation) — if this becomes a real user complaint later,
  the actor/channel pattern (R2 §2 alternative) is the documented upgrade path, not implemented now
  per YAGNI.

## Security Considerations
- No auth in v1 (not requested; local benchmark tool). If `openjev-server` is ever exposed beyond
  localhost, document that as an explicit future concern — do not add unrequested auth machinery
  now, but flag in the phase's completion report that it currently has zero access control, so the
  user can decide if that matters for their deployment.
- Input validation: reject malformed JSON / oversized request bodies via axum's built-in body-limit
  defaults + `ApiError` mapping — no unbounded body read.

## Next Steps
- Phase 6 folds this phase's integration/concurrency tests into the consolidated suite and CI
  workflow. Phase 7 deploys and exercises this binary (alongside `openjev-cli`) on the real Linux
  server.
