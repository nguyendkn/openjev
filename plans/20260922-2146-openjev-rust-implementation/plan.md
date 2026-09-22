---
title: "openjev-rs: Rust port of OpenJev/SemIf + Laya comparison (CLI + HTTP server)"
description: "Port OpenJev/SemIf's constrained-readout vs generation benchmark, plus Laya single-pass encoder scoring, to a Rust workspace (apps/cli + apps/server binaries, shared engine/laya/models/pipeline/timing crates) on llama-cpp-2 + ggmlc-run, CPU-only."
status: pending
priority: P2
effort: 8-11d
lane: normal
branch: main
tags: [feature, backend, cli, api, llm, rust]
created: 2026-09-22
---

## Overview

Port [openjev.com](https://openjev.com) (SemIf) — a browser tool that benchmarks two ways an LLM
answers a multiple-choice question: **constrained single-token readout** (1 forward pass, softmax
over candidate label logits only) vs **JSON generation** (model free-generates
`{"option": prob, ...}`, strip `<think>`, validate) — to a native Rust workspace with two binaries
(`openjev-cli`, `openjev-server`) sharing library crates. **Extended (final user direction) to
compare a third method**: Laya (`mys/laya-GGUF`), a parallel single-pass encoder-classification
project that claims *calibrated* probabilities — a direct contrast to SemIf's own "not calibrated"
disclaimer for constrained-readout. Engines: `llama-cpp-2` (thin bindings, same underlying runtime
family as the original's wllama/llama.cpp WASM) for the 3 LLMs, `ggmlc-run` CLI shell-out for Laya
(its GGUF uses the `ggmlc` format, which `llama-cpp-2` cannot read). CPU-only inference throughout.
Greenfield project at `C:\Users\nguyendk\Documents\Projects\openjev`, no existing code.

Citations: `docs/research/openjev-rust-research.md` (original architecture, what to preserve),
`research/researcher-01-llama-cpp-2-engine.md` (engine API), `research/researcher-02-cli-http-architecture.md`
(crate layout, axum+blocking pattern), `research/researcher-03-model-acquisition.md` (HF repos,
hf-hub), `research/researcher-04-testing-strategy.md` (test/CI strategy).

## Decisions Locked (do not re-litigate)

1. ~~v1 ships CLI + HTTP API in the same crate/binary~~ **SUPERSEDED (2026-09-22, final user
   direction — overrides this original decision, do not re-ask):** CLI and HTTP server are **two
   fully separate binaries** — `apps/cli` (bin `openjev-cli`, one-shot benchmark run, no
   subcommands) and `apps/server` (bin `openjev-server`, axum HTTP server). Both depend on the same
   shared library crates (`engine`, `models`, `pipeline`, `timing`) — only the top-level
   entrypoint/packaging changed, not the underlying logic. See § Architecture.
2. All 3 original models supported from v1, fully in scope, none downgraded: Qwen3-0.6B, MiniCPM
   "2B", Qwen3(.5) "4B" — exact repo IDs pinned in Phase 0 (see Unresolved below).
3. Engine = `llama-cpp-2` crate (not candle) — matches original's llama.cpp-family runtime.
4. Two pipelines, timed independently, per original methodology: model-load, warmup, tokenize,
   constrained-readout, generation (§4 of `docs/research/openjev-rust-research.md`).
5. Constrained readout = logits restricted to candidate-label token ids BEFORE softmax (not
   full-vocab softmax then filter).
6. Generation = greedy, max_tokens=512, strip `<think>...</think>`, validate JSON schema against
   the option set. No "calibrated confidence" claim — probabilities are conditional on shown labels.
7. **CPU-only inference in v1.** No GPU build features (`cuda`/`vulkan`/`rocm`/`opencl`) enabled
   for `llama-cpp-2`/`llama-cpp-sys-2` — default (no GPU feature flags) build is the baseline per
   R1 §3. `n_threads` is configurable via CLI/HTTP, default tuned **near the available core count**
   (not a conservative low default) — see Phase 1/4/5. Dev/code machine is Windows; the real
   performance-tuning + e2e target is a **Linux server**: Intel Xeon Gold 5320, 32 vCPU, 62GB RAM
   (60GB free), 4GB swap unused, 295GB disk (276GB free). This hardware has ample RAM/disk for all
   3 models simultaneously (largest quantized GGUF is a few GB) — RAM/disk is never a reason to cut
   scope; the only real variable is CPU latency, measured empirically (Phase 0/1 sanity, Phase 7
   full tuning loop), never assumed. Linux build of `llama-cpp-2`/`llama-cpp-sys-2` is typically
   *simpler* than Windows (gcc/clang + cmake via apt, no MSVC/vcpkg) — not separately deep-researched
   up front; resolved live in Phase 7 only if a real issue surfaces there.
8. All 3 models stay **fully in-scope for v1 product behavior** on this hardware — the 4B model is
   NOT downgraded to optional/low-priority. The only accommodation is operational: its CI
   *integration test* may be `#[ignore]`-tagged by default if it proves slow (Phase 6) — the CLI/
   HTTP product surface treats it identically to the other two models.
9. **v1 compares 3 methods, not 2** (added 2026-09-22, final user direction): constrained-readout,
   JSON generation (both llama-cpp-2, as above), **and Laya single-pass encoder classification**
   (`mys/laya-GGUF`, ModernBERT-large, 421M params — a parallel project to Jev/SemIf, encoder-only,
   scores a decision in ONE forward pass, no autoregressive decoding). Laya's model card claims
   "calibrated probabilities" — directly contrasting with SemIf's own "NOT calibrated" disclaimer
   for constrained-readout (Decisions Locked #5) — this contrast is the point of including it.
   Laya's GGUF is compiled by `ggmlc` (github.com/monatis/ggmlc, MIT, C++17/CMake — same toolchain
   family as llama.cpp, not a new build risk category), which `llama-cpp-2` CANNOT read — integrated
   via shell-out to the `ggmlc-run` CLI binary (`std::process::Command`), not an FFI binding (v1
   scope: simplest correct integration, not a new native-binding surface). Default quant:
   **UD_Q4_K_M** (CPU-speed-first, consistent with the Q4_K_M default for MiniCPM/Qwen-4B).
   `mys/laya-multilingual-GGUF` (mmBERT-base, 322M) is explicitly deferred — documented as a known
   future extension point, not implemented in v1 (YAGNI: no stated need for non-English yet).
   A benchmark run returns all 3 results by default; `--skip-laya` opts out. **No separate `laya`
   crate** (simplified per direct user follow-up, 2026-09-22) — the `ggmlc-run` wrapper lives
   *inside* the `models` crate (registry + download + scoring, one crate), since download/checksum
   logic is already runtime-agnostic there; `pipeline::laya` calls into `models` directly, same as
   `pipeline::readout`/`generate` call into `engine`. See § Architecture and Phase 0's Unresolved
   #6-#8 (ggmlc-run CLI syntax, Windows build, CPU latency — all unverified, must be resolved with
   evidence, not assumed).

## Unresolved / Must Verify in Phase 0 (block all other phases)

1. **MiniCPM model name + HF repo id.** R3 found no `MiniCPM4-2B(-GGUF)` on HF; only
   `openbmb/MiniCPM5-2B-GGUF` and `openbmb/MiniCPM4-8B-GGUF`/`MiniCPM4-0.5B` exist. Site copy says
   "MiniCPM5 2B", GitHub repo description says "MiniCPM4 2B" — neither confirmed via primary
   source. **Action**: check openjev's own GitHub source (`workszop/openjev`, model config /
   worker.js) for the literal HF repo id it downloads from wllama; fall back to
   `openbmb/MiniCPM5-2B-GGUF` if the source is ambiguous/unavailable, and record the reasoning.
   **This fallback is final, not a pause point** — if Phase 0 cannot confirm the original's exact
   repo ID from source, proceed directly with the documented fallback and record the reasoning; do
   NOT block Phase 0 waiting for further user confirmation (confirmed via `/vk-plan-validate`
   interview, see § Validation Summary).
