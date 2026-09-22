# OpenJev-rs Codebase Summary

Rust port of OpenJev/SemIf (CPU-only LLM benchmark: constrained-readout vs generation vs Laya). **Loop 5 (HoH) stage** — `engine`/`models`/`pipeline`/`apps/cli`/`apps/server` are all implemented and running real inference (not stubs); all 3 registered LLM models (`qwen3-0.6b`, `minicpm5-2b`, `qwen3-4b`) verified end-to-end via both `apps/cli` and a live external `apps/server` on `103.146.166.46:80`. Laya (the 3rd comparison method, `laya` registry entry) is now wired as a blocking HTTP client to a persistent `laya serve` process (NOT the generic `ggmlc-run` CLI Loop 1 explored, and NOT a per-request shell-out — see "Laya Server Process" in README) — `--skip-laya`/`skip_laya` is genuinely real, not a no-op. See `docs/research/openjev-rust-research.md` for reference architecture and `plans/20260922-2328-hoh-openjev-rs/loops/` for the current HoH loop history (source of truth for what changed since Phase 0).

---

## Directory Tree

```
openjev/
├── apps/
│   ├── cli/              → openjev-cli binary (one-shot benchmark run, --skip-laya real, Loop 5)
│   └── server/           → openjev-server binary (axum HTTP API: POST /bench, GET /health)
├── crates/
│   ├── timing/           → Timings struct only (0 intra-workspace deps) [IMPLEMENTED]
│   ├── models/           → registry + hf-hub + Laya HTTP client (`laya serve`, 0 intra-workspace deps) [STUB]
│   ├── engine/           → GGUF loader via llama-cpp-2, LlamaContext wrapper [STUB]
│   └── pipeline/         → 3 submodules: readout, generate, laya [STUB]
├── docs/
│   └── research/         → openjev-rust-research.md (reference architecture)
├── plans/
│   ├── 20260922-2146-openjev-rust-implementation/  → main plan tree (Phase 0-6)
│   └── 20260922-2328-hoh-openjev-rs/               → HOH plan
├── Makefile              → build/test/bench/serve/fmt/clippy targets
├── Cargo.toml            → workspace manifest
└── rust-toolchain.toml   → toolchain pinning
```

---

## Per-Crate State

### `crates/timing` — **IMPLEMENTED**
| Aspect | Details |
|--------|---------|
| Files | `lib.rs` (27 lines) |
| Key Types | `Timings` struct (7 fields, `Serialize`, `Clone`, `Copy`, `Debug`) |
| Fields | `model_load_ms`, `warmup_ms`, `tokenize_ms`, `constrained_readout_ms`, `generation_ms`, `laya_model_load_ms`, `laya_inference_ms` |
| Dependencies | `serde` 1.0 (derive only) |
| Status | Complete; lowest-level shared type, 0 intra-workspace deps |

