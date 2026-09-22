# Research: CLI + HTTP API Architecture for OpenJev-rs

Scope: crate layout, axum+blocking-inference integration, clap shared config, phase timing. 5 web searches, no code written.

## 1. Project layout: workspace vs single binary

**Verified:**
- Cargo workspaces are the standard 2026 pattern for "core lib + multiple entrypoints": a `core`/`engine` lib crate holding business logic, plus thin binary crates (`cli`, `server`) that depend on it. Root `Cargo.toml` is a virtual workspace (`[workspace]`, no `[package]`); shared deps pinned via `[workspace.dependencies]`. (rust-book.cs.brown.edu/ch14-03-cargo-workspaces, rustify.rs/glossary/workspace)
- Real-world precedent (OpenAI Codex CLI, per DeepWiki): a **single user-facing binary** (`codex-cli`) dispatches all modes — TUI, exec, app-server, MCP server — via **subcommands**, not separate binaries. (deepwiki.com/openai/codex/7.1)
- clap best practice: don't parse CLI args inside library crates (couples the lib API to CLI); parse in `main`, pass structs inward. This is what makes the config reusable from both CLI and server paths regardless of binary topology. (rust-cli-recommendations.sunshowers.io/handling-arguments.html)

**Inferred recommendation for OpenJev-rs:** given the locked scope is "CLI + HTTP API in the same crate," the Codex precedent maps directly: one binary crate `openjev-rs` with clap subcommands (`bench`, `serve`, …), backed by a `core`/`engine` lib crate (or lib target in the same crate) holding the inference pipeline, timing, and config types. A 3-crate workspace (`core` + `cli` + `server` as separate binaries) is the alternative if CLI and server need to ship/version independently — not indicated here, so single-binary-with-subcommands is the simpler KISS-aligned choice matching precedent.

**Not found:** no directly comparable "Rust local-LLM benchmark tool with both CLI and HTTP mode" project surfaced in search (Ollama itself is Go, not Rust); llama.cpp Rust wrapper ecosystem (`llama-cpp-rs`/`llama-cpp-2`) examples found were embedding-server-focused (`embellama`), not CLI+server hybrids — see open question below.

## 2. axum + blocking llama-cpp-2 inference

