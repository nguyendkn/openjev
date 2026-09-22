# Code Standards — OpenJev-rs

Engineering conventions for the Rust implementation of OpenJev/SemIf. Workspace layout follows Cargo idiom: crates per concern, shared dependencies, file size <200 lines per module. Phase 0 spike verified llama-cpp-2 + Rust ecosystem; these rules reflect confirmed & planned patterns.

---

## 1. Error Handling

**MUST**: All public module functions return `Result<T, ModuleError>` with hand-rolled enum per crate using `thiserror` derive macro (pending; not yet in Cargo.toml — **add as dep to each error-producing crate**).

**Error type naming**: `<CrateName>Error` (e.g., `EngineError`, `PipelineError`, `ModelsError`). Each crate owns its error type; no cross-crate error type reuse.

**Thiserror derives**: implement `std::fmt::Display` + `std::error::Error` via `#[derive(Error, Debug)]`. Use `#[error("message")]` on each variant with optional `{source}` ref for error chains.

**Error propagation**:
- Library crates: return `Result` types, never `.unwrap()` / `.expect()` in impl paths (test/bin code allowed for setup).
- Binary crates (cli, server): wrap library `Result` → CLI exit codes / HTTP 500+ response codes; log error chain via `eprint!` or structured logging.
- Subprocess (Laya `ggmlc-run`): shell-out failure (non-zero exit, not found, timeout) → recoverable error variant in `ModelsError` (not panic); caller treats as missing model result, retries / degrades per feature.

**Validation errors**: JSON schema validation failures (generation pipeline) → distinct `ValidationError` variant within `PipelineError`, not a panic or silent drop.

**KV-cache / context state**: reset failures (if API exposes reset methods) → `EngineError` variant; treat as correctable (log + continue or fail run depending on recovery strategy Phase 1 defines).

---

## 2. Module & Crate Organization

**Workspace structure** (Cargo.toml workspace root):
```
crates/
  timing/         # Timings struct + Duration helpers (minimal, no inference code)
  models/         # Model registry, download, Laya subprocess wrapper
  engine/         # llama-cpp-2 context & inference engine
  pipeline/       # Generation & readout pipelines, combining models + engine
apps/
  cli/            # openjev-cli binary (clap derive, bench/serve subcommands)
  server/         # openjev-server binary (reserved; Phase 5; today unused)
```

**File size**: MUST stay <200 lines per `.rs` file. Split public fn groups into separate modules:
- `engine/{mod,context,sampling}.rs` not `engine/{mod,all}.rs`
- `pipeline/{mod,readout,generate,laya}.rs` not `pipeline/{mod,all}.rs`
- `models/{mod,download,laya}.rs` not `models/{mod,all}.rs`

**Module re-export** (`mod.rs`): public types/fns re-exported; private types stay in submodule. Example:
```rust
pub mod readout;
pub mod generate;
pub use readout::ReadoutResult;
pub use generate::GenerateResult;
```

**Error module**: `src/error.rs` in each crate (not `errors.rs`); re-exported from `mod.rs` as `pub use error::*;` or `pub mod error;` depending on whether public API includes the error type.

**Dependencies**: crate-to-crate via path refs in Cargo.toml (`path = "../other"`); no circular deps. Dependency DAG: timing ← models, engine ← pipeline; cli/server depend on all libs (timing, models, engine, pipeline).

---

## 3. Naming Conventions

**Rust std idiom**:
- **Modules/files**: `snake_case` (`llama_context.rs`, `model_download.rs`, not `llamaContext.rs`)
- **Types/traits**: `PascalCase` (`Timings`, `ReadoutResult`, `EngineError`)
- **Constants**: `SCREAMING_SNAKE_CASE` (`DEFAULT_N_CTX`, `MAX_TOKENS`)
- **Functions/methods**: `snake_case` (`run_readout`, `decode_prompt`, `sample_greedy`)
- **Variables/params**: `snake_case` (`model_path`, `token_ids`, `is_last`)
- **Lifetimes**: `'a`, `'b` (not `'input`, `'ctx`)

