# OpenJev-rs Codebase Summary

Rust port of OpenJev/SemIf (CPU-only LLM benchmark: constrained-readout vs generation). **Phase 0 stage** — skeleton only, no business logic. See `docs/research/openjev-rust-research.md` for reference architecture.

---

## Directory Tree

```
openjev/
├── apps/
│   ├── cli/              → openjev-cli binary (one-shot benchmark run, --skip-laya planned)
│   └── server/           → openjev-server binary (axum HTTP API: POST /bench, GET /health)
├── crates/
│   ├── timing/           → Timings struct only (0 intra-workspace deps) [IMPLEMENTED]
│   ├── models/           → registry + hf-hub + ggmlc-run wrapper (0 intra-workspace deps) [STUB]
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

### `crates/models` — **STUB**
| Aspect | Details |
|--------|---------|
| Files | `lib.rs`, `download.rs`, `laya.rs`, `error.rs` (all stubs, 1–2 lines each) |
| Modules Declared | `download`, `laya`, `error` |
| Purpose | Model registry (4 entries: Qwen3-0.6B, MiniCPM-2B, Qwen3-4B, Laya-en), hf-hub download wrapper, ggmlc-run shell-out for Laya scoring |
| Dependencies | `hf-hub` 1.0, `serde` 1.0 |
| Deps Within Workspace | None |
| Status | Interface planned; no code yet |

### `crates/engine` — **STUB**
| Aspect | Details |
|--------|---------|
| Files | `lib.rs` (1 line: `pub mod error;`), `error.rs` (1 line comment) |
| Purpose | GGUF loader via `llama-cpp-2`, LlamaContext wrapper, logits/KV-cache APIs |
| Dependencies | `llama-cpp-2` 0.1, `models`, `timing` |
| Deps Within Workspace | `models`, `timing` |
| Status | Interface & research verified (Phase 0); implementation deferred to Phase 1 |

### `crates/pipeline` — **STUB**
| Aspect | Details |
|--------|---------|
| Files | `lib.rs` (4 lines: module decls), plus `readout.rs`, `generate.rs`, `laya.rs`, `error.rs` (all stubs) |
| Modules Declared | `readout`, `generate`, `laya`, `error` |
| Purpose | 3 pipelines: constrained single-token readout (Phase 2), greedy generation (Phase 3), Laya scoring (Phase 3) |
| Dependencies | `engine`, `models`, `timing` |
| Deps Within Workspace | `engine`, `models`, `timing` |
| Status | Interface & test scenarios planned; no implementation yet |

### `apps/cli` — **STUB**
| Aspect | Details |
|--------|---------|
| Files | `src/main.rs` (1 line: println stub), `Cargo.toml` (package + bin name + deps) |
| Binary Name | `openjev-cli` |
| Purpose | One-shot benchmark runner; CLI args via clap derive |
| Dependencies | `clap` 4.4 (derive), `serde` 1.0, `serde_json` 1.0, + all core crates |
| Deps Within Workspace | `engine`, `models`, `pipeline`, `timing` |
| Status | Skeleton only; subcommand enum & main loop in Phase 4 (phase-04-cli-bench.md) |

### `apps/server` — **STUB**
| Aspect | Details |
|--------|---------|
| Files | `src/main.rs` (1 line: println stub), `Cargo.toml` (package + bin name + deps) |
| Binary Name | `openjev-server` |
| Purpose | axum HTTP API server; `POST /bench`, `GET /health` endpoints |
| Dependencies | `clap` 4.4 (derive), `axum` 0.7, `tokio` 1.0 (full features), `serde` 1.0, `serde_json` 1.0, + all core crates |
| Deps Within Workspace | `engine`, `models`, `pipeline`, `timing` |
| Status | Skeleton only; route handlers & `tokio::task::spawn_blocking` pattern in Phase 5 (phase-05-http-api-serve.md) |

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
         └─── (no external crate deps declared yet; readout/generate/laya logic TBD)

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

| Phase | Scope | Target File(s) | Status |
|-------|-------|-----------------|--------|
| **Phase 0** | Spike verification (model paths, API shapes) | — | In progress (validation round 1 complete; 2 BLOCKERs + 4 MINORs flagged) |
| **Phase 1** | Engine: GGUF load, tokenize, logits, KV-cache, warmup | `crates/engine/lib.rs` | Planning (depends: Phase 0 facts) |
| **Phase 2** | Pipeline: constrained-readout (single-token softmax over options) | `crates/pipeline/readout.rs` | Planned (depends: Phase 1) |
| **Phase 3** | Pipeline: generation (greedy, max 512 tokens, strip `<think>...</think>`) | `crates/pipeline/generate.rs` | Planned (depends: Phase 1) |
| **Phase 4** | CLI: bench subcommand, run both pipelines, JSON output | `apps/cli/src/main.rs` | Planned (depends: Phase 2, 3) |
| **Phase 5** | HTTP API: `POST /bench`, `GET /health`, spawn_blocking inference | `apps/server/src/main.rs` | Planned (depends: Phase 2, 3) |
| **Phase 6** | Tests: unit + integration (real model), coverage ≥90% line / ≥75% branch | `tests/` (new) | Planned (depends: Phase 1–5) |
| **Phase 7** | E2E server tuning, deployment automation | — | Deferred |

### Critical Unresolved Items (Phase 0)
1. **Model repo IDs** — exact HF repo for "MiniCPM-2B" & "Qwen3-4B" (both UNVERIFIED / INFERRED from search)
2. **hf-hub API shape** — exact download signature & model caching behavior (UNVERIFIED)
3. **llama-cpp-2 logits API** — exact method names for `get_logits_ith` & KV-cache reset (Inferred, not confirmed)
4. **llama-cpp-2 chat template** — whether `llama_chat_apply_template` is wrapped (UNVERIFIED; fallback: hand-rolled ChatML)
5. **KV-cache reset between pipelines** — which method to call, when to call it (UNVERIFIED)

All blocking items listed in `phase-00-spike-verification.md:28-79` must be closed before Phase 1 starts.

---

## Known Issues

**Validation Round 1 Findings:**

- **Structural BLOCKERs (2):** `src/pipeline/mod.rs` & `src/cli/mod.rs` written by parallel phases; contradicts plan.md "no overlap" claim (phase files themselves acknowledge coordination needed).
- **Consistency BLOCKERs (2):**
  - `Timings` type consumed by Phase 2/3 before it's defined (Phase 4 owns it).
  - Phase 3 needs `Engine::sample_greedy()` that Phase 1 never declares.
- **Testability BLOCKER (1):** Phase 0 declares "no tests" but is `lane: normal` (not exempted from quality gate).

See `plans/20260922-2146-openjev-rust-implementation/reports/` for full validation details.

---

## Key Characteristics

- **Workspace:** 6-crate, single-resolver
- **Executables:** 2 (`openjev-cli`, `openjev-server`)
- **Models Registry:** 4 hardcoded entries (Qwen3-0.6B Q8_0, MiniCPM-2B, Qwen3-4B, Laya-en)
- **Benchmark Dimensions:** 7 timed phases (model-load, warmup, tokenize, constrained-readout, generation, laya-model-load, laya-inference)
- **Inference Runtime:** llama.cpp via `llama-cpp-2` (CPU-only, quantized GGUF)
- **Scoring:** External `ggmlc-run` CLI for Laya (no FFI)
- **Code Maturity:** All stubs except `crates/timing` (27 lines) — Phase 0 architectural validation in progress

---

Last updated: 2026-09-22  
Plan: `plans/20260922-2146-openjev-rust-implementation/plan.md`  
Research: `docs/research/openjev-rust-research.md`
