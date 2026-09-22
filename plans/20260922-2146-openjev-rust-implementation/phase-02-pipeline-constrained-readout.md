---
phase: 2
name: Pipeline - Constrained Readout + Laya Scoring
status: pending
depends_on: [1]
---

## Context Links
- `../plan.md` § Decisions Locked (#5, #9), § Architecture (`pipeline` crate)
- `docs/research/openjev-rust-research.md` §1 (definition), §4 (what to preserve)
- `phase-01-engine-model-management.md` (`Engine::decode_prompt`, `Engine::tokenize`,
  `models::laya::LayaRunner::score`, `timing` crate)

## Overview
- Priority: P0, parallel with Phase 3 (both write to the `pipeline` crate, one small shared-file
  touchpoint — see plan.md Parallel Execution Matrix — both depend only on Phase 1).
- Status: pending.
- Implement TWO single-forward-pass scoring methods in the `pipeline` crate, folded into this one
  phase for cohesion (both are "one pass, no autoregressive decoding" methods, unlike Phase 3's
  generation loop): (a) the "direct readout" path — restrict logits to the candidate option
  labels' token ids, softmax over just that subset, return a probability per option; (b) Laya
  single-pass encoder scoring — call `models::laya::LayaRunner::score` and wrap its result into the
  pipeline's result shape.

## Key Insights
- Original semantics (research §1): normalize ONLY over the option labels actually shown for that
  question (2-20 choices), not the full vocabulary — this is a conditional probability, not a
  calibrated confidence. Must carry that disclaimer through to any output struct/doc comment.
  **Laya's own model card claims calibrated probabilities** (plan.md Decisions Locked #9) — do NOT
  carry the same "not calibrated" disclaimer onto `LayaResult`; the contrast between the two
  disclaimers is the point of running both.
- `llama-cpp-2` gives a raw full-vocab `&[f32]` logits slice (`engine::Engine::decode_prompt`);
  there is no crate-provided "restricted softmax over N ids" helper (R1 §1) — this crate owns that
  math.
- Softmax numerical stability: subtract max logit before exponentiating (standard technique) to
  avoid overflow on large logits.
- Option labels are the actual answer tokens (e.g. "A", "B", "C", ... or single-letter/short-string
  choices per the question) — each must tokenize to exactly one token id for a true single-token
  readout; if a candidate label tokenizes to >1 token, that's a caller/data error to surface, not
  silently mishandle (e.g. by only taking the first sub-token). Laya has no equivalent constraint —
  `ggmlc-run` handles its own tokenization internally (opaque to this crate).
- `crates/pipeline/Cargo.toml` depends on `crates/engine` (path) + `crates/models` (path — for
  `models::laya::LayaRunner`) + `crates/timing` (path) — this phase's `Cargo.toml` edit is the
  first time those path deps get declared (Phase 0 scaffolded an empty `pipeline` crate with no
  deps yet).
- `pipeline::laya` is a thin wrapper, NOT a reimplementation — all the real `ggmlc-run`
  shell-out/parsing logic lives in `models::laya` (Phase 1); this submodule's job is only to fit
  that result into the pipeline's timing/error conventions (`&mut Timings`, `PipelineError`), same
  shape as `readout`/`generate`.

## Requirements
### Functional
**Readout** (llama-cpp-2 path):
- Given a prompt + a list of candidate option strings (labels), tokenize each candidate to a single
  token id (error if any candidate isn't exactly 1 token for the loaded model's vocab).
- Decode the prompt once via `Engine::decode_prompt`, get full-vocab logits at the last position.
- Restrict to the candidate token ids' logits, apply numerically-stable softmax over just that
  subset, return `Vec<(String /* label */, f32 /* probability */)>` summing to 1.0 (within
  epsilon).
- Time this pipeline's tokenize + decode + softmax steps separately, recording into the
  `timing::Timings` struct (its own crate, available here since `pipeline` depends on `timing`) via
  a `&mut Timings` parameter (`timings.constrained_readout`) — the `apps/cli`/`apps/server`
  orchestration layer owns *assembling and printing* the final report, this pipeline only
  *populates its own fields*.

**Laya** (ggmlc-run path, via `models::laya`):
- Given a prompt + candidate options, resolve/load the Laya-en `LayaRunner` (via
  `models::laya::LayaRunner::load_from_spec`, using the Laya-en `ModelSpec` from `models`' registry)
  and call `score(prompt, options)`.
- Wrap the returned label/score pairs into `LayaResult { scores: Vec<(String, f32)> }`.
- Time the model-load (first call only, cacheable by caller) and inference steps separately into
  `timings.laya_model_load`/`timings.laya_inference` (both `Option<Duration>`, `Some` only when
  this path actually runs).
- Respect `--skip-laya` (a caller-level flag, not enforced inside this crate — `apps/cli`/
  `apps/server` simply don't call the laya submodule when skipped; this submodule has no "skip"
  concept of its own, keeping it simple).

### Non-functional
- Pure softmax/restriction math (readout) has zero dependency on `llama-cpp-2` types beyond
  `&[f32]` in and `Vec<(String, f32)>` out — keep it independently unit-testable without a loaded
  model (feed it a synthetic logits vector + fake candidate-id map).
- `pipeline::laya`'s orchestration wrapper never panics on a `LayaError` from `models::laya` —
  every variant maps to a `PipelineError` variant, structured, not swallowed.

## Architecture
```
crates/pipeline/src/readout.rs
  pub struct ReadoutResult { pub probabilities: Vec<(String, f32)> }  // sums to 1.0, conditional on shown labels — NOT calibrated
  pub fn constrained_softmax(logits: &[f32], candidate_ids: &[(String, i32)]) -> Vec<(String, f32)>  // pure fn, unit-testable
  pub fn run_readout(engine: &mut engine::Engine, prompt: &str, options: &[String], timings: &mut timing::Timings) -> Result<ReadoutResult, PipelineError>

crates/pipeline/src/laya.rs
  pub struct LayaResult { pub scores: Vec<(String, f32)> }  // Laya's own claim: calibrated (contrast w/ ReadoutResult's disclaimer)
  pub fn run_laya(runner: &models::laya::LayaRunner, prompt: &str, options: &[String], timings: &mut timing::Timings) -> Result<LayaResult, PipelineError>
    // thin: calls runner.score(prompt, options), records timings.laya_inference, maps LayaError -> PipelineError
```
`constrained_softmax` is the pure-math core (candidate token ids -> restricted logits -> softmax);
`run_readout` is the orchestration wrapper (tokenize options, call engine, call
`constrained_softmax`, record timing). `run_laya` is intentionally thin — no math of its own, just
timing + error-shape adaptation around `models::laya::LayaRunner::score`.

## Related Code Files
- CREATE `crates/pipeline/src/lib.rs` — crate root with `pub mod readout;` + `pub mod laya;`
  (shared file, see plan.md Parallel Execution Matrix's "Shared integration files" note — Phase 3
  appends `pub mod generate;` alongside these lines, additive-only, never reorders/removes them).
- CREATE `crates/pipeline/src/readout.rs` — `constrained_softmax`, `run_readout`, `ReadoutResult`.
- CREATE `crates/pipeline/src/laya.rs` — `run_laya`, `LayaResult`.
- CREATE `crates/pipeline/src/error.rs` — `PipelineError` variants for this phase's two
  submodules: candidate label not single-token, engine decode failure, empty candidate list
  (readout); binary-not-found/spawn/parse failure mapped from `LayaError` (laya). Shared file with
  Phase 3, same additive-only convention: Phase 3 adds its own generation-specific variants, never
  touches these.
- MODIFY `crates/pipeline/Cargo.toml` — add path deps `engine = { path = "../engine" }`, `models =
  { path = "../models" }`, `timing = { path = "../timing" }` (first real deps declared on this
  crate, scaffolded empty by Phase 0).

## Implementation Steps
1. Write `constrained_softmax(logits: &[f32], candidate_ids: &[(String, i32)]) -> Vec<(String, f32)>`
   as a pure function: gather `logits[id]` for each candidate, subtract max for stability,
   exponentiate, sum, divide — return label/probability pairs in the same order as input.
2. Add explicit handling for edge cases (per R4 §3 property-test guidance): empty candidate list ->
   `Err` (not a panic, not a silent empty `Vec`); single candidate -> probability exactly 1.0;
   all-equal logits -> uniform distribution.
3. Write `run_readout`: for each option string, call `Engine::tokenize`, assert exactly 1 resulting
   token (else `PipelineError::MultiTokenLabel`); call `Engine::decode_prompt` once for the shared
   prompt; call `constrained_softmax` with the gathered candidate ids; record `Instant` deltas for
   tokenize-candidates and decode+softmax into the caller-supplied `&mut Timings`
   (`timings.constrained_readout = ...`). Callers are responsible for calling `Engine::
   reset_context` before this if a prior pipeline run shares the same `Engine` instance (Phase
   4/5's orchestration does this — see those phases).
4. Document (doc comment on `ReadoutResult`) the "conditional on shown labels, not calibrated
   confidence" caveat from the original tool's semantics — carries the product's disclaimer into
   the API surface so callers (`apps/cli`/`apps/server` output) don't misrepresent it.
5. Write `run_laya(runner, prompt, options, timings)`: call `runner.score(prompt, options)`, time
   the call into `timings.laya_inference` (the caller times `laya_model_load` separately around
   `LayaRunner::load_from_spec`, since that's a one-time cost this function doesn't control), map
   `Err(LayaError)` to the corresponding `PipelineError` variant, wrap `Ok` into
   `LayaResult { scores }`.
6. Document (doc comment on `LayaResult`) that Laya's own model card claims calibrated
   probabilities — explicitly note the contrast with `ReadoutResult`'s disclaimer, so callers
   surface both claims accurately rather than treating the two results as equivalent.

## Todo List
- [ ] `constrained_softmax` pure function implemented + edge cases handled
- [ ] `run_readout` orchestration implemented, timing recorded
- [ ] `run_laya` orchestration implemented, timing recorded, `LayaError` mapped to `PipelineError`
- [ ] `PipelineError` covers multi-token-label, empty-candidates, and Laya failure cases
- [ ] `crates/pipeline/Cargo.toml` declares `engine`/`models`/`timing` path deps
- [ ] Doc comments carry the "not calibrated" (readout) vs "claims calibrated" (Laya) contrast

## Success Criteria
- `constrained_softmax` output always sums to 1.0 within 1e-5 for any non-empty finite input.
- Given a real loaded model (Qwen3-0.6B) and a 2-4 option question, `run_readout` returns
  probabilities matching expected relative ordering for an obviously-biased prompt (manual sanity
  check, e.g. "The capital of France is: A) London B) Paris" should favor B strongly).
- Given a real `LayaRunner` and the same 2-4 option question, `run_laya` returns non-empty scores
  or a structured `Err` (never panics).
- CPU latency reference (informational only, not a hard threshold): record wall-clock time for
  `run_readout` and `run_laya` on the dev machine as a sanity baseline. This environment is
  CPU-only — do not compare against the original browser tool's GPU/WebGPU numbers (docs/research
  §1's "~1.02s on RTX 3090" figure, or Laya's own RTX 4050 figures, are different hardware classes
  entirely). Real target-hardware latency (32-vCPU Linux server) is measured and tuned in Phase 7.

## Test Strategy & Quality Gate
Lane: normal.
- Spec: Goal = compute a normalized probability distribution over a small candidate-label subset
  from one forward pass's logits (readout), AND wrap a Laya single-pass score into the pipeline's
  result/timing shape (laya). AC: (1) Given a logits vector and N candidate ids, When
  `constrained_softmax` runs, Then output has N entries summing to 1.0 ± 1e-5, all in [0,1]. (2)
  Given an empty candidate list, When called, Then returns `Err`, never panics/NaN. (3) Given a
  candidate label that tokenizes to 2+ tokens, When `run_readout` runs, Then returns
  `Err(MultiTokenLabel)`. (4) Given a `LayaRunner` that returns `Err(LayaError::BinaryNotFound)`,
  When `run_laya` runs, Then it returns the corresponding `PipelineError` variant, never panics.
  I/O contract: see Architecture section signatures. Out-of-scope: generation path (Phase 3),
  CLI/HTTP surfacing (Phase 4/5), `models::laya`'s own subprocess logic (Phase 1).
- Pyramid ~80/15/5 for `readout` (mostly pure math + thin orchestration, unit-heavy is correct):
  unit tests for `constrained_softmax` edge cases (empty/single/uniform/near-one-hot, per R4 §3)
  and property tests (proptest) asserting sum≈1.0, all∈[0,1], no NaN/Inf across random `f32` logit
  vectors of varying size; 1 integration test (feature-gated, real Qwen3-0.6B) as the e2e scenario
  — "given a real biased prompt, the favored option's probability is highest." For `laya`
  (thin wrapper, less pure math to test): unit tests for `LayaError`→`PipelineError` mapping using
  a fake/mock `score()` result (no real subprocess needed for this mapping logic); 1 integration
  test (feature-gated, real `ggmlc-run` + Laya-en) as its own e2e scenario — "given a real prompt,
  `run_laya` returns non-empty scores."
- Coverage target: ≥90% line / ≥75% branch on `crates/pipeline/src/readout.rs` and
  `crates/pipeline/src/laya.rs`.
- Evidence commands: `cargo test -p pipeline readout:: laya::` (unit + proptest, no native
  model/subprocess needed for the mapping/math logic), `cargo test -p pipeline --features
  integration readout:: laya::` (the two e2e scenarios), diff-coverage tool report for both files.

## Risk Assessment
- Risk: a candidate option label doesn't tokenize to exactly 1 token for some model/vocab
  combination (e.g. multi-character labels on certain tokenizers) — surfaced as `Err`, not silently
  degraded; if this proves common across real question sets, revisit (open question, not a silent
  scope change) whether multi-token labels need a different readout strategy (e.g. sum/product of
  per-token logits) — flag to user if encountered, do not decide unilaterally mid-implementation.
- Risk: Laya's `ggmlc-run` output format (confirmed in Phase 0) turns out to need more parsing
  robustness than `models::laya` provides — this phase's `run_laya` cannot fix that itself (parsing
  lives in Phase 1); if a real-world parse failure surfaces during this phase's integration test,
  file it back against `models::laya`, don't work around it here.

## Security Considerations
- None beyond Phase 1's model-integrity controls (already covers logits provenance and the
  `ggmlc-run` argv-based invocation with no shell-injection surface).

## Next Steps
- Phase 4 (`apps/cli`) and Phase 5 (`apps/server`) both call `run_readout` and `run_laya` and
  surface `ReadoutResult`/`LayaResult` + `Timings` in their respective output formats, alongside
  Phase 3's `GenerateResult`, for the 3-way comparison.