**Field names in structs**: match serde JSON case in API docs (if JSON field is `model_name` stay `snake_case`; do NOT auto-convert via `#[serde(rename)]` — keep Rust field = JSON key).

**Boolean params/fields**: prefix with verb when intent is unclear (e.g., `AddBos::Always` not just `true`; use enums for safety).

**Acronyms**: lower after first letter in identifiers (`LlamaContext`, `KvCache`, `EogToken`, not `LLAMAContext`/`KVCache`/`EOGToken`).

---

## 4. Testing Strategy

**Coverage targets** (lanes: normal=0-100%, high-risk=0-100%; phase-0 exemption does not apply per quality-gate.md):
- **Line coverage**: ≥90%
- **Branch coverage**: ≥75%
- **Evidence command**: `cargo test --all --features integration --coverage` (or `cargo tarpaulin --features integration`)

**Test organization**:
- **Unit tests**: module-level `#[cfg(test)]` blocks within the `.rs` file being tested (same file, under the impl).
- **Integration tests**: `tests/integration_*` at crate root, exercising public APIs across module boundaries.
- **Real data, never mocks**:
  - Use real GGUF fixture files (Qwen3-0.6B Q4_K_M, <100MB, vendored or cached via hf-hub test feature).
  - Real model context from llama-cpp-2, never stubbed inference.
  - For subprocess (Laya): real ggmlc-run binary if available in test env, skip test gracefully if not found (use `skip!` macro, not panic).
  - JSON parsing tests: real schema-valid/schema-invalid samples, not arbitrary strings.

**Integration test feature flag**: gate all non-unit tests behind `--features integration` (Cargo.toml: `[features] integration = []`); unit tests always run.

**Test naming**: test fn names describe the scenario, e.g. `#[test] fn constrained_readout_favors_provided_option_label() { ... }`.

**Error case coverage**: test validation failures, I/O errors, schema mismatches (not just happy path).

**Timing assertions**: do NOT assert exact microsecond values; instead assert the call completes in reasonable time (e.g., `elapsed < Duration::from_secs(30)` for a single model load on dev machine — bounds vary by phase).

---

## 5. API Design Conventions

### 5.1 Engine (`crates/engine`)

**Planned surface** (Phase 1 commits):
```rust
pub struct Engine { /* llama-cpp-2 context + model */ }
impl Engine {
    pub fn load(model_path: &Path, params: EngineParams) -> Result<Self, EngineError>;
    pub fn tokenize(&self, text: &str) -> Result<Vec<Token>, EngineError>;
    pub fn decode_prompt(&mut self, tokens: &[Token]) -> Result<(), EngineError>;
    pub fn decode_next(&mut self) -> Result<Token, EngineError>;
    pub fn warmup(&mut self) -> Result<(), EngineError>;
    pub fn sample_greedy(&self) -> Result<Token, EngineError>;  // Phase 3 requirement
}
```

**Logits access**: Phase 2 reads logits after prompt decode; engine exposes `pub fn get_logits(&self) -> &[f32]` or iterator; constrained-softmax (restricting to option-label token ids) happens in **pipeline**, not engine. Engine does not know about options.

**Context / KV cache**: `ctx` field is private; cache reset (if needed between readout/generate) happens inside engine methods. If `kv_cache_clear` is exposed, it's `pub fn clear_cache(&mut self) -> Result<(), EngineError>`.

**Timings**: engine does not record timing; pipeline/app measures wall-clock time around engine method calls and assigns to `Timings` struct.

### 5.2 Models (`crates/models`)

**Surface**:
```rust
pub async fn download_model(
    name: &str,              // e.g., "qwen3-0.6b-q8_0"
    cache_dir: &Path
) -> Result<PathBuf, ModelsError>;

pub fn verify_checksum(path: &Path, expected: &str) -> Result<bool, ModelsError>;

pub fn run_laya_inference(
    model_name: &str,
    prompt: &str,
    options: &[String]
) -> Result<Option<LayaResult>, ModelsError>;
```

**Model registry**: TOML or JSON file mapping model names to HF repo/revision/filename; pinned revisions prevent silent upstream changes. Laya shell-out failure (model not found, ggmlc-run missing) → `Ok(None)`, not error, so caller can degrade or retry.

