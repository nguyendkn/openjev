---
phase: 6
name: Test Suite Completion + CI Outline
status: pending
depends_on: [1, 2, 3, 4, 5]
---

## Context Links
- `research/researcher-04-testing-strategy.md` (full scope: split strategy, mocking, proptest, CI)
- All prior phase files' "Test Strategy & Quality Gate" sections (this phase consolidates, does not
  replace them — each phase already wrote its own tests incrementally; this phase closes gaps +
  builds CI)
- `../plan.md` § Architecture (workspace crate layout), § Decisions Locked #9 (Laya)

## Overview
- Priority: P1, runs last (depends on all implementation phases existing) but individual test files
  should be written incrementally alongside Phases 1-5 per their own Quality Gate sections, not
  batched entirely to the end — this phase's job is: fill any gaps, add the `InferenceEngine` mock
  trait for fast unit tests, add proptest coverage where missing, and write the CI workflow
  (including `ggmlc-run` provisioning for Laya's integration tests).
- Status: pending.

## Key Insights
- Feature-gate (`#[cfg(feature = "integration")]`), not `#[ignore]`, for anything needing the real
  native `llama-cpp-2` link or a real `ggmlc-run` invocation — avoids paying native-build/subprocess
  cost on every default `cargo test` (R4 §1). This has already been the pattern used in Phases 1-5's
  own test sections; this phase enforces it project-wide, per-crate, and wires the Cargo feature
  consistently on `engine`, `models` (covers Laya too), `pipeline`, `cli`, `server` — the 5 crates
  R4 §1's split logic applies to (`timing` has no integration-gated tests, it's a plain struct).
- `mockall` is the de facto standard for trait mocking (R4 §2, 100M+ downloads) — introduce an
  `InferenceEngine` trait (wrapping the subset of `engine::Engine` that Phase 2/3's `readout`/
  `generate` submodules actually call: tokenize, decode_prompt, decode_next/sample) so pipeline
  unit tests can run against `MockInferenceEngine` instead of a real loaded model, decoupling
  pipeline-logic tests from native build cost entirely (R4 §2's recommended pattern, not yet
  introduced in Phase 2/3's own sections which assumed real-model integration tests for their e2e
  case — this phase adds the mock-based fast path as a complement, not a replacement). The same
  mockable-trait question applies to `models::laya::LayaRunner` for `pipeline::laya`'s unit tests —
  decide both at implementation time per the same "worth it?" criterion (see Architecture).
- proptest for softmax (sum≈1, ∈[0,1], no NaN/Inf, monotonicity) and think-strip/JSON-validate
  (never panics) — largely specified per-phase already (Phase 2/3); this phase verifies both are
  actually present and adds any missing property.
- CI: default job = `cargo test --workspace` (no `integration` feature) + `cargo clippy --workspace
  -- -D warnings` + `cargo fmt --all --check`, fast, no native link if `llama-cpp-2`/`ggmlc-run`
  usage is properly feature-gated behind `integration` (R4 §4) — note: `crates/engine` and
  `crates/models` themselves depend on `llama-cpp-2`/`hf-hub` unconditionally per Phase 1's design
  (core dependencies, not optional), so the "no native link" framing from R4 §4 only fully applies
  to which TESTS run, not whether the crates compile — `llama-cpp-2`'s native build still happens on
  every `cargo build --workspace`/`cargo test --workspace`. Use `sccache` (R4 §4,
  Mozilla-Actions/sccache-action) to keep that native compile fast across CI runs regardless.
  `ggmlc-run` is a SEPARATE binary (not a Rust crate dependency) — the default CI job never needs to
  build it at all, since Laya's integration tests (which are the only tests that invoke it) are
  feature-gated off by default.
- Separate CI workflow for the full 4-model matrix (3 LLMs + Laya; nightly/manual/scheduled, not
  per-PR) given GitHub's 10GB `actions/cache` cap and multi-GB model sizes (R4 §4) — cache only the
  smallest model (Qwen3-0.6B) for the default integration job; reserve MiniCPM-2B/Qwen-4B/Laya-en
  for the scheduled full-matrix workflow. The full-matrix job also needs to clone+build
  `github.com/monatis/ggmlc` (same CMake/C++17 toolchain already needed for `llama-cpp-2`) before
  Laya's integration test can run — this is CI-workflow scope, not a new Rust-level risk.
