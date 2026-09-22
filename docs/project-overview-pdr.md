# OpenJev-rs Product/Design Requirements

**Version:** 0.1 (Phase 0 baseline; requirements below still reflect original v1 design intent)
**Last updated:** 2026-09-23 (Loop 4, HoH — see status note)
**Status:** Phase 0 spike items resolved; engine/models/pipeline/apps/cli/apps/server all implemented and running real inference for `qwen3-0.6b`, `qwen3-4b`, `minicpm5-2b` (both CLI and a live external HTTP server on `103.146.166.46:80`). Laya (item 3 of the 3-way comparison — encoder scoring via `ggmlc-run`) is NOT yet implemented; `laya_model_load_ms`/`laya_inference_ms` are always `0`. See `docs/codebase-summary.md` and `plans/20260922-2328-hoh-openjev-rs/loops/` for current, authoritative implementation status — this document's Functional Requirements/Acceptance Criteria/Open Risks sections below are the *original* v1 design targets and are only lightly annotated with resolution status, not rewritten.

## Problem Statement

Existing OpenJev (browser tool) compares two methods of answering multiple-choice questions with local LLMs:
1. **Constrained single-token readout:** one forward pass, logits restricted to option labels, softmax after restriction. Fast but potentially uncalibrated.
2. **JSON generation:** free-form greedy generation (max 512 tokens), stripped of reasoning tags, schema validated. Flexible but slower, multi-pass.

**Gap:** No systematic CPU-only comparison of these methods with a third candidate—Laya single-pass encoder classification (ModernBERT, claims calibrated probabilities)—at scale on production hardware. Browser tool not suitable for benchmarking; CLI + HTTP API needed for automation, reproducibility, and server deployment.

**Scope:** Rust implementation (CPU-only v1) of OpenJev + Laya comparison, deployed as two independent binaries (one-shot CLI + HTTP API server) on a shared inference library stack. Preserve original methodology, add calibration contrast via encoder path.

## Goals

- **Port OpenJev benchmark logic** to native Rust, maintaining methodological fidelity to the original.
- **Extend to 3-way comparison:** constrained readout, JSON generation, Laya single-pass scoring.
- **Establish CPU-only baseline** on Linux server (Intel Xeon Gold 5320, 32 vCPU, 62GB RAM) with ≥1 tuning iteration per method.
- **Ship two separate binaries:** `openjev-cli` (one-shot runs) + `openjev-server` (HTTP API), both depending on shared library crates.
- **Enable reproducible benchmarking:** all 3 models (Qwen3-0.6B, MiniCPM "2B", Qwen3 "4B") + Laya fully supported from v1.

## Non-Goals

- Multi-GPU inference, CUDA/ROCm/OpenCL v1 (deferred).
- Laya multilingual variant (`laya-multilingual-GGUF`, ModernBERT-base) v1 (documented future extension).
- Web UI (browser OpenJev remains separate reference; this is CLI/API only).
- Batching across multiple questions in a single inference pass (sequential per question).
- LLM fine-tuning or model quantization (use pre-quantized GGUFs only).

## Target Users

- **ML practitioners:** benchmark local LLM inference methods, compare calibration claims, optimize thread/batch tuning for their hardware.
- **Inference framework developers:** validate llama.cpp port correctness and performance profile, compare constrained-readout vs autoregressive paths.
- **DevOps/infrastructure teams:** deploy standardized benchmarks for new server hardware, track regression across updates.

## Functional Requirements

### CLI Binary (`openjev-cli`)

**Interface:** flat argument list (no subcommands; the only behavior is bench run).

```
openjev-cli \
  --model {qwen-0.6b,minicpm-2b,qwen-4b} \
  --prompt "What is X?" \
  --options "A" "B" "C" "D" \
  [--threads <n>] \
  [--batch-size <n>] \
  [--skip-laya] \
  [--format {json,text}]
```

**Outputs:** timing + all 3 method results as JSON (default) or human text.

**Must-have:**
- Download models on first run (hf-hub, cache, SHA256 checksum).
- Load GGUF via llama-cpp-2, configure n_threads (default near available core count, tunable).
- Run warmup pass (1 empty forward pass, discard), then measure: tokenize, constrained readout, reset context, JSON generation, optionally Laya.
- Return `Timings` struct (7 fields: model_load, warmup, tokenize, readout, generation, laya_model_load, laya_inference) + per-method results (logits, JSON, Laya scores).
- `--skip-laya` flag: omit Laya from run (for testing, or if Laya broken).