**Async download**: use `tokio::fs` or `hf-hub`'s async API; CLI blocks on this (sync shell), server exposes as `POST /download` later (Phase 5).

### 5.3 Pipeline (`crates/pipeline`)

**Constrained readout**:
```rust
pub fn run_readout(
    engine: &mut Engine,
    prompt: &str,
    options: &[String],
    timings: &mut Timings
) -> Result<ReadoutResult, PipelineError>;

pub struct ReadoutResult {
    pub option_probs: Vec<(String, f32)>,  // (label, prob) after softmax over *only* option-token ids
    pub elapsed_ms: u128,  // captured at pipeline level, not engine
}
```

**Generation**:
```rust
pub fn run_generate(
    engine: &mut Engine,
    prompt: &str,
    options: &[String],
    timings: &mut Timings
) -> Result<GenerateResult, PipelineError>;

pub struct GenerateResult {
    pub option_probs: Vec<(String, f32)>,  // parsed from generated JSON after stripping <think>
    pub raw_output: String,
    pub elapsed_ms: u128,
}
```

**MUST**: constrain logits before softmax (order-critical for correctness per spec). Strip `<think>...</think>` tags before JSON parsing. JSON validation failure → `PipelineError::ValidationFailed { expected: "schema", got: "actual" }`.

**Max tokens**: greedy generation loop caps at 512; explicit loop-counter check, not model parameter (since `llama-cpp-2` API phase 0 confirmed this pattern).

### 5.4 CLI (`apps/cli`)

**Subcommands** (clap derive):
```rust
#[derive(Parser)]
struct Cli {
    #[command(subcommand)]
    command: Command
}

#[derive(Subcommand)]
enum Command {
    Bench(BenchArgs),
    Serve(ServeArgs),  // Phase 5; stubbed in Phase 4
}

#[derive(Args)]
struct BenchArgs {
    #[arg(long)]
    model_path: PathBuf,
    #[arg(long, value_delimiter = ',')]
    options: Vec<String>,
    #[arg(long)]
    prompt: String,
}
```

**Output**: `benchmark_report.json` (default) or `-o <path>`. JSON structure mirrors `TimingsReport { timings: Timings, readout: ReadoutResult, generate: GenerateResult }`. Pretty-print via `serde_json::to_string_pretty`.

**Error handling**: library errors caught in `main`, printed to stderr with exit code 1.

### 5.5 Server (Future; Phase 5)

**Axum routes** (planned):
```
POST /bench
  body: { model_path, prompt, options: ["A", "B", "C"] }
  response: 200 { timings, readout, generate }
  error: 400 (bad input), 500 (inference failure)
```

**Blocking inference**: wrap engine calls in `tokio::task::spawn_blocking`; use `Arc<Mutex<Engine>>` singleton to serialize access (KV cache not concurrent-safe per llama-cpp-2 research phase 0).

---

## 6. Locked Engineering Rules

### MUST rules (inviolable per product spec)

- **CPU-only v1**: NO GPU feature flags (cuda, rocm, etc.) anywhere. Dependency audit via `cargo tree` in CI.
- **Constrained readout ordering**: logits restricted to option-label token ids **BEFORE** softmax. Softmax computed app-side over ~5-20 tokens, not full vocab.
- **Generation limits**: greedy decoding, max 512 tokens enforced via loop counter (not model parameter), `<think>...</think>` tags stripped before JSON parsing.
- **Timings struct**: every stage records wall-clock time into shared `Timings` struct (7 fields: model_load, warmup, tokenize, constrained_readout, generation, laya_model_load, laya_inference). No bypass; new stages extend struct, not side-channel.
- **CLI / server separation**: no shared code between `apps/cli/` and `apps/server/`. If logic duplicates, factor to `crates/` library, not app-level shared file.
- **Subprocess (Laya)**: shell-out to external `ggmlc-run` CLI; subprocess failure (exit code, timeout, not found) → recoverable error result (null/error return), never panic/crash.
- **No mocks in tests**: real GGUF files, real model contexts, real data fixtures. Skip tests gracefully if fixture unavailable (e.g., integration flag).