- All 4 models remain fully in-scope for the product (plan.md Decisions Locked #8, #9) — the 4B
  model's and Laya's *CI integration tests only* may be `#[ignore]`-tagged by default (run via
  `--include-ignored` in the scheduled/manual full-matrix job) since they are the slowest/newest to
  set up on a typical CI runner. This is a CI-speed accommodation, not a product scope cut. CI
  runner CPU specs are NOT the same as the 32-vCPU Linux target server (plan.md Decisions Locked
  #7) — do not assume CI timing reflects target-hardware performance; that's Phase 7's job on the
  real hardware.

## Requirements
### Functional
- `InferenceEngine` trait (or narrower, per-pipeline traits if that's a cleaner fit given Phase
  1/2/3's actual call shape) + `MockInferenceEngine` via `#[automock]`, used to add fast
  mock-backed unit tests for `run_readout`/`run_generate`'s orchestration logic (currently only
  covered by real-model integration tests per Phase 2/3's own sections). Same consideration for a
  mockable `LayaRunner` trait to unit-test `pipeline::laya::run_laya`'s error-mapping without a
  real `ggmlc-run` call.
- Confirm/complete proptest coverage: softmax properties (Phase 2), strip+parse never-panics
  property (Phase 3) — add if either phase's implementation shipped without them.
- Each testable crate's `Cargo.toml` (`engine`, `models`, `pipeline`, `cli`, `server`) declares an
  `integration` feature flag, wired correctly (gates test compilation, per R4 §1 — not just
  `#[ignore]`).
- `.github/workflows/ci.yml`: default job (fast: unit+proptest+mock tests via `cargo test
  --workspace`, `cargo clippy --workspace`, `cargo fmt --all --check`, sccache-cached native compile
  of `llama-cpp-2`).
- `.github/workflows/integration.yml` (or a second job in the same file, scheduled/manual trigger):
  full `--features integration` run across the workspace — default variant caches+uses only
  Qwen3-0.6B (no `ggmlc-run` build needed for the default integration job either, since Laya's
  integration test is itself `#[ignore]`-tagged); a separate scheduled/manual "full model matrix"
  job additionally builds `ggmlc-run` and exercises MiniCPM-2B, Qwen-4B, and Laya-en.

### Non-functional
- No test in the default (non-integration) job requires network access, a downloaded model, or a
  built `ggmlc-run` binary.
- CI workflow files are valid YAML, buildable in GitHub Actions syntax (manual review sufficient,
  no local Actions runner required for this plan).

## Architecture
```
crates/engine/src/traits.rs (or crates/pipeline/src/traits.rs — decide at implementation time)
  #[cfg_attr(test, mockall::automock)]
  pub trait InferenceEngine {
      fn tokenize(&self, text: &str) -> Result<Vec<i32>, EngineError>;
      fn decode_prompt(&mut self, tokens: &[i32]) -> Result<Vec<f32>, EngineError>;
      // + whatever decode_next/sample surface Phase 3 actually needs
  }
  impl InferenceEngine for engine::Engine { ... }   // real impl delegates to llama-cpp-2 calls

crates/models/src/laya.rs (extended, IF worth it — see below)
  #[cfg_attr(test, mockall::automock)]
  pub trait LayaScorer {
      fn score(&self, prompt: &str, options: &[String]) -> Result<Vec<(String, f32)>, LayaError>;
  }
  impl LayaScorer for LayaRunner { ... }
```
`pipeline::readout`/`generate` (Phase 2/3) take `&mut impl InferenceEngine` (or `&mut dyn
InferenceEngine`) instead of a concrete `engine::Engine` directly, IF this refactor is worth it —
see Risk Assessment for the trade-off; alternative is keeping pipeline functions concrete and only
adding mock-based tests for smaller sub-pieces (`constrained_softmax`, `strip_think`,
`parse_and_validate` are already pure and mock-free per Phase 2/3's own unit test sections). Same
logic applies to `pipeline::laya::run_laya` and a `LayaScorer` trait over `LayaRunner`. Default to
NOT introducing either trait unless the orchestration wrappers themselves have logic worth testing
beyond what the pure sub-functions already cover under mock-free unit tests. Decide at
implementation time based on actual code complexity, not preemptively.

## Related Code Files
- CREATE (conditionally, see Architecture note) `crates/engine/src/traits.rs` —
  `InferenceEngine` trait + `Engine` impl.
- CREATE (conditionally) — a `LayaScorer` trait + `LayaRunner` impl, likely inline in
  `crates/models/src/laya.rs` rather than a separate file (small surface).
- MODIFY `crates/pipeline/src/readout.rs`, `crates/pipeline/src/generate.rs`,
  `crates/pipeline/src/laya.rs` — only if the relevant trait is introduced; otherwise no change
  needed (pure sub-functions already testable).
- MODIFY `crates/engine/Cargo.toml`, `crates/models/Cargo.toml`, `crates/pipeline/Cargo.toml`,
  `apps/cli/Cargo.toml`, `apps/server/Cargo.toml` — add `mockall`, `proptest` as
  `[dev-dependencies]` where used; confirm each crate's `integration` feature exists and correctly
  gates the right test modules/files.
- CREATE `.github/workflows/ci.yml` — default fast job (workspace-wide).
- CREATE `.github/workflows/integration.yml` — scheduled/manual full-matrix job (workspace-wide +
  `ggmlc-run` build step).
- CREATE/MODIFY `crates/*/tests/`, `apps/*/tests/` (per-crate integration test dirs) or in-module
  `#[cfg(test)]` blocks — consolidate per Rust convention (prefer in-module `#[cfg(test)]` for unit
  tests per file, per-crate `tests/` dir only for true black-box integration tests per R4's split).

## Implementation Steps
1. Audit Phases 1-5's actual shipped test files against each phase's own "Test Strategy & Quality
   Gate" section — list any gap (missing edge case, missing proptest, missing e2e scenario),
   including Phase 2's Laya-specific additions and Phase 1's `models::laya` tests.
2. Decide (and document the decision, one paragraph each) whether `InferenceEngine` and/or
   `LayaScorer` trait extraction is worth it given the real complexity of `run_readout`/
   `run_generate`/`run_laya` as shipped — implement only if yes, independently for each.
3. Add/complete proptest properties for softmax (Phase 2) and strip+parse (Phase 3) per each
   phase's spec if gaps found in step 1.
4. Verify `integration` feature flag correctly gates every real-model/real-subprocess test (grep
   for `LlamaModel::load_from_file`/`Engine::load`/`ggmlc_run_path`/`Command::new("ggmlc-run")`
   calls in test code across all crates, confirm all are behind `#[cfg(feature = "integration")]`).
5. Write `.github/workflows/ci.yml`: checkout, install Rust toolchain (matching
   `rust-toolchain.toml`), install CMake/MSVC-equivalent (Windows runner) or Linux build deps,
   `Mozilla-Actions/sccache-action` setup, `cargo clippy --workspace -- -D warnings`, `cargo fmt
   --all -- --check`, `cargo test --workspace` (default features only — no `ggmlc-run` build step
   needed here).
6. Write `.github/workflows/integration.yml` (or extend ci.yml with a second job): trigger on
   schedule (e.g. nightly) + manual `workflow_dispatch`; cache Qwen3-0.6B GGUF via `actions/cache`
   keyed on the model's checksum/filename (per Phase 1's registry); run `cargo test --workspace
   --features integration`; a further manual-only job (or matrix input) additionally clones+builds
   `github.com/monatis/ggmlc`, downloads MiniCPM-2B, Qwen-4B, and Laya-en, and runs `cargo test
   --workspace --features integration -- --include-ignored` for the 4B and Laya cases if
   `#[ignore]`-tagged per Key Insights, accepting the larger cache/time/build cost since it's not
   per-PR. Note this CI job runs CPU-only, on generic CI-runner hardware, NOT the 32-vCPU target
   server — it validates correctness, not target-hardware performance (Phase 7).
7. Run the full test suite locally (`cargo test --workspace`, then `cargo test --workspace
   --features integration` with at least Qwen3-0.6B available) and paste results as this phase's
   completion evidence.

## Todo List
- [ ] Gap audit against Phases 1-5's own test sections completed, incl. Laya-specific tests
- [ ] `InferenceEngine`/`LayaScorer` trait decisions made + documented (implemented if warranted)
- [ ] Proptest gaps (if any) closed for softmax + strip/parse
- [ ] `integration` feature flag verified to gate all real-model AND real-`ggmlc-run` tests
      correctly, across all 5 testable crates
- [ ] `.github/workflows/ci.yml` written (fast default job, workspace-wide)
- [ ] `.github/workflows/integration.yml` written (scheduled/manual, Qwen3-0.6B default + full
      4-model matrix option incl. `ggmlc-run` build step)
- [ ] Full local test run (`cargo test --workspace` + `cargo test --workspace --features
      integration`) evidence pasted

## Success Criteria
- `cargo test --workspace` (default) runs with no network access, no model download, no
  `ggmlc-run` build, completes in seconds-to-low-minutes (native `llama-cpp-2` compile time aside,
  which is a one-time/cached cost).
- `cargo test --workspace --features integration` (with Qwen3-0.6B cached locally) passes all
  real-model scenarios named across Phases 1-5's own Quality Gate sections.
- CI workflow YAML is syntactically valid and matches the split described above (fast default +
  separate scheduled/manual heavy job with `ggmlc-run` provisioning).
- Diff coverage on all of Phases 1-5's new code meets each phase's stated ≥90%/≥75% thresholds
  (consolidated report, not just per-phase claims).

## Test Strategy & Quality Gate
Lane: normal. This phase's own gate is about the test suite's completeness and CI correctness, not
new product logic:
- Spec: Goal = every phase's Quality Gate commitments are actually met in the shipped code, plus a
  working CI split (fast default / heavy scheduled incl. Laya). AC: (1) Given the default feature
  set, When `cargo test --workspace` runs, Then it exits 0 with no network access. (2) Given
  `--features integration` and a cached Qwen3-0.6B, When `cargo test --workspace --features
  integration` runs, Then it exits 0. (3) Given the full source tree, When `cargo clippy --workspace
  -- -D warnings` runs, Then it exits 0. (4) Given both workflow YAML files, When parsed as GitHub
  Actions syntax, Then both are valid (manual review or `actionlint` if available). I/O contract:
  N/A (meta-phase over the test suite itself). Out-of-scope: actually provisioning a GitHub Actions
  runner/secrets for real CI execution (validated by inspection + local equivalent commands, not a
  live CI run, since this is a planning deliverable, not a deployed pipeline).
- Coverage target: consolidated diff coverage across all workspace crates' new code from Phases
  1-5, ≥90% line/≥75% branch, matching each phase's own stated target — this phase's job is to
  verify the aggregate, not to independently re-derive a different threshold.
- Evidence commands: `cargo test --workspace`, `cargo test --workspace --features integration`,
  `cargo clippy --workspace -- -D warnings`, `cargo fmt --all -- --check`, diff-coverage tool
  full-workspace report.

## Risk Assessment
- Risk: `crates/engine` and `crates/models` are core (not optional) dependencies on
  `llama-cpp-2`/`hf-hub`, so even the "fast default" CI job pays a native compile cost on
  cache-miss — mitigated by `sccache` (R4 §4), not eliminated; document this honestly rather than
  claiming a falsely-zero-cost default job.
- Risk: `actions/cache`'s 10GB/repo cap could be exceeded if the full 4-model matrix job's caches
  (now including Laya-en + the built `ggmlc-run` binary) and the default job's `target/`+sccache
  caches compete for the same budget — mitigate by scoping cache keys distinctly (model cache vs
  build cache vs `ggmlc-run` binary cache) and monitoring actual usage once CI is live; not fully
  solvable at plan time without real usage data.

## Security Considerations
- CI workflow should not print any HF tokens/secrets if a private/gated model repo is ever added
  later (not expected per Phase 0's licensing findings, all 4 models are public/Apache-2.0) — no
  secrets needed for v1's public repos.

## Next Steps
- Runs in parallel with Phase 7 (disjoint files: `crates/*/tests/`+`apps/*/tests/`+`.github/**`
  here vs `docs/benchmarks/**` there), both depending on Phases 1-5. Post-completion of both: user
  reviews the full plan bundle's Verification Gate evidence per `workflows/development-rules.md`
  before considering v1 done.