2. **Qwen "4B" model name + HF repo id.** Both `Qwen3-4B-GGUF` and `Qwen3.5-4B-GGUF` exist on HF
   (unsloth, bartowski, official Qwen org). **Action**: same as above — check openjev source for
   the literal id used; fall back to official `Qwen/Qwen3-4B-GGUF` (matches "Qwen3" family used for
   the 0.6B model) if source is ambiguous. **Same final-fallback rule as #1 above** — proceed on the
   documented fallback, do not pause for re-confirmation.
3. **`hf-hub` crate real API + version.** R3's two fetches disagreed: `HFClient`/`HFClientSync`
   builder API (v1.0.0-ish) vs older `Api`/`ApiBuilder`/`Repo::with_revision` — plus conflicting
   default cache path. **Action**: `cargo add hf-hub --dry-run` for the resolved version, then read
   `cargo doc --open` (or docs.rs at that pinned version) locally — do not code against either
   summary blind.
4. **`llama-cpp-2` logits + chat-template API.** R1 confirmed `LlamaBatch::add(token, pos, seq_ids,
   logits: bool)` and the generation-loop shape from `examples/simple`, but could NOT confirm
   `LlamaContext::get_logits`/`get_logits_ith` signatures, KV-cache reset methods
   (`kv_cache_clear`/`kv_cache_seq_rm`), or whether `llama_chat_apply_template` is wrapped.
   **Action**: `cargo doc --open` locally against the pinned crate version, or read
   `llama-cpp-2/src/context/llama_context.rs` + `src/model.rs` source directly. Fallback if no
   chat-template wrapper exists: hand-build ChatML prompt strings (Qwen3/MiniCPM both use
   `<|im_start|>role\n...<|im_end|>` framing) — acceptable per R1, do not block on this.