### SHOULD rules (best practice, may be overridden by phase discovery)

- **Error type consistency**: `thiserror` enums with `#[error(...)]` messages; avoid `anyhow::anyhow!` for structured library errors.
- **Async in libraries**: defer to call site (cli uses sync shell, server uses tokio); don't force async on libraries.
- **Workspace shared deps**: pin common versions in root `Cargo.toml [workspace.dependencies]`, not per-crate.
- **Serde derives**: always `#[derive(Serialize, Deserialize)]` on types exported to JSON (CLI output, HTTP responses).

### MUST NOT rules

- **No panics in inference paths**: validation failure, I/O error, schema mismatch → `Result` type error, never `.unwrap()`.
- **No feature-gated error types**: error enums are always available (do not `#[cfg(feature = "integration")]` an error variant).
- **No silent failures**: null/missing result is a return value; actual error → logged/reported via error type.
- **No cross-crate error re-export**: each crate owns its error type; if calling code needs to match on a dep's error, the dep's error is public and used directly (no wrapping wrapper types).

---

## 7. Cargo & Build Discipline

**Edition**: `2021` (workspace-wide, already set).

**Resolver**: `resolver = "2"` (workspace-wide).

**Feature flags**:
- `integration` (bool, off by default): enables real-model/subprocess tests.
- No GPU flags (cuda/rocm/etc.).
- `dynamic-link` (if ever needed for DLLs) stays off by default to avoid runtime dependency.

**Dependencies**:
- **Pinned versions** in workspace root `[workspace.dependencies]`; crate manifests reference via `{ workspace = true }`.
- **Minimal deps**: only declared if used (no "may be useful later").
- **Common stack**: serde (all), clap (cli/server), axum/tokio (server), hf-hub (models), llama-cpp-2 (engine).
- **Error handling**: add `thiserror` to engine/models/pipeline Cargo.toml after this doc approval.

**Target spec**:
- Linux (primary CI), Windows (supported per phase-0 research), macOS (untested, likely works).
- No wasm/embedded targets.

---

## 8. Documentation & Code Clarity

**Inline comments**: explain *why*, not *what* (code speaks for itself). Focus on constraints: "logits restricted before softmax for correctness" vs. "restrict logits".

**Public fn doc comments**: `///` blocks with ## Errors section (what error types can occur, when). Example:
```rust
/// Loads a GGUF model and initializes inference context.
///
/// # Errors
/// Returns `EngineError::ModelNotFound` if path does not exist.
/// Returns `EngineError::LoadFailed` if llama.cpp context creation fails.
pub fn load(model_path: &Path) -> Result<Self, EngineError> { ... }
```

**Struct/enum doc comments**: describe invariants. Example:
```rust
/// Result of a constrained-readout inference pass.
/// Probabilities sum to ~1.0 (floating-point rounding).
pub struct ReadoutResult {
    pub option_probs: Vec<(String, f32)>,
}
```

---

## 9. Verification & CI

**Checks** (to be wired in CI):
- `cargo check --all-targets`
- `cargo test --all --features integration`
- Coverage report: `cargo tarpaulin --features integration --out Html` (>= 90% line, >= 75% branch).
- Clippy: `cargo clippy --all-targets -- -D warnings`
- Format: `cargo fmt --check`

**No feature-gated public APIs**: a public type or fn either exists or does not (no `#[cfg(...)]` on pub items except examples).

---

## 10. Deferred / Planned Conventions

- **Async runtime**: Phase 5 adds `tokio` to server; cli remains sync until multi-model parallelism is needed.
- **Config file loading**: Phase 5+ may add `figment` or manual TOML; today CLI flags only.
- **Structured logging**: Phase 6 may add `tracing` layers; today `eprint!` macros suffice.
- **Chat template**: Phase 1 hand-builds ChatML prompts (fallback if `llama-cpp-2` template API missing).

---

**Last updated**: 2026-09-22 | **Status**: Phase 0 spike verified; awaiting Phase 1 implementation
