# Goal: openjev-rs — Rust port of OpenJev/SemIf + Laya comparison

> **Execution flow: /vk-hoh-run** — spec = `plans/20260922-2146-openjev-rust-implementation/spec.md`, loops = 3.
> Inner loop: /vk-hoh-plan → /vk-hoh-dev → /vk-hoh-qa per loop × T.

## Mission
Build a Rust CPU-only workspace (`openjev-cli` + `openjev-server` binaries, 4 shared lib
crates) that benchmarks 3 ways to answer a multiple-choice question with a local model:
llama-cpp-2 constrained-readout, llama-cpp-2 JSON generation, and Laya (`ggmlc-run`)
single-pass encoder scoring — matching/extending openjev.com/SemIf's original 2-way benchmark.

## Context & Key Files
- Full plan: `plans/20260922-2146-openjev-rust-implementation/plan.md`
- Phases, in order (0 blocks all): `phase-00-spike-verification.md` →
  `phase-01-engine-model-management.md` → {`phase-02-pipeline-constrained-readout.md`,
  `phase-03-pipeline-generation.md`} → {`phase-04-cli-bench.md`, `phase-05-http-api-serve.md`}
  → {`phase-06-test-suite-ci.md`, `phase-07-e2e-server-tuning.md`}
- Original architecture reference: `docs/research/openjev-rust-research.md`

## Requirements
**Must do:**
- Phase 0 FIRST: resolve all 8 Unresolved items in `plan.md` with primary-source evidence
  (repo IDs, `hf-hub` API, `llama-cpp-2` logits API, `ggmlc-run` CLI/build) before Phase 1+ —
  never guess.
- Workspace: `apps/cli` (bin `openjev-cli`), `apps/server` (bin `openjev-server`),
  `crates/{engine,models,pipeline,timing}` (libs). No `[package]` at workspace root.
- 3 comparison methods per request, all returned by default (`--skip-laya`/`skip_laya` to opt
  out): constrained-readout, JSON generation (strip `<think>`, validate schema, max 512
  tokens), Laya scoring (shell-out to `ggmlc-run`, not FFI).
- CPU-only build: no cuda/vulkan/rocm/opencl features on llama-cpp-2. `n_threads`/batch-size
  configurable, default near core count.
- `Timings`: 7 fields (model_load, warmup, tokenize, constrained_readout, generation,
  laya_model_load, laya_inference), serde-serializable, identical shape in CLI/HTTP output.
- 4 models: Qwen3-0.6B (Q8_0), MiniCPM "2B" + Qwen "4B" (Q4_K_M default, exact repo IDs per
  Phase 0), Laya-en (`mys/laya-GGUF`, UD_Q4_K_M).
- Phase 7: deploy + benchmark on the real Linux server (Xeon Gold 5320, 32 vCPU, SSH — no
  confirmation gate needed), tune n_threads/build flags, write
  `docs/benchmarks/server-tuning-results.md`.

**Must not:**
- Don't use candle; don't add GPU build features; don't skip Phase 0 verification; don't add
  auth/rate-limiting (not requested); don't guess `ggmlc-run`'s CLI contract or model repo IDs
  without checking a primary source.

## Success Criteria
- `cargo build --workspace` exits 0 (Windows, CPU-only).
- `openjev-cli --model qwen3-0.6b --prompt "..." --options A,B --format json` prints valid
  JSON, 7 timing fields + 3 method results.
- `openjev-server --port 8080` + `POST /bench` returns same `BenchReport` shape; `GET /health`
  → 200.
- `cargo test --workspace` passes (no network); `cargo test --workspace --features
  integration` passes with cached models.
- Diff coverage ≥90% line / ≥75% branch on Phases 1-5's new code.
- Phase 7: all 3 LLMs + Laya run successfully on the real server; ≥1 tuning iteration recorded
  with before/after `Timings`.

## Out of Scope
- GPU inference, authentication/rate-limiting, `mys/laya-multilingual-GGUF`, horizontal
  scaling/multi-instance state.

## Verification
```bash
cargo build --workspace
cargo test --workspace
cargo test --workspace --features integration
./target/release/openjev-cli --model qwen3-0.6b --prompt "The capital of France is:" --options A,B --format json
```
