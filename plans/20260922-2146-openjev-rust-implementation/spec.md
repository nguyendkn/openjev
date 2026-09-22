# Spec: openjev-rs

See `plan.md` for full context, decisions, and architecture rationale.

## Product
Rust workspace comparing 3 ways to answer a multiple-choice question with a local model,
CPU-only: (1) constrained single-token logit readout (llama-cpp-2), (2) JSON generation
(llama-cpp-2), (3) Laya single-pass encoder scoring (`ggmlc-run` shell-out). Two binaries:
`openjev-cli` (one-shot), `openjev-server` (axum HTTP).

## Must verify before coding (Phase 0 — blocks everything)
8 unresolved items: MiniCPM/Qwen-4B exact HF repo IDs, `hf-hub` crate API, `llama-cpp-2`
logits/chat-template API, model licensing, `ggmlc-run` CLI syntax + Windows build + CPU
latency sanity. See `plan.md` § Unresolved — resolve with primary-source evidence, never guess.

## Success criteria (verifiable)
- `cargo build --workspace` exits 0 (Windows, CPU-only; 6 crates: apps/cli, apps/server,
  crates/{engine,models,pipeline,timing}).
- `openjev-cli --model qwen3-0.6b --prompt "..." --options A,B --format json` → valid JSON,
  7 `Timings` fields + all 3 method results.
- `openjev-server --port 8080`, `POST /bench` → same `BenchReport` shape; `GET /health` → 200.
- `--skip-laya` / `skip_laya:true` → `laya` null, rest unchanged.
- `cargo test --workspace` (fast, no network/model download) passes.
- `cargo test --workspace --features integration` (real models cached locally) passes all
  Phase 1-5 scenarios.
- Diff coverage ≥90% line / ≥75% branch on Phases 1-5's new code.
- Phase 7: all 3 LLMs + Laya run successfully on the real Linux server (Xeon Gold 5320, 32
  vCPU), ≥1 tuning iteration recorded (before/after `Timings`), `docs/benchmarks/
  server-tuning-results.md` written with a concrete recommended config.

## Key files
- `plan.md`, `phase-00-spike-verification.md` … `phase-07-e2e-server-tuning.md` (this dir).
- `docs/research/openjev-rust-research.md` (original SemIf architecture, what to preserve).
