---
phase: 4
name: apps/cli (openjev-cli binary)
status: pending
depends_on: [2, 3]
---

## Context Links
- `../plan.md` § Architecture (`apps/cli`, `timing` crate), § Decisions Locked #9 (Laya)
- `research/researcher-02-cli-http-architecture.md` §1, §3, §4
- `phase-02-pipeline-constrained-readout.md` (readout + laya pipelines), `phase-03-pipeline-generation.md`
  (generation pipeline) — all 3 methods this binary wraps

## Overview
- Priority: P1, parallel with Phase 5 — **fully disjoint now** (`apps/cli` vs `apps/server` are
  separate packages, no shared files at all; both depend on Phase 2+3).
- Status: pending.
- Implement `openjev-cli` — a one-shot benchmark binary (no subcommands; `serve` lives in its own
  `apps/server` binary per plan.md Decisions Locked #1, superseded/final): loads a model, runs all
  3 comparison methods (constrained readout, JSON generation, Laya single-pass scoring — plan.md
  Decisions Locked #9) against a given question, prints timing + results as JSON or human-readable
  text.

## Key Insights
- clap derive is the 2026-recommended API (R2 §3); parse in `main`, pass plain structs into
  library code — keeps `engine`/`pipeline`/`models` clap-free and reusable from `apps/server` too.
- Because `serve` is now a fully separate binary (plan.md Decisions Locked #1, final), this crate
  needs **no `Command`/subcommand enum at all** — a flat `#[derive(Parser)] struct Cli { ... }` is
  the whole args surface. This is simpler than the earlier single-binary design (no
  `Command::Bench`/`Command::Serve` split, no shared-file coordination with another phase).
- `Timings` struct (plain, `Instant`-delta based, `#[derive(Serialize)]`, its own `timing` crate) is
  the chosen approach over `tracing` spans (R2 §4) — simpler to unit-test, directly
  JSON-serializable for a one-shot benchmark report; this phase is where it's actually assembled
  end-to-end (model-load, warmup, tokenize, constrained-readout, generation — 5 distinct phases per
  original methodology).

## Requirements
### Functional
- `openjev-cli --model <qwen3-0.6b|minicpm-2b|qwen-4b> --prompt <text> --options <a,b,c,...>
  [--format json|text] [--threads <n>] [--batch-size <n>] [--skip-laya]`: loads the chosen LLM
  model (via `models`/`engine` crates), runs warmup, runs `run_readout` (Phase 2) and
  `run_generate` (Phase 3) against the same prompt+options; also runs `run_laya` (Phase 2, via
  `models::laya`) against the SAME options regardless of which LLM model was chosen (Laya is a
  separate, always-English-for-now, fixed model — not one of the 3 LLM `--model` choices) unless
  `--skip-laya` is passed. Prints a combined report with per-phase timings (model-load, warmup,
  tokenize, constrained-readout, generation, laya_model_load, laya_inference) and all 3 methods'
  results.
- `--skip-laya` opts out of the Laya comparison (default: Laya runs). When skipped,
  `timings.laya_model_load`/`laya_inference` stay `None` and the report's `laya` field is `None`/
  omitted — not a zeroed-out placeholder.
- `--threads`/`--batch-size` map directly to `engine::EngineConfig.n_threads`/`batch_size` (the LLM
  engine only — Laya has no equivalent knob, see Phase 1); omitted → `engine`'s default (near
  available core count). This is the exact knob Phase 7 sweeps on the real Linux server without
  needing code changes.
- `--format json` emits the full `Timings` + all 3 results as a single serialized JSON object (for
  scripting/comparison with the original browser tool's output); `--format text` (default) prints a
  readable summary.
- Model selection (`--model`) maps directly to the `models` crate's 3 registered LLM `ModelSpec`
  entries — this binary must support all 3, no subset. Laya-en is not part of this selection (it
  always runs, or is skipped entirely via `--skip-laya`).

### Non-functional
- Exit code 0 on success; non-zero + stderr message on any pipeline/engine error (download failure,
  checksum mismatch, decode failure, schema-invalid generation output is NOT a CLI failure — it's a
  valid reportable result, per Phase 3's risk note that mismatched generation output is expected
  real behavior). A Laya-specific failure (e.g. `ggmlc-run` binary missing) with `--skip-laya` NOT
  passed is also NOT a whole-run failure — report it as a `None`/error entry in the `laya` field of
  the JSON output, exit 0, rather than aborting the entire benchmark over an optional third method.

## Architecture
`Timings` is defined in the `timing` crate (its own crate so `pipeline` can depend on it without a
forward reference to this binary — see plan.md Architecture) — this binary POPULATES and PRINTS it,
it does not define the struct.
```
apps/cli/src/args.rs
  #[derive(Parser)] pub struct Cli {
      #[command(flatten)] pub model: ModelConfig,
      pub prompt: String, pub options: Vec<String>, pub format: OutputFormat,
      pub threads: Option<usize>, pub batch_size: Option<usize>,
      pub skip_laya: bool,   // default false (Laya runs by default)
  }
  #[derive(Args)] pub struct ModelConfig { pub model: ModelChoice, ... }   // ModelChoice: ValueEnum over the 3 registered LLM models
apps/cli/src/run.rs
  pub struct BenchReport { pub timings: timing::Timings, pub readout: pipeline::readout::ReadoutResult,
      pub generate: pipeline::generate::GenerateResult, pub laya: Option<Result<pipeline::laya::LayaResult, String>> }
      // laya: None if --skip-laya; Some(Err(msg)) if attempted but failed — never aborts the whole run
  pub fn run(args: Cli) -> Result<(), CliError>   // orchestrates: models::ensure_downloaded -> engine::Engine::load_from_spec -> warmup ->
                                                    //   run_readout -> reset_context -> run_generate -> (if !skip_laya) run_laya (isolated) -> print
apps/cli/src/main.rs
  fn main() { let cli = Cli::parse(); std::process::exit(match run::run(cli) { Ok(()) => 0, Err(e) => { eprintln!("{e}"); 1 } }); }
```
Laya's `run_laya` call is wrapped so its `Err` becomes `BenchReport.laya = Some(Err(msg))` rather
than propagating via `?` and aborting the whole binary — the 2 core LLM comparisons (readout,
generation) are the well-established, already-verified methods; Laya is a newer, additional
integration point, so a Laya-specific failure degrades gracefully rather than taking down the
entire benchmark run.

## Related Code Files
- CREATE `apps/cli/src/args.rs` — `Cli`, `ModelConfig`, `ModelChoice`, `OutputFormat` clap structs
  (flat parser, no subcommand enum), incl. `--skip-laya`.
- CREATE `apps/cli/src/run.rs` — `run()` orchestration, `BenchReport`.
- MODIFY `apps/cli/src/main.rs` — parse `Cli::parse()`, call `run::run`, map `Result` to exit code
  (fills in Phase 0's trivial stub).
- MODIFY `apps/cli/Cargo.toml` — confirm `engine`/`models`/`pipeline`/`timing` path deps + `clap`/
  `serde_json` present (scaffolded by Phase 0, verify not missing; add any additional deps needed).

## Implementation Steps
1. Define `ModelConfig`/`ModelChoice` (enum over the 3 registered LLM models) in
   `apps/cli/src/args.rs`, `#[derive(Args)]`/`#[derive(ValueEnum)]` as appropriate for clap.
2. Define the top-level `Cli` struct flattening `ModelConfig` + prompt/options/format/threads/
   batch-size/skip-laya fields — no subcommand wrapper needed.
3. Implement `run::run`: resolve `ModelChoice` -> `models::ModelSpec` (registry) -> time
   `engine::Engine::load_from_spec` (with `EngineConfig` from `--threads`/`--batch-size`) as
   `model_load`, `Engine::warmup` as `warmup`, tokenize options as part of `tokenize` phase
   (`Timings.tokenize` = time to tokenize the candidate options once, shared before both pipelines
   run since both need tokenized options — Phase 2/3's own internal timing covers only their
   decode+softmax/decode+loop portions, not this shared tokenize step, avoiding double-counting).
4. Call `run_readout`, then `Engine::reset_context` (Phase 1 primitive — clears KV-cache state so
   the generation pipeline doesn't inherit stale context from the readout pipeline), then
   `run_generate`; merge both pipelines' internal timing deltas into the shared `Timings`
   (`constrained_readout`, `generation` fields).
5. If `!args.skip_laya`: resolve the Laya-en `ModelSpec`, call
   `models::laya::LayaRunner::load_from_spec` (timed into `timings.laya_model_load`), then
   `pipeline::laya::run_laya` (timed into `timings.laya_inference`) — wrap the whole step so any
   `Err` becomes `BenchReport.laya = Some(Err(err.to_string()))` rather than propagating and
   aborting the run; on success, `BenchReport.laya = Some(Ok(result))`. If `skip_laya`, leave
   `BenchReport.laya = None` and both Laya `Timings` fields `None`.
6. Implement output: `--format json` -> `serde_json::to_string_pretty(&report)` where `report`
   combines `Timings` + `ReadoutResult` + `GenerateResult` + the `laya` field (as described above);
   `--format text` -> formatted println, including a Laya section only when `laya.is_some()`.
7. Wire `main.rs`: `Cli::parse()`, call `run::run`, map any `Err` to a printed error + non-zero
   exit.

## Todo List
- [ ] `ModelConfig`/`ModelChoice`/`Cli` (flat, no subcommand) clap structs in `apps/cli/src/args.rs`,
      including `--threads`/`--batch-size`/`--skip-laya`
- [ ] `run::run` orchestrates full flow, all 3 LLM models selectable, calls `Engine::reset_context`
      between `run_readout` and `run_generate`
- [ ] Laya step implemented: runs by default, isolated error handling (never aborts the whole run),
      skippable via `--skip-laya`
- [ ] JSON and text output formats both implemented, including the `laya` field/section
- [ ] `main.rs` parses args, dispatches to `run::run`, correct exit codes

## Success Criteria
- `openjev-cli --model qwen3-0.6b --prompt "..." --options A,B --format json` runs end-to-end
  against a real model and prints valid JSON with all 7 timing fields (5 required + 2 Laya) present
  and all 3 methods' results present.
- `openjev-cli --model qwen3-0.6b --prompt "..." --options A,B --skip-laya --format json` runs
  end-to-end with `laya: null`/absent and the 2 Laya timing fields `null`, everything else
  unchanged.
- All 3 registered LLM models are selectable and load correctly (manual verification, at least once
  per model given download size/time cost).
- A missing/broken `ggmlc-run` binary does NOT fail the whole run when Laya isn't skipped — the
  `laya` field reports the error, `readout`/`generate` still complete normally, exit code is still
  0.
- Errors (e.g. bad model name) produce a clear message + non-zero exit, not a panic/stack trace.

## Test Strategy & Quality Gate
Lane: normal.
- Spec: Goal = a single binary invocation that runs all 3 comparison methods against one
  model+question and reports timings+results, degrading gracefully if only the optional Laya path
  fails. AC: (1) Given valid args and a cached model, When `openjev-cli` runs, Then exit 0 and JSON
  output parses with all `Timings` fields present and all 3 results present. (2) Given an invalid
  model name, When it runs, Then exit non-zero with a clear stderr message, no panic. (3) Given
  `--format text`, When it runs, Then output is human-readable (non-empty, no raw Debug dump). (4)
  Given `--skip-laya`, When it runs, Then `laya` is absent/null and both Laya timing fields are
  null, everything else unchanged. (5) Given a Laya-scoring failure (e.g. simulated
  `LayaError::BinaryNotFound` via a mock in the unit-test layer) and `--skip-laya` NOT passed, When
  `run::run` executes, Then it still exits 0 with `readout`/`generate` populated and `laya`
  reporting the error. I/O contract: CLI args -> stdout (JSON or text) + exit code. Out-of-scope:
  HTTP surfacing (Phase 5), model download reliability under network failure (covered by Phase 1's
  own tests), `models::laya`'s own subprocess correctness (Phase 1).
- Pyramid ~60/20/20 (CLI orchestration layer is thin but the e2e scenario carries real weight
  here): unit tests for arg parsing (`Cli::try_parse_from` with various arg combos, valid/invalid,
  incl. `--skip-laya`), output formatting (`Timings`/report `Serialize` round-trip), and the
  Laya-failure-doesn't-abort-the-run behavior (AC #5, mockable without a real `ggmlc-run`);
  integration test (feature-gated) running the actual `openjev-cli` binary against Qwen3-0.6B (+
  Laya) end-to-end as the primary e2e scenario named "openjev-cli --model qwen3-0.6b with a
  2-option question produces valid JSON with all timing fields and all 3 results."
- Coverage target: ≥90% line / ≥75% branch on `apps/cli/**` new code (`crates/timing/**` itself is
  Phase 1's coverage responsibility, defined there — this binary only consumes it).
- Evidence commands: `cargo test -p cli` (arg parsing + formatting + Laya-failure-isolation, no
  model/subprocess needed), `cargo test -p cli --features integration` (the e2e scenario), diff-
  coverage tool report.

## Risk Assessment
- Timing ownership is decided (not deferred): `Timings.tokenize` covers only the shared
  options-tokenization step this binary performs once before both pipelines run; Phase 2/3's
  `run_readout`/`run_generate` internal instrumentation feeds `constrained_readout`/`generation`
  fields exclusively. Document this split in code comments at the `Timings` field definitions
  (Phase 1) so it isn't re-litigated per call site.
- Forgetting the `Engine::reset_context` call between `run_readout` and `run_generate` would let
  generation silently inherit readout's KV-cache state, skewing both correctness and timing —
  step 4 makes this call explicit; a code-review checklist item, not just prose here.
- Forgetting to isolate the Laya step's error (letting it propagate via `?` like the other
  pipelines) would make the whole binary fail whenever `ggmlc-run` has any issue — since Laya is
  the newest, least-verified integration point, this would make the CLI needlessly fragile; step 5
  makes the isolation explicit.

## Security Considerations
- CLI accepts local file/text input only (prompt/options as args) — no network-facing surface in
  this binary (that's Phase 5's `apps/server`). No injection risk beyond normal shell-arg handling
  clap already covers.

## Next Steps
- Phase 5 (`apps/server`) reuses `ModelConfig`-equivalent resolution, `Engine`, `run_readout`/
  `run_generate`/`run_laya`, and `Timings` — same orchestration logic (including the
  Laya-isolated-error pattern), different transport (HTTP request/response instead of stdout), and
  now a fully separate binary with zero shared files.