### HTTP Server (`openjev-server`)

**Interface:** axum server exposing `/bench` (POST), `/health` (GET).

**POST /bench** — request shape:
```json
{
  "model": "qwen-0.6b" | "minicpm-2b" | "qwen-4b",
  "prompt": "...",
  "options": ["A", "B", "C", "D"],
  "threads": <int, optional>,
  "batch_size": <int, optional>,
  "skip_laya": <bool, default false>
}
```

**Response shape:** `BenchReport` (same fields as CLI output: Timings + 3 results).

**Must-have:**
- Singleton `Arc<Mutex<AppState>>` (inference is inherently sequential; one request at a time).
- `tokio::task::spawn_blocking` around engine calls (llama-cpp-2 is not async-native).
- **As-implemented deviation (Loop 3), same effective guarantee:** `Arc<Mutex<...>>` + `spawn_blocking` was not viable — `Engine` wraps raw `llama-cpp-2` FFI pointers and is `!Send`, which `spawn_blocking`'s `F: Send` bound rejects. Implemented instead as one dedicated OS thread owning `HashMap<String, Engine>`, reached via `mpsc`/`oneshot` channels from the async handler — still fully serializes inference, `Engine` never crosses a thread boundary. Loop 4 added panic supervision (G13): the worker thread is wrapped in a `catch_unwind` supervisor that respawns it with an empty model cache on any panic, so a single bad request can't permanently kill inference. See `apps/server/src/app.rs`'s `AppState`/`supervisor_loop` doc comments.
- `GET /health` returns `{"status": "ok"}`.
- Graceful error handling per method: a Laya failure does not abort readout/generation results (flag partial result).

### Shared Library Stack

**`timing` crate:** `Timings` struct only, 7 fields, `#[derive(Serialize)]`. Zero intra-workspace deps.

**`models` crate:** 
- Static registry: 4 entries (Qwen3-0.6B/Q8_0, MiniCPM-2B/Q4_K_M default, Qwen3-4B/Q4_K_M default, Laya-en/UD_Q4_K_M).
- hf-hub integration: download, cache, SHA256 checksum validation.
- Laya scoring wrapper: shell-out to `ggmlc-run` CLI (no FFI, no separate crate).
- Zero intra-workspace deps.

**`engine` crate:**
- GGUF loader via llama-cpp-2.
- `LlamaContext` wrapper (CPU-only build, n_threads configurable).
- `sample_greedy()` for generation.
- `reset_context()` for state isolation between runs.
- Depends: `models`, `timing`.