### `crates/models` — **IMPLEMENTED**
| Aspect | Details |
|--------|---------|
| Files | `lib.rs`, `download.rs`, `laya.rs`, `error.rs` |
| Modules Declared | `download`, `laya`, `error` |
| Purpose | Model registry (`REGISTRY`, 4 entries, exact ids: `qwen3-0.6b`, `qwen3-4b`, `minicpm5-2b`, `laya`), `hf-hub`-backed cache-aware download (`ensure_downloaded`), `find(id)` lookup; `laya::score()` — blocking HTTP client (`reqwest`) to a persistent `laya serve` process at `http://127.0.0.1:8090/v1/decide` |
| Dependencies | `hf-hub` 1.0, `serde` 1.0, `serde_json` 1.0, `reqwest` 0.13 (blocking+json+rustls — reused from `hf-hub`'s existing dependency tree, no second HTTP stack added) |
| Deps Within Workspace | None |
| Status | All 4 registry entries (`qwen3-0.6b`, `qwen3-4b`, `minicpm5-2b`, `laya`) exercised end-to-end (LLM models Loops 1-4, Laya Loop 5). `laya.rs` is a real HTTP client (Loop 5) — corrected from the earlier planned `ggmlc-run` shell-out; the real Laya tool is a separate `laya` binary run as `laya serve`, called over HTTP, not shelled out to per-request. |

### `crates/engine` — **IMPLEMENTED**
| Aspect | Details |
|--------|---------|
| Files | `lib.rs`, `error.rs` |
| Purpose | GGUF loader via `llama-cpp-2` (`Engine::load`), tokenize/chat-template/decode/sample/reset-KV-cache API, process-wide shared `LlamaBackend` singleton (`shared_backend()`, Loop 4 fix — `llama-cpp-2`'s `LlamaBackend::init()` can only succeed once per process while any prior instance is alive, which broke multi-model caching until fixed) |
| Dependencies | `llama-cpp-2` 0.1, `models`, `timing` |
| Deps Within Workspace | `models`, `timing` |
| Status | Implemented and verified against 3 real GGUF models (CPU-only, quantized). Uses `unsafe` lifetime-widening for a self-referential `Engine` (heap-boxed `LlamaModel` + `LlamaContext<'static>` borrowing from it) — documented in the struct's doc comment. |

### `crates/pipeline` — **IMPLEMENTED**
| Aspect | Details |
|--------|---------|
| Files | `lib.rs`, `readout.rs`, `generate.rs`, `laya.rs`, `error.rs` |
| Modules Declared | `readout`, `generate`, `laya`, `error` |
| Purpose | Constrained single-token readout (`run_readout`, softmax restricted to option-label token ids), greedy generation (`run_generate`), and Laya scoring (`run_laya`, wraps `models::laya::score()`) — all 3 implemented and used by `apps/cli`/`apps/server` as of Loop 5 |
| Dependencies | `engine`, `models`, `timing` |
| Deps Within Workspace | `engine`, `models`, `timing` |
| Status | `readout`/`generate` implemented, unit-tested (3 tests in `readout.rs`), and verified against 3 real models. `laya.rs` implemented Loop 5 (real HTTP client to `laya serve`, verified end-to-end via `apps/cli`/`apps/server`, no dedicated unit test — G10/G12, tracked, non-blocking; `generate.rs` also still has no committed unit tests, same G10). |

### `apps/cli` — **IMPLEMENTED**
| Aspect | Details |
|--------|---------|
| Files | `src/main.rs`, `Cargo.toml` |
| Binary Name | `openjev-cli` |
| Purpose | One-shot benchmark runner (`--model --prompt --options --format json`); loads a registry model, runs readout+generate, prints `BenchReport`-shaped JSON |
| Dependencies | `clap` 4.4 (derive), `serde` 1.0, `serde_json` 1.0, + all core crates |
| Deps Within Workspace | `engine`, `models`, `pipeline`, `timing` |
| Status | Implemented and verified for `qwen3-0.6b`, `qwen3-4b`, `minicpm5-2b` on the Linux server (real GGUF inference, valid JSON, correct sanity-prompt answers). |

### `apps/server` — **IMPLEMENTED**
| Aspect | Details |
|--------|---------|
| Files | `src/main.rs`, `src/app.rs` |
| Binary Name | `openjev-server` |
| Purpose | axum HTTP API; `POST /bench` (lazy-load + per-model `Engine` cache), `GET /health`. `Engine` is `!Send` (FFI raw pointers), so model caching is confined to one dedicated OS thread reached via `mpsc`/`oneshot` channels from the async handler (not `spawn_blocking` — documented pivot, see `AppState` doc comment in `app.rs`). Worker thread is panic-supervised (Loop 4, G13 fix): a `catch_unwind`-wrapped supervisor respawns the worker with a fresh, empty model cache on any panic instead of leaving the channel permanently dead. |
| Dependencies | `clap` 4.4 (derive), `axum` 0.7, `tokio` 1.0 (full features), `serde` 1.0, `serde_json` 1.0, + all core crates |
| Deps Within Workspace | `engine`, `models`, `pipeline`, `timing` |
| Status | Implemented and running live on `103.146.166.46:80` (root, no auth — accepted risk, see G14 below), externally curl-verified for all 3 exercised models, concurrency, invalid-model 4xx, and panic-recovery. G12 (no committed unit tests for `app.rs`/`main.rs`) tracked, non-blocking — all validation is black-box HTTP-level and passes. |

---

## Dependency Graph

```
timing (0 workspace deps)
  ↑
  ├─── models (depends: timing)
  │      ├─── hf-hub 1.0         (model download)
  │      └─── serde 1.0
  │
  ├─── engine (depends: models, timing)
  │      ├─── llama-cpp-2 0.1    (GGUF loader, logits API)
  │      └─── models, timing
  │
  └─── pipeline (depends: engine, models, timing)
         ├─── engine, models, timing
         └─── serde_json 1.0 (JSON parsing for `generate`); readout/generate/laya all implemented (Loop 5)

apps/cli (depends: all core crates)
  ├─── clap 4.4 (derive)         (CLI arg parsing)
  ├─── serde 1.0 (derive)        (config serialization)
  ├─── serde_json 1.0            (JSON output)
  └─── engine, models, pipeline, timing

apps/server (depends: all core crates)
  ├─── clap 4.4 (derive)         (CLI args for server mode)
  ├─── axum 0.7                  (HTTP routing + handlers)
  ├─── tokio 1.0 (full)          (async runtime, spawn_blocking)
  ├─── serde 1.0 (derive)        (request/response bodies)
  ├─── serde_json 1.0            (JSON serialization)
  └─── engine, models, pipeline, timing
```

---

## External Dependencies & Purpose

| Crate | Version | Purpose | Why This One |
|-------|---------|---------|--------------|
| `llama-cpp-2` | 0.1 | GGUF model loading, logits/KV-cache access | Thin wrapper around llama.cpp C API; preserves exact benchmark behavior vs original (which uses wllama, wrapping same llama.cpp) |
| `hf-hub` | 1.0 | Hugging Face Hub model downloads | Standard Rust HF integration; supports static model registry by revision/filename |
| `axum` | 0.7 | HTTP routing, async request handling | Minimal, composable web framework; tokio-native, pairs with `spawn_blocking` for CPU-bound inference |
| `tokio` | 1.0 (full) | Async runtime, blocking thread pool | Ships `spawn_blocking` for offloading inference to avoid blocking async tasks |
| `clap` | 4.4 (derive) | CLI argument parsing | Declarative, reusable across CLI and server via `#[derive(Args)]` + `#[command(flatten)]` |
| `serde` | 1.0 | Serialization/deserialization | Benchmark timings & config → JSON (stdout, HTTP responses) |
| `serde_json` | 1.0 | JSON codec | Standard JSON I/O for CLI and HTTP responses |

---

## Build & Run Commands

All commands assume root directory (`C:\Users\nguyendk\Documents\Projects\openjev`).

### Build
```bash
# Build all crates (workspace)
cargo build --workspace

# Build only CLI
cargo build -p cli

# Build only server
cargo build -p server
```

### Test
```bash
# Test all crates
cargo test --workspace
```
*Note:* Real-model integration tests deferred to Phase 6 (`--features integration`).

### Run
```bash
# Run CLI (one-shot benchmark)
cargo run -p cli --

# Run server (HTTP API)
cargo run -p server --
```
Mapped to Makefile targets `make bench` and `make serve`.

### Quality
```bash
# Format check
cargo fmt --all -- --check

# Lint (deny warnings)
cargo clippy --workspace -- -D warnings

# CI suite (fmt + clippy + test)
make ci
```

### Makefile Targets
| Target | Command | Purpose |
|--------|---------|---------|
| `build` | `cargo build --workspace` | Compile all crates |
| `test` | `cargo test --workspace` | Run unit + integration tests |
| `bench` | `cargo run -p cli --` | Execute CLI benchmark runner |
| `serve` | `cargo run -p server --` | Start HTTP API server |
| `fmt` | `cargo fmt --all` | Reformat code |
| `clippy` | `cargo clippy --workspace -- -D warnings` | Lint, deny all warnings |
| `ci` | `fmt-check clippy test` | Full CI pipeline (no fixes, only checks) |
| `fmt-check` | `cargo fmt --all -- --check` | Verify code formatted (no changes) |
| `server-deploy` | (stub) | Phase 7 placeholder |

---

## What's NOT Implemented Yet

Original Phase 0-7 plan (`plans/20260922-2146-openjev-rust-implementation/`) was superseded by the HoH loop process (`plans/20260922-2328-hoh-openjev-rs/`) partway through — Phases 1-5 below are done, tracked instead as closed via that loop history, not this table.

| Phase | Scope | Target File(s) | Status |
|-------|-------|-----------------|--------|
| **Phase 1** | Engine: GGUF load, tokenize, logits, KV-cache, warmup | `crates/engine/lib.rs` | **Done** (Loop 1-2) |
| **Phase 2** | Pipeline: constrained-readout (single-token softmax over options) | `crates/pipeline/readout.rs` | **Done** (Loop 2) |
| **Phase 3** | Pipeline: generation (greedy, strip `<think>...</think>`) | `crates/pipeline/generate.rs` | **Done** (Loop 2) |
| **Phase 4** | CLI: bench subcommand, run both pipelines, JSON output | `apps/cli/src/main.rs` | **Done** (Loop 2), re-verified for 3 models (Loop 4) |
| **Phase 5** | HTTP API: `POST /bench`, `GET /health` | `apps/server/src/main.rs` | **Done** (Loop 3), panic-supervised worker + 3-model verification (Loop 4) |
| **Phase 6** | Tests: unit + integration (real model), coverage | `tests/` (new) | Partial — 3 unit tests in `pipeline/readout.rs`; `generate.rs` (G10) and `apps/server` (G12) have no committed unit tests (black-box HTTP validation covers `apps/server` instead). Not blocking so far. |
| **Phase 7** | E2E server tuning, deployment automation | — | Laya scoring now wired (Loop 5); load/perf tuning proper still not started |

### Critical Unresolved Items (Phase 0) — resolution status
1. **Model repo IDs** — resolved. Exact registry (`crates/models/src/download.rs::REGISTRY`): `qwen3-0.6b`→`Qwen/Qwen3-0.6B-GGUF`, `qwen3-4b`→`Qwen/Qwen3-4B-GGUF`, `minicpm5-2b`→`openbmb/MiniCPM5-2B-GGUF` (note: `MiniCPM5`, not the originally-assumed `MiniCPM-2B`), `laya`→`mys/laya-GGUF`. All 4 pinned to a resolved commit SHA.
2. **hf-hub API shape** — resolved. `HFClientSync::model(owner,name).download_file().filename(..).revision(..).send()`, cache-aware (no network request on a warm cache), confirmed via 3 real downloaded/reused models.
3. **llama-cpp-2 logits API** — resolved. `ctx.get_logits_ith(i)` confirmed working against 3 real models.
4. **llama-cpp-2 chat template** — resolved. `model.chat_template(None)` + `apply_chat_template` used when present; hand-rolled ChatML fallback otherwise. All 3 exercised models (Qwen3 family + MiniCPM5) use a ChatML-compatible template — the fallback path exists but hasn't been forced/observed on a model that actually lacks GGUF chat-template metadata.
5. **KV-cache reset between pipelines** — resolved. `Engine::reset_context()` → `ctx.clear_kv_cache()`, called between readout/generate AND at the top of every `apps/server` request (cross-request isolation, Loop 3 fix).

---

## Known Issues

Validation Round 1 (Phase 0 planning-time) findings below are historical/superseded — the plan structure they describe (`src/pipeline/mod.rs`, `src/cli/mod.rs`, Phase numbering) was replaced by the actual `crates/`+`apps/` workspace layout and the HoH loop process. Kept for history; see `plans/20260922-2146-openjev-rust-implementation/reports/` for the original detail.

**Current known issues** (tracked in `plans/20260922-2328-hoh-openjev-rs/loops/issue-ledger.md`):
- **G9** — this file and `docs/project-overview-pdr.md` were stale relative to code; Loop 4 refreshed both, Loop 5 refreshed the Laya-specific parts.
- **G10** — `crates/pipeline/generate.rs` has no committed unit tests (black-box CLI/HTTP validation covers it instead).
- **G12** — `apps/server/src/{app,main}.rs` has no committed unit tests (black-box HTTP validation covers it instead).
- **G14** — `apps/server` binds port 80 (root, no auth) instead of an unprivileged port; accepted deviation, documented residual risk (root-bind, future reverse-proxy collision, more-probed port).
- **G15** — `apps/server/src/app.rs`'s `panic_message()` was passing `&payload` instead of `&*payload`, so `downcast_ref` never matched real `&str`/`String` panic payloads (always logged `<non-string panic payload>`). Fixed in Loop 5 (`panic_message(&*payload)`); verified via fault-injection — real panic message now logs correctly.
- **Laya — RESOLVED (Loop 5).** `crates/pipeline/laya.rs`/`crates/models/laya.rs` are implemented: a blocking HTTP client to a persistent `laya serve` process (see README's "Laya Server Process"), NOT the generic `ggmlc-run` CLI Loop 1 explored and NOT a per-request shell-out. `laya_model_load_ms` stays 0 by design (model loads once at `laya serve` startup); `laya_inference_ms` is now genuinely populated (~7.5-8.9s per call on the target CPU server, warm). `--skip-laya`/`skip_laya` is real (was a documented no-op through Loop 4). Verified end-to-end via `apps/cli` and external `apps/server` `/bench` curl, including graceful degradation (`laya: null`, no 500) when `laya serve` is down, and self-heal on its next successful response.

**Historical (Phase 0 planning-time) findings, superseded:**
- Structural BLOCKERs (2): `src/pipeline/mod.rs` & `src/cli/mod.rs` written by parallel phases; contradicted the original plan.md "no overlap" claim — moot, actual layout is `crates/pipeline`, `apps/cli`.
- Consistency BLOCKERs (2): `Timings` consumed before defined; `Engine::sample_greedy()` needed but undeclared — both resolved in the actual implementation (`Timings` lives in `crates/timing`, defined first; `Engine::sample_greedy` is implemented in `crates/engine/src/lib.rs`).
- Testability BLOCKER (1): Phase 0 "no tests" — moot, Phase 0 itself has no surviving artifact; see G10/G12 above for the current, real test-coverage gaps.

---

## Key Characteristics

- **Workspace:** 6-crate, single-resolver
- **Executables:** 2 (`openjev-cli`, `openjev-server`)
- **Models Registry:** 4 hardcoded entries (`qwen3-0.6b`, `qwen3-4b`, `minicpm5-2b`, `laya`); all 4 exercised end-to-end via both binaries (LLM models Loop 4, Laya Loop 5)
- **Benchmark Dimensions:** 7 timed phases (model-load, warmup, tokenize, constrained-readout, generation, laya-model-load, laya-inference) — `laya_model_load_ms` stays `0` by design (Laya's model loads once at `laya serve` startup, not per-request); `laya_inference_ms` is now genuinely populated (Loop 5) unless `--skip-laya`/`skip_laya` is set
- **Inference Runtime:** llama.cpp via `llama-cpp-2` (CPU-only, quantized GGUF), one process-wide shared `LlamaBackend` (Loop 4 fix enabling multi-model caching in one process)
- **Scoring:** Laya via a persistent `laya serve` HTTP process (separate `laya` binary, built from `examples/laya` in the `ggmlc` repo) + a blocking `reqwest` client in `crates/models` — not the generic `ggmlc-run` CLI, not a per-request shell-out (corrected in Loop 5)
- **Code Maturity:** `engine`/`models`/`pipeline`/`apps/cli`/`apps/server` all implemented and running real inference, including Laya as of Loop 5; live externally-reachable server on `103.146.166.46:80`; remaining gaps are test coverage (G10/G12, non-blocking, black-box-covered)

---

Last updated: 2026-09-23 (Loop 5, HoH)
Plan: `plans/20260922-2328-hoh-openjev-rs/` (current); `plans/20260922-2146-openjev-rust-implementation/plan.md` (original, superseded — see Known Issues)
Research: `docs/research/openjev-rust-research.md`