**Verified:**
- `axum` 0.8.x is current stable as of ~March 2026 (crates.io/axum). Confirm exact version at implementation time via `cargo add axum`.
- Rule: never block inside an async task; route blocking/CPU-heavy work (model inference, blocking I/O) through `tokio::task::spawn_blocking`. Mixing blocking calls into async handlers directly causes thread-pool starvation — described as the most common production incident class in Rust async services (silent tail-latency spikes, no panics). (rustify.rs/glossary/spawn, markaicode.com production-architecture article)
- Canonical handler pattern:
  ```
  async fn handler() -> Result<impl IntoResponse, StatusCode> {
      tokio::task::spawn_blocking(move || { /* blocking inference */ })
          .await.unwrap()
  }
  ```
  (docs.rs/tokio spawn_blocking; github.com/tokio-rs/axum discussions #2045, #3277)
- Caveat found in a users.rust-lang.org thread: naive `spawn_blocking` use can still starve axum's worker threads under load ("All axum threads busy after spawning a tokio blocking thread") — worth reading before implementation.
- For CPU-bound (not I/O-blocking) work specifically, one production-architecture writeup recommends a **dedicated thread pool (e.g. rayon)** rather than relying on tokio's default blocking-thread pool sizing, since tokio's blocking pool is tuned for blocking I/O, not sustained CPU saturation.

**llama.cpp context sharing (verified):**
- `LlamaContext` (and model handle) in `llama-cpp-2` is `!Send`/`!Sync` — cannot be freely shared across threads via bare `Arc`. (github.com/utilityai/llama-cpp-rs issue #483; docs.rs/llama_cpp-2)
- Two workable patterns surfaced:
  1. **`Arc<Mutex<...>>` singleton**: wrap the loaded model/context, clone the `Arc` into each blocking task, lock the `Mutex` before use. Serializes inference — matches OpenJev-rs's benchmark use case (one run at a time) better than a concurrent server anyway. Example precedent: `embellama` crate's `Arc<Mutex<EmbeddingEngine>>` singleton. (crates.io/embellama, docs.rs/embellama)
  2. **Dedicated worker thread + channel (actor pattern)**: own the context on one long-lived OS thread, send requests in via `mpsc` channel, receive results back. Avoids repeated lock contention/context-switch overhead of Mutex under concurrent load; recommended production stack per markaicode article: tokio multi-thread runtime + axum + bounded `mpsc` channels + `spawn_blocking` workers + tracing/OpenTelemetry.
- Upstream llama.cpp server itself has open issues around concurrent-request handling being fragile (ggml-org/llama.cpp #15008, #4666) — reinforces that serializing access (pattern 1, or single-worker actor pattern 2) is the safer default rather than trying to run multiple concurrent contexts.

**Recommendation for OpenJev-rs:** since benchmark runs are inherently sequential per model, an `Arc<Mutex<LlamaContext>>` (or a single dedicated worker thread owning the context, fed via channel) both work; the worker+channel/actor pattern is preferable if the HTTP API must also stay responsive (e.g. return "job accepted" immediately, poll for phase timings) — Mutex+spawn_blocking is simpler if requests can just block until done. Given "local benchmark runs," simplicity likely wins: `Arc<Mutex<...>>` + `spawn_blocking`.

## 3. clap derive: shared config across CLI and server

**Verified:**
- Derive API is the recommended (not builder) API in 2026. (docs.rs/clap _derive::_tutorial)
- `#[derive(Args)]` produces a reusable, flattenable arg group — e.g. a `ModelConfig { model_path, model_name, ... }` struct can be `#[command(flatten)]`ed into multiple subcommand structs (`Bench`, `Serve`), giving one canonical config type used by both the CLI parse path and, when starting the HTTP server, the same struct as its runtime config (no duplication). (rust.code-maven.com/clap-subcommand, docs.rs derive tutorial)
- Parse in `main`, pass structs into library/business logic — keeps `core` crate clap-free and reusable if a server-only or config-file-driven entrypoint is added later. (rust-cli-recommendations.sunshowers.io)
- For config from multiple sources (CLI flags + env + file, useful if `serve` needs config beyond flags), combine clap with `figment` or manual TOML layering — mentioned as a common extension, not deeply sourced here.

## 4. Phase-by-phase timing architecture

**Verified/standard patterns (from general Rust knowledge + tracing docs, since results were mostly incidental):**
- Two established approaches: (a) a plain `Timings` struct populated via `std::time::Instant::now()` deltas at phase boundaries (model-load, warmup, tokenize, constrained-readout, generation) — trivially `#[derive(Serialize)]` with `serde` for both CLI stdout (pretty-print or JSON) and HTTP JSON response; (b) `tracing` spans per phase (`#[instrument]` or manual `span!`/`Instant`-in-span), captured via a custom `Subscriber`/`tracing-serde` layer for structured JSON output. (docs.rs/tracing, docs.rs/tracing-serde, docs.rs/tracing-subscriber Json formatter)
- `tracing-serde` explicitly exists to serialize tracing span/event data via serde — usable if OpenJev-rs wants tracing-based observability in addition to (not instead of) a benchmark-report JSON.
- Given the requirement is "measure wall-clock time separately per phase" and emit that as **both CLI stdout and HTTP JSON**, the simple custom `Timings` struct (approach a) is easier to unit-test (no subscriber setup, just assert struct field values) and directly serializable — likely the better fit than full `tracing` spans, which are more suited to live observability/log correlation than to a discrete benchmark-report artifact. `tracing` can still be layered on top for logging without owning the reported timing values.
- No specific crate found purpose-built for "named-phase timing struct → JSON" (searches surfaced `criterion`/`benchmark` crates, which are for statistical micro-benchmarking loops, not one-shot phase timing of a single run) — this suggests a small hand-rolled `Timings`/`PhaseTimer` type is idiomatic and expected, not a missing library.

## Unresolved questions
1. No concrete example found of a Rust project combining llama.cpp inference with both a CLI and an HTTP server in one crate — the workspace/subcommand recommendation is inferred from adjacent precedent (Codex CLI structure), not a direct analog. Worth a targeted GitHub search (`llama-cpp-rs` + `axum` in same repo) before locking the plan.
2. Exact current `axum` and `llama-cpp-2` patch versions weren't pinned from crates.io pages directly (search returned version-list URLs, not the version numbers) — confirm via `cargo add axum llama-cpp-2 --dry-run` at implementation time.
3. Whether OpenJev-rs's HTTP API needs to support *concurrent* benchmark runs (multiple models/requests in flight) or strictly one-at-a-time — this determines whether `Arc<Mutex<>>` (serializes, simplest) or a per-request context pool is warranted. Not specified in the given scope.
4. Whether `tracing` is wanted at all for logging/observability (separate from the `Timings` JSON report) — affects whether to add `tracing-subscriber` as a dependency now or defer.

## Sources
- https://rust-book.cs.brown.edu/ch14-03-cargo-workspaces.html
- https://rustify.rs/glossary/workspace
- https://deepwiki.com/openai/codex/7.1-mcp-server-configuration-and-management
- https://rust-cli-recommendations.sunshowers.io/handling-arguments.html
- https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html
- https://rust.code-maven.com/clap-subcommand
- https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html
- https://github.com/tokio-rs/axum/discussions/2045
- https://github.com/tokio-rs/axum/discussions/3277
- https://users.rust-lang.org/t/all-axum-threads-busy-after-spawning-a-tokio-blocking-thread/131847
- https://markaicode.com/architecture/rust-production-system-design-architecture/
- https://github.com/utilityai/llama-cpp-rs/issues/483
- https://docs.rs/llama_cpp
- https://docs.rs/llama-cpp-2
- https://crates.io/crates/embellama
- https://docs.rs/embellama/latest/embellama/
- https://github.com/ggml-org/llama.cpp/issues/15008
- https://github.com/ggml-org/llama.cpp/issues/4666
- https://crates.io/crates/axum
- https://docs.rs/tracing/latest/tracing/
- https://github.com/DesmondWillowbrook/tracing/blob/master/tracing-serde/README.md
- https://docs.rs/tracing-subscriber/latest/tracing_subscriber/fmt/format/struct.Json.html