**`pipeline` crate (3 submodules):**
- `readout`: constrained softmax over candidate-label logits only (BEFORE softmax, not after).
- `generate`: greedy sampling, max_tokens=512, strip `<think>...</think>`, validate JSON schema.
- `laya`: call into `models::laya::score()` (shell-out, don't duplicate).
- Depends: `engine`, `models`, `timing`.

## Acceptance Criteria by Phase

| Phase | Acceptance Criteria |
|---|---|
| **0: Spike** | Verify 8 unresolved items with primary-source evidence (repo source, `cargo doc`, `--help`, HF model card). Resolve workspace skeleton build (Cargo.toml structure, Rust 2021, Windows build validates). Do NOT guess. Block all later phases if any item remains unresolved. |
| **1: Engine/Models/Timing** | ✓ Load any of 3 models via llama-cpp-2. ✓ Download + cache + checksum via hf-hub. ✓ CPU-only build (no GPU features). ✓ `n_threads` configurable. ✓ E2E: `engine::test_load_qwen_and_generate_one_token()` passes (real model, one forward pass, check logit shape). ✓ ggmlc-run wrapper compiles, `LayaRunner::score()` shell-out signature defined. ✓ Timings struct populated, all 7 fields captured. |
| **2: Pipeline — Constrained Readout + Laya** | ✓ Given prompt + options, constrain logits to option label token IDs BEFORE softmax, return probabilities. ✓ Single forward pass, KV-cache reset after use. ✓ Laya: shell-out to `ggmlc-run`, parse output, return probabilities. ✓ E2E: `pipeline::test_readout_three_models()` passes (all 3 models, identical prompt, compare logit order). ✓ E2E: `pipeline::test_laya_scoring()` passes (Laya model load + score, check latency order-of-magnitude). |
| **3: Pipeline — JSON Generation** | ✓ Greedy sampling, max_tokens=512. ✓ Strip `<think>...</think>` tags. ✓ Validate JSON schema against option set (reject if malformed). ✓ Return probabilities (conditional on shown options, NO calibration claim). ✓ E2E: `pipeline::test_generate_three_models()` passes (all 3 models, valid JSON from each, compare speed). |
| **4: CLI Binary** | ✓ Parse args (model, prompt, options, threads, batch, skip-laya, format). ✓ Orchestrate all 3 pipelines in sequence. ✓ Output JSON (default) + text format. ✓ E2E: `openjev-cli --model qwen-0.6b --prompt "Q?" --options A B C D` completes, all 3 results present (or Laya skipped if --skip-laya). ✓ Exit code 0 on success, non-zero on error. |
| **5: HTTP Server** | ✓ POST /bench accepts all 3 methods. ✓ GET /health returns status. ✓ Arc<Mutex> serializes inference. ✓ spawn_blocking isolates llama-cpp-2. ✓ Error handling: one method's failure does NOT abort others. ✓ E2E: `curl -X POST http://localhost:8080/bench -d {...}` returns BenchReport, all 3 results (or partial if Laya errors). |
| **6: Tests + CI** | ✓ ≥90% line coverage on new code. ✓ ≥75% branch coverage. ✓ Unit tests per crate. ✓ E2E scenarios (load model, score, compare results). ✓ GitHub CI workflow: test on Windows (dev), Linux (server target). ✓ Pre-commit: fmt, clippy. |
| **7: E2E + Tuning** | ✓ Deploy both binaries to Linux server (Xeon Gold 5320). ✓ Benchmark all 3 models × all 3 methods (9 combinations). ✓ ≥1 tuning iteration: vary n_threads, batch_size, measure latency per method. ✓ Document baseline numbers + tuning decisions in `docs/benchmarks/server-tuning-results.md`. ✓ Laya CPU latency measured (not assumed). |

## Success Metrics

- **Latency baselines (Phase 7 to establish):**
  - Constrained readout: <50ms per decision (all 3 models, single forward pass).
  - JSON generation: <500ms per decision (all 3 models, max 512 tokens).
  - Laya: <150ms per decision (ModernBERT, single forward pass, order-of-magnitude from GPU baseline ~25ms).
- **Coverage:** ≥90% line, ≥75% branch on all 6 crates.
- **Reproducibility:** same hardware, same tuning → within ±5% latency variance across 3 runs.
- **Calibration contrast:** document whether Laya's claimed "calibrated probabilities" exceed constrained-readout's empirically in practice (Phase 7 analysis, not a pass/fail gate).

## Constraints

- **CPU-only inference v1** (no CUDA/ROCm/OpenCL; may add later if demand).
- **Dev environment:** Windows (MSVC, vcpkg), target environment: Linux (gcc/apt).
- **All 3 models in scope v1** (none optional); Laya optional only via `--skip-laya` flag for troubleshooting.
- **Workspace structure:** fully separate `apps/cli` and `apps/server` binaries (no shared entrypoint files), shared library crates only.
- **Constrained readout:** logits restricted BEFORE softmax (not full-vocab softmax then filter).
- **Generation:** greedy only (no beam search v1).
- **No GPU feature flags enabled** in llama-cpp-2 default build; n_threads tuned near available core count, not conservative low default.

## Open Risks / Phase 0 Unresolved Items

**8 items requiring primary-source verification before Phase 1 starts. Do NOT guess; block all other phases if unresolved.**

| # | Item | Current State | Fallback (if source ambiguous) | Verification Method |
|---|---|---|---|---|
| 1 | MiniCPM HF repo ID | **RESOLVED** — `openbmb/MiniCPM5-2B-GGUF`, file `MiniCPM5-2B-Q4_K_M.gguf`. Registry id is `minicpm5-2b` (not `minicpm-2b`) — matches the predicted fallback exactly. Downloaded, cached, verified via real inference (Loop 4). | `openbmb/MiniCPM5-2B-GGUF` | Check OpenJev GitHub source (workszop/openjev, worker.js model config) for exact repo ID used; if ambiguous, use fallback + record reasoning |
| 2 | Qwen "4B" HF repo ID | **RESOLVED** — `Qwen/Qwen3-4B-GGUF`, file `Qwen3-4B-Q4_K_M.gguf`, registry id `qwen3-4b`. Matches the predicted fallback exactly. Downloaded, cached, verified via real inference (Loop 4). | `Qwen/Qwen3-4B-GGUF` (matches 0.6B family) | Check OpenJev GitHub source; if ambiguous, use fallback + record reasoning |
| 3 | hf-hub crate API | **RESOLVED** — `hf-hub` 1.0, `HFClientSync::new().model(owner,name).download_file().filename(..).revision(..).send()`; cache-aware, confirmed via 4 real cached models (`crates/models/src/download.rs`). | Resolve via `cargo add hf-hub --dry-run`, then read `cargo doc --open` or docs.rs for pinned version | Read docs.rs / local `cargo doc` against pinned version (do NOT code blind against summaries) |
| 4 | llama-cpp-2 logits + KV-cache API | **RESOLVED** — `ctx.get_logits_ith(i)`, `ctx.clear_kv_cache()`, `model.chat_template(None)` + `apply_chat_template` (ChatML fallback if absent) — all confirmed working against 3 real models (`crates/engine/src/lib.rs`). Loop 4 additionally discovered+fixed a process-wide `LlamaBackend::init()`-can-only-succeed-once constraint not anticipated here (see `docs/codebase-summary.md` `crates/engine` entry). | Hand-build ChatML prompts (Qwen3/MiniCPM both use `<\|im_start\|>role` framing) if no wrapper exists | Read `cargo doc --open` or `llama-cpp-2/src/context.rs` + `src/model.rs` source directly |
| 5 | Model licensing | **RESOLVED** — all 4 registry entries report `apache-2.0` via HF's `cardData.license` field (`crates/models/src/download.rs` header comment). | Keep NOTICE file if Apache-2.0 confirmed; verify in Phase 0/1 before public distribution | Read HF license metadata field for each pinned repo |
| 6 | ggmlc-run CLI syntax | Still open — `ggmlc-run` binary is built and present on the server (`/tmp/ggmlc/build/runtime/ggmlc-run`), but the scoring subcommand/flags are not wired into `crates/models::laya` yet. Loop 5+. | N/A (unguessable; must resolve) | Read `ggmlc-run --help` after building (see #7) or Laya model card's usage snippet on HF |
| 7 | ggmlc-run Windows build | Not applicable to current scope — dev/build/run all happen on the Linux target server, not the Windows dev machine, for this project so far. | N/A (same risk category as llama-cpp-2, not new) | Clone github.com/monatis/ggmlc, build on Windows dev machine, verify `ggmlc-run` binary runs |
| 8 | Laya CPU latency | Still open — not measured; blocked on #6 (Laya scoring path unimplemented). | N/A (must measure) | Once #6/#7 resolved, run one quick sanity timing on a test system (not full Phase 7 tuning yet) to establish order-of-magnitude |

**Phase 0 exit criteria:** 5 of 8 items resolved with evidence (1-5, all LLM/inference-path items — no guessing was needed, registry ids matched the documented fallbacks exactly). Items 6-8 (all Laya-specific) remain open, tracked for Loop 5+, not blocking the LLM-only work done through Loop 4.

## Architecture

See `workflows/documentation-management.md` and plan file `plans/20260922-2146-openjev-rust-implementation/plan.md` § Architecture for full workspace layout, crate responsibilities, and phase dependency graph.

**Abbreviated:**
- `timing`: Timings struct only.
- `models`: registry + hf-hub + ggmlc-run wrapper (merged, not separate crate).
- `engine`: GGUF load, LlamaContext, sample_greedy, reset_context.
- `pipeline`: readout.rs, generate.rs, laya.rs submodules.
- `apps/cli`, `apps/server`: fully separate binaries.

## Implementation Roadmap

**7 phases:** 0 (spike) → 1 (engine/models/timing) → {2, 3 in parallel} (pipeline readout+Laya, generation) → {4, 5 in parallel} (CLI, server) → {6, 7 in parallel} (tests+CI, e2e+tuning).

Effort: 8–11 days (Phase 0 ~2 days; Phase 1 ~3 days; Phases 2–5 ~3 days; Phases 6–7 ~2 days parallel).

See phase files in `plans/20260922-2146-openjev-rust-implementation/` for detailed acceptance criteria, success scenarios, and blockers per phase.