5. **Model licensing** (Apache-2.0 per R3, but per-specific-repo license field not read directly) —
   verify each pinned repo's HF license metadata in Phase 0/1 before distributing any
   download-manifest publicly; keep a NOTICE file if Apache-2.0 confirmed.
6. **`ggmlc-run` CLI syntax for typed-decision scoring** (Laya, Decisions Locked #9). Web research
   confirmed the binary exists (`ggmlc-run info model.gguf`, `ggmlc-run chat ...`, `ggmlc-run run
   model.gguf --image ...`) but did NOT confirm the exact subcommand/flags for the
   choice/score/noun-style typed-decision scoring the laya model card describes. **Action**: read
   `ggmlc-run --help` directly (after building it, see #7) or the laya model card's own usage
   snippet on HF — do not guess the input/output format.
7. **`ggmlc-run` Windows build.** Same toolchain family as `llama-cpp-2` (CMake 3.18+, C++17
   compiler) so not a new risk *category*, but not yet actually built/verified this session.
   **Action**: clone github.com/monatis/ggmlc, build on the Windows dev machine, confirm the
   `ggmlc-run` binary runs.
8. **Laya CPU latency — no public data exists.** Only GPU numbers are published (RTX 4050, CUDA
   graph: ~25ms/decision, ~143ms for a 7-question preset). **Action**: once #6/#7 are resolved, run
   one quick sanity timing (not full tuning — that's Phase 7) to get an order-of-magnitude CPU
   number; do not assume GPU-class speed on CPU.

Do not guess any of these 8 items — Phase 0 exists specifically to close them with primary-source
evidence (repo source read, `cargo doc`, `--help` output, or HF model card) before Phase 1 starts.

## Architecture (proposed — user-directed structure, final)

**Design history** (kept for context, not re-litigated): v1 started as a single Cargo package
(`src/engine/`, `src/models/`, etc.), was then split into a 5-crate workspace with CLI+server still
sharing one `cli` binary crate, and is now — per the user's final, explicit direction — a workspace
with **4 library crates + 2 fully separate binaries**. Each revision traded KISS-single-package
simplicity for clearer compilation/test/reuse boundaries and (in this final revision) fully
independent CLI/server deployability; the user's stated reasoning overrides the original R2 §1
single-package default each time — taste-level trade-off, not re-litigated further absent a new
explicit direction.

```
.claude/                          # existing, untouched by this plan
apps/
  cli/             (bin)          # package "cli", [[bin]] name = "openjev-cli" — one-shot benchmark run, NO subcommands
    Cargo.toml                    #   (server split out, so "bench" is this binary's only behavior)
    src/main.rs                   # thin: parse args, call run::run, map Result to exit code
    src/args.rs                   # #[derive(Parser)] flat args: --model, --prompt, --options, --threads, --batch-size, --format
    src/run.rs                    # orchestration: ensure_downloaded -> Engine::load -> warmup -> run_readout -> reset_context -> run_generate -> print
  server/          (bin)          # package "server", [[bin]] name = "openjev-server" — axum HTTP server
    Cargo.toml
    src/main.rs                   # thin: parse args (--port), build AppState/router, axum::serve
    src/args.rs                   # #[derive(Parser)] flat args: --port
    src/app.rs                    # AppState, router, bench_handler, BenchRequest/BenchReport DTOs, ApiError
crates/
  engine/          (lib)          # GGUF load via llama-cpp-2, LlamaContext wrapper — depends on `models` (path) + `timing` (path)
    Cargo.toml
    src/lib.rs
    src/error.rs
  models/          (lib)          # model registry (4 entries: 3 llama-cpp-2 + laya-en) + hf-hub download/cache/checksum
                                   #   + ggmlc-run CLI wrapper (shell-out) for Laya single-pass scoring, all in ONE crate
                                   #   (merged per direct user request — no separate `laya` crate) — zero intra-workspace deps
    Cargo.toml
    src/lib.rs
    src/download.rs
    src/laya.rs                   # LayaRunner/LayaConfig/score() — ggmlc-run shell-out lives here, not a separate crate
    src/error.rs
  pipeline/        (lib)          # readout.rs + generate.rs + laya.rs submodules, one crate — depends on `engine` (path) + `models` (path, for Laya scoring) + `timing` (path)
    Cargo.toml
    src/lib.rs
    src/readout.rs
    src/generate.rs
    src/laya.rs
    src/error.rs
  timing/          (lib)          # Timings struct only — lowest-level shared type, zero intra-workspace deps
    Cargo.toml
    src/lib.rs
docs/
  research/                       # existing (openjev-rust-research.md)
  benchmarks/                     # Phase 7 output (runbook + tuning results)
plans/                            # existing (this plan)
.gitignore                        # NEW — /target, .env.local, *.gguf
.env.local.example                 # NEW — template: HF_TOKEN= (optional; v1's 3 models are public/Apache-2.0,
                                   #   no token required per Phase 0 licensing check — documented convenience only)
rust-toolchain.toml                 # NEW — pins the exact Rust stable version Phase 0 resolves the build against
                                   #   (matters because native cmake/MSVC-on-Windows vs gcc-on-Linux builds are
                                   #   toolchain-version-sensitive)
Makefile                          # NEW — targets: build, test, bench (runs apps/cli), serve (runs apps/server),
                                   #   fmt, clippy, ci (test + fmt --check + clippy), server-deploy (Phase 7's
                                   #   SSH runbook steps)
Cargo.toml                        # workspace root: [workspace] members = ["apps/cli","apps/server",
                                   #   "crates/engine","crates/models","crates/pipeline","crates/timing"],
                                   #   resolver = "2", NO [package] (virtual manifest)
README.md                         # NEW — project description, build/run instructions for both binaries,
                                   #   links to docs/research + plans/
```

Crate/app responsibilities:
- `timing` (lib) — `Timings` struct (`Instant` deltas per phase: model_load/warmup/tokenize/
  constrained_readout/generation/**laya_model_load**/**laya_inference**), `#[derive(Serialize)]`.
  Zero intra-workspace deps — the lowest-level shared type, so `engine`, `models`, `pipeline`, and
  both binaries can all depend on it without any circular-dependency risk. Also the objective
  comparison instrument Phase 7 uses across tuning iterations.
- `models` (lib) — static registry of 4 entries: the 3 llama-cpp-2 models (`{name, hf_repo_id,
  revision_sha, filename, quant, expected_sha256, license}`) plus `laya-en` (`mys/laya-GGUF`,
  UD_Q4_K_M); download/cache/checksum via `hf-hub` (runtime-agnostic, serves all 4 entries); AND
  the `ggmlc-run` CLI wrapper (`std::process::Command` shell-out) for Laya's single-pass scoring —
  **one crate, not two** (merged per direct user follow-up, 2026-09-22): download/registry and
  Laya-scoring share the same "resolve a model spec, act on it" shape, and Laya's own download path
  already lives here regardless. `mys/laya-multilingual-GGUF` is a documented, deliberately-deferred
  future entry (YAGNI — no v1 non-English requirement), not implemented. Zero intra-workspace deps.
- `engine` (lib) — load GGUF via `llama-cpp-2`, own `LlamaBackend`/`LlamaModel`/`LlamaContext`,
  `!Send`/`!Sync` context wrapped for cross-thread use by callers (R2 §2). Depends on `models`
  (receives an already-resolved model spec/path) and `timing`. **CPU-only build**: no
  `cuda`/`vulkan`/`rocm`/`opencl` feature flags on `llama-cpp-2`/`llama-cpp-sys-2`; exposes
  configurable `n_threads` (default tuned near available core count, overridable via CLI/HTTP) and
  configurable batch size, both left tunable for Phase 7's real-server optimization loop. Also owns
  greedy-token sampling (`sample_greedy`) and a KV-cache reset call (`reset_context`, used between
  pipeline runs so neither contaminates the other's measured state) — both live here because
  `LlamaContext` is private to this crate.
- `pipeline` (lib, submodules `readout`, `generate`, `laya`) — constrained single-token logit
  readout (restricted softmax), JSON generation (strip `<think>`, validate schema), and Laya
  single-pass scoring, respectively. `readout`/`generate` call into `engine`; `laya` calls into
  `models`'s Laya-scoring function directly (no separate crate to depend on). Depends on `engine` +
  `models` + `timing`.
- `apps/cli` (bin, package `cli`, binary `openjev-cli`) — one-shot benchmark run: parses flat clap
  args (no subcommands — the only behavior left in this binary once `serve` moved to its own app;
  includes `--skip-laya` to opt out, default runs all 3 methods), orchestrates all 3 pipeline
  methods against one model+question, prints `Timings` + all 3 results as JSON/text. Depends on
  `engine`, `models`, `pipeline`, `timing`.
- `apps/server` (bin, package `server`, binary `openjev-server`) — axum HTTP server exposing the
  same 3-way benchmark as `POST /bench` (+ `GET /health`, + `skip_laya` request field, default
  false). `tokio::task::spawn_blocking` around engine calls, `Arc<Mutex<AppState>>` singleton
  serializing inference (R2 §2 — benchmark runs are inherently sequential, Mutex+spawn_blocking
  simplest correct fit). Depends on `engine`, `models`, `pipeline`, `timing`.

## Phases

| # | Phase | Depends on | Must verify first |
|---|---|---|---|
| 0 | Spike: model IDs, hf-hub API, llama-cpp-2 API, `ggmlc-run` CLI/build, workspace skeleton (Windows build) | — | blocks 1-6 |
| 1 | `engine`/`models`/`timing` crates (load GGUF, hf-hub download, cache, checksum, CPU-only + n_threads; `models` also wraps `ggmlc-run` shell-out for Laya) | 0 | — |
| 2 | `pipeline` crate: constrained readout + Laya scoring | 1 | — |
| 3 | `pipeline` crate: JSON generation | 1 | — |
| 4 | `apps/cli` (`openjev-cli` binary) | 2, 3 | — |
| 5 | `apps/server` (`openjev-server` binary, axum) | 2, 3 | — |
| 6 | Test suite completion + CI outline | 1-5 (incremental; final pass after 5) | — |
| 7 | E2E + performance tuning on real Linux server (SSH), incl. Laya | 1-5 | — (see § Validation Summary: no per-command gate required for this confirmed dev server) |

Dependency shape: Phase 0 gates everything. Phase 1 gates 2 and 3 (both pipelines need the
`engine`/`models` crates). 2 and 3 can run in parallel (independent files within `pipeline`, one
small shared-file touchpoint — see Parallel Execution Matrix) and both gate 4 and 5 (both binaries
wrap all 3 pipeline methods via the `pipeline` crate). 4 and 5 can run in parallel — **fully
disjoint now**: `apps/cli` and `apps/server` are separate packages with no shared files at all (a
genuine simplification from the earlier single-binary design, where `cli`'s `Command` enum and
`main.rs` dispatch were shared touchpoints). Phase 6 unit/property tests can be written
incrementally alongside 1-5 per-crate; the consolidated suite + CI workflow pass happens last.
Phase 7 needs both binaries (`openjev-cli` + `openjev-server`) built, so it depends on 1-5; it runs
independently of Phase 6 (disjoint files — `docs/benchmarks/` vs `crates/*/tests/`+`.github/`) and
can proceed in parallel with it.

## Parallel Execution Matrix

| Phase | Owned files/globs | Depends on |
|---|---|---|
| 0 | `Cargo.toml` (workspace root), `crates/*/Cargo.toml` (4), `apps/*/Cargo.toml` (2), `.gitignore`, `.env.local.example`, `rust-toolchain.toml`, `Makefile`, `README.md`, spike notes | — |
| 1 | `crates/timing/**`, `crates/models/**` (incl. `crates/models/src/laya.rs`), `crates/engine/**` | 0 |
| 2 | `crates/pipeline/src/readout.rs`, `crates/pipeline/src/laya.rs`, `crates/pipeline/src/lib.rs`\*, `crates/pipeline/src/error.rs`\*, `crates/pipeline/Cargo.toml` | 1 |
| 3 | `crates/pipeline/src/generate.rs`, `crates/pipeline/src/lib.rs`\*, `crates/pipeline/src/error.rs`\* | 1 |
| 4 | `apps/cli/**` — fully owned, no shared files with Phase 5 | 2, 3 |
| 5 | `apps/server/**` — fully owned, no shared files with Phase 4 | 2, 3 |
| 6 | `crates/*/tests/**`, `apps/*/tests/**`, `.github/workflows/**`, `Makefile`'s `ci` target | 1-5 |
| 7 | `docs/benchmarks/**`, `Makefile`'s `server-deploy` target | 1-5 |

`*` = shared, additive-only file (documented below) — NOT a disjoint-ownership claim; each phase
adds its own non-overlapping lines to it. **Only Phase 2/3 have a shared-file touchpoint now** —
Phase 4/5's earlier shared-file note (a `Command` enum + `main.rs` dispatch shared between CLI and
server subcommands) no longer applies: `apps/cli` and `apps/server` are fully separate packages, a
real simplification from the previous single-binary design. Note Phase 2 now owns TWO pipeline
submodule files (`readout.rs` + `laya.rs`, both single-forward-pass scoring methods, folded into
the same phase for cohesion — see `phase-02-pipeline-constrained-readout.md`).

**Shared integration files (documented, additive-only edits):**
- `crates/pipeline/src/lib.rs` and `crates/pipeline/src/error.rs`: touched by both Phase 2 and
  Phase 3. `lib.rs`: each phase appends its own `pub mod` line(s) (Phase 2: `pub mod readout;` +
  `pub mod laya;`; Phase 3: `pub mod generate;`). `error.rs`: each phase adds its own
  `PipelineError` variants (no shared variant renamed/removed by either). Whichever phase lands
  first creates the file with its own addition(s); the other appends. If dispatched to two
  parallel agents, the second to merge resolves this in a trivial, non-semantic conflict.

Waves: {0} → {1} → {2, 3 in parallel} → {4, 5 in parallel} → {6, 7 in parallel}. Phases 2/3 have one
small shared-file, additive-only touchpoint (above); everything else in their owned globs is
disjoint. Phases 4/5 are now fully disjoint (no shared files at all). Phases 6/7 touch fully
disjoint files (`crates/*/tests/`+`.github/` vs `docs/benchmarks/`). Safe to dispatch each wave's
phases concurrently given the additive-merge convention for Phase 2/3's one shared file.

## Phase Files

- `phase-00-spike-verification.md`
- `phase-01-engine-model-management.md`
- `phase-02-pipeline-constrained-readout.md`
- `phase-03-pipeline-generation.md`
- `phase-04-cli-bench.md`
- `phase-05-http-api-serve.md`
- `phase-06-test-suite-ci.md`
- `phase-07-e2e-server-tuning.md`

## Automatic Validation

**Rounds run:** 1 deep (4 parallel `code-reviewer` spawns, one per lens) + 1 solo delta
verification (see note) + 2 solo self-checks after subsequent architecture revisions (workspace
crate split, then apps/+crates/ final layout — see notes below). **Outcome:** ESCALATED-then-resolved.

### Round Log
| Round | Type | Structural | Consistency | Fidelity | Testability | BLOCKERs | Clean |
|---|---|---|---|---|---|---|---|
| 1 | deep | 2 BLOCKER / 4 MINOR | 4 BLOCKER / 1 MINOR | 1 MINOR | 1 BLOCKER / 2 MINOR | 7 | no |
| 2 | delta (solo, see note) | fixed, re-verified | fixed, re-verified | fixed, re-verified | fixed, re-verified | 0 | yes |

**Note on round 2 process:** round 1's four `code-reviewer` subagents completed
(`reports/validation-round-1-{structural,consistency,fidelity,testability}.md`) but their
completion notifications were delayed ~50+ minutes past when work continued on unrelated plan
updates (CPU-only/hardware/Phase-7 additions requested mid-flight); the coordinator instructed not
to wait further and to complete a validation pass by direct means once findings were available.
Round 1's 4 reports were read directly once located; every BLOCKER + relevant MINOR was fixed in
one pass (with hygiene greps confirming no stale references from the renames — moving `Timings` to
Phase 1, adding `Engine::sample_greedy`/`reset_context` to Phase 1, correcting the Parallel
Execution Matrix's false "no overlap" claims, rewriting Phase 0's Quality Gate section to satisfy
§6 directly, naming Phase 1's e2e scenario, G/W/T-izing Phase 0/6 ACs). Round 2 was performed as a
direct self-verification against round 1's exact findings (not a fresh `code-reviewer` spawn) — a
deviation from the canonical loop's role-isolation requirement (§3), made explicitly to avoid
re-risking a second multi-agent hang under time pressure. Real findings (round 1's 7 BLOCKERs) came
from genuinely independent review; only their re-verification after fixing was done solo.

**Note on post-round-2 architecture revisions:** after round 2's clean state, the user directed two
further architecture changes in sequence — (a) split the single package into a 5-crate workspace
(`timing`/`models`/`engine`/`pipeline`/`cli`, with `cli` still hosting both `bench`/`serve`
subcommands), then (b) superseded that with the final layout: `apps/cli` + `apps/server` as fully
separate binaries plus 4 shared library crates, dropping the CLI/server shared-file touchpoint
entirely and adding root-level project files (`.gitignore`, `.env.local.example`,
`rust-toolchain.toml`, `Makefile`, `README.md`). Both revisions were re-verified solo (grep-based
hygiene pass: no stale `src/engine`/`src/cli`/`src/server` paths, no stale intermediate
`crates/cli`-as-bin-crate-with-server-inside paths, cross-phase references consistent, phase
dependency order unchanged — only file paths and the CLI/server packaging changed) rather than via
fresh 4-lens spawns, for the same reason as round 2 (avoiding repeated multi-agent dispatch under
active time pressure while the architecture was still being iterated live).

**Note on the Laya addition (3rd revision):** the user then directed adding a 3rd comparison
method, Laya (single-pass encoder scoring via `ggmlc-run` shell-out), initially specified as a
separate `crates/laya` crate, then simplified per direct follow-up ("gộp laya vào models luôn đi")
to live inside `crates/models` instead (`crates/models/src/laya.rs`) — no separate crate. This
touched all 9 plan files: `plan.md` (Decisions Locked #9, Architecture tree/responsibilities,
Phases table, Parallel Execution Matrix), `phase-00` (3 new Unresolved items — ggmlc-run CLI
syntax/Windows build/CPU sanity — + workspace skeleton scope), `phase-01` (`models::laya`
implementation, `Timings`' 2 new Laya fields), `phase-02` (renamed to include Laya, new
`pipeline::laya` submodule, folded in as a 2nd single-forward-pass method alongside readout),
`phase-03` (unaffected, verified via grep), `phase-04`/`phase-05` (3-way `BenchReport`,
`--skip-laya`/`skip_laya` flag, isolated-failure handling so a Laya issue never aborts the whole
run), `phase-06` (also caught and fixed pre-existing stale single-crate `src/` paths that had been
missed in the earlier apps/+crates/ restructuring pass, plus added Laya CI provisioning), `phase-07`
(also fixed a previously-missed RAM figure drop and workspace-path staleness, added mandatory Laya
benchmarking, and actually applied the SSH-gate removal that an earlier note had prematurely marked
"Applied" without the corresponding edit — corrected here). Self-check performed via grep sweep
across all 9 files (not a fresh 4-lens spawn, same time-pressure rationale as prior revisions):
confirmed no stale `src/engine|cli|server|pipeline|models` single-crate paths outside intentional
historical-note context, no stale separate `crates/laya` references, consistent `LayaResult`/
`LayaRunner`/`LayaError`/`skip_laya` naming across all files that reference them, consistent
workspace member lists (6 crates: `apps/cli`, `apps/server`, `crates/engine`, `crates/models`,
`crates/pipeline`, `crates/timing`), unchanged phase dependency order (0→1→{2,3}→{4,5}→{6,7}), and
matching Phase Files list vs files on disk.

Full round-1 artifacts: `reports/validation-round-1-structural.md`,
`reports/validation-round-1-consistency.md`, `reports/validation-round-1-fidelity.md`,
`reports/validation-round-1-testability.md`.

## Validation Summary

**Validated:** 2026-09-22
**Questions asked:** 4 (`/vk-plan-validate` interview, single round)

### Confirmed Decisions
- **Model-ID fallback (Unresolved #1/#2):** if Phase 0 cannot confirm the original site's exact HF
  repo IDs from source, proceed with the plan's documented fallbacks
  (`openbmb/MiniCPM5-2B-GGUF`, `Qwen/Qwen3-4B-GGUF`) and record the reasoning — do NOT block Phase 0
  waiting for further user confirmation. *(Applied — see Unresolved #1/#2 above.)*
- **Quantization default for the 2B/4B models:** **Q4_K_M** (CPU-speed-first), not Q8_0. The
  0.6B model's Q8_0 (confirmed from the original site) is unaffected — this only sets the default
  for the two larger models, which the original site's exact quant level was never confirmed for
  anyway. Registry stays user-configurable; Q4_K_M is just the shipped default. *(Applied in
  `phase-00-spike-verification.md` and `phase-01-engine-model-management.md`, `crates/models/**`.)*
- **Test rigor (Phase 6):** keep as planned — ≥90% line / ≥75% branch coverage on new code, full
  test pyramid, mandatory e2e scenarios per `quality-gate.md` §6. Not relaxed.
- **Phase 7 SSH execution:** confirmed dev/test server, user's own infra — **no per-session or
  per-command confirmation gate required**. This *relaxes* the plan's earlier "user SSH confirmation
  at execution time" note (added defensively before this interview) — proceed directly with SSH
  deploy/build/benchmark/tune commands within Phase 7's documented scope when that phase executes.
  *(Applied — see `phase-07-e2e-server-tuning.md`.)*

### Action Items — status: APPLIED
- [x] `plan.md` Decisions Locked §8 / Unresolved #1-#2: fallback-is-final-not-a-pause clarification
  appended (see Unresolved #1/#2 above).
- [x] `phase-00-spike-verification.md` and `phase-01-engine-model-management.md`: **Q4_K_M** set as
  the default quant for the MiniCPM-2B and Qwen-4B registry entries (0.6B stays Q8_0, unchanged),
  at their new `crates/models/**` location.
- [x] `phase-07-e2e-server-tuning.md`: the "user SSH confirmation at execution time" gate removed
  from Overview/Success-Criteria wording — pre-approved for this specific dev server; normal safety
  judgment retained (no destructive/irreversible ops without flagging).

### Recommendation
Proceed to implementation. No blocking issues raised in this interview — all 4 answers either
confirmed the plan as-is or slightly loosened a constraint (quant default, SSH gate). All 3 action
items applied during the subsequent architecture-revision editing pass (this was "the next plan
touch" the recommendation anticipated).
