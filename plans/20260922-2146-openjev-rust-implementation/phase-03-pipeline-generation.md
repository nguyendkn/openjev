---
phase: 3
name: Pipeline - JSON Generation
status: pending
depends_on: [1]
---

## Context Links
- `../plan.md` § Decisions Locked (#6), § Architecture (`pipeline` crate)
- `docs/research/openjev-rust-research.md` §1, §4 (max_tokens=512, strip `<think>`, validate schema)
- `research/researcher-01-llama-cpp-2-engine.md` §2 (greedy loop pattern, confirmed)
- `phase-01-engine-model-management.md` (`Engine::decode_prompt`, `decode_next`, `sample_greedy`,
  `tokenize`, `timing` crate)

## Overview
- Priority: P0, parallel with Phase 2 (both write to the `pipeline` crate, one small shared-file
  touchpoint — see plan.md Parallel Execution Matrix — both depend only on Phase 1).
- Status: pending.
- Implement the "generation" path in the `pipeline` crate: model free-generates `{"option": prob,
  ...}` token-by-token (greedy, max 512 tokens), strip any `<think>...</think>` block, parse +
  validate resulting JSON against the question's option set.

## Key Insights
- Generation loop pattern confirmed in R1 §2 (`examples/simple`): `LlamaSampler::chain_simple([dist,
  greedy()])`, loop `sample -> check EOG -> decode_next -> increment counter -> check max_tokens`.
  Reuse this exact shape via `Engine::sample_greedy` + `Engine::decode_next` (both owned and
  implemented by Phase 1's `engine` crate — this phase only calls them, it does not touch
  `crates/engine/**`).
- `<think>...</think>` is Qwen3-family reasoning-model output — ordinary generated tokens, no
  special engine handling; it's purely a post-processing string-strip step before JSON parsing
  (R1 §2, confirmed no engine-level special case needed).
- Max tokens = 512 is a loop-counter stop condition (`n_cur >= n_len`), same as EOG-token stop —
  whichever triggers first ends generation; both are valid legitimate stops, not just the 512 cap.
- Schema validation must check: valid JSON object, keys are a subset/exact-match of the question's
  option set (decide subset-vs-exact per what "validate schema" implies — plan default: keys must
  exactly match the given option set, values must be numbers in [0,1]; reject extra/missing keys or
  non-numeric/out-of-range values), matching the original's semantics of comparing against direct
  readout for the SAME option set.

## Requirements
### Functional
- `run_generate(engine, prompt, options, timings) -> Result<GenerateResult, PipelineError>`:
  greedy-decode up to 512 tokens or EOG, decode the full raw output string.
- `strip_think(raw: &str) -> String`: remove `<think>...</think>` block(s) if present, return
  remainder trimmed; must not panic on missing closing tag (return original string unstripped, or a
  structured error — decide and document, since an unterminated block mid-512-token-cutoff is a
  realistic case per R4 §3).
- `parse_and_validate(stripped: &str, options: &[String]) -> Result<GenerateResult, PipelineError>`:
  `serde_json::from_str` into a `HashMap<String, f64>` (or dedicated struct), validate keys exactly
  match `options`, values are finite numbers in `[0,1]`.
- Time tokenize/decode-loop/strip+parse steps separately into the shared `timing::Timings`.

### Non-functional
- `strip_think` and `parse_and_validate` are pure functions, independently unit-testable without a
  loaded model (feed canned strings).
- Never panic on malformed model output (truncated JSON at the 512-token cutoff, missing closing
  tag, non-JSON garbage) — always return a structured `Err`.

## Architecture
```
crates/pipeline/src/generate.rs
  pub struct GenerateResult { pub probabilities: HashMap<String, f64>, pub raw_output: String }
  pub fn strip_think(raw: &str) -> String                                    // pure, no panic
  pub fn parse_and_validate(stripped: &str, options: &[String]) -> Result<GenerateResult, PipelineError>  // pure
  pub fn run_generate(engine: &mut engine::Engine, prompt: &str, options: &[String], timings: &mut timing::Timings) -> Result<GenerateResult, PipelineError>  // orchestration
```

## Related Code Files
- CREATE `crates/pipeline/src/generate.rs` — `strip_think`, `parse_and_validate`, `run_generate`,
  `GenerateResult`.
- MODIFY `crates/pipeline/src/lib.rs` — append `pub mod generate;` (shared file, see plan.md
  Parallel Execution Matrix's "Shared integration files" note — Phase 2 may create this file first
  with its own `pub mod readout;` line; this phase only appends its own line, never removes/
  reorders Phase 2's).
- MODIFY/CREATE `crates/pipeline/src/error.rs` — add `PipelineError` variants: `JsonParseError`,
  `SchemaMismatch { expected: Vec<String>, got: Vec<String> }`, `ValueOutOfRange` (shared file with
  Phase 2, same additive-only convention: only add new variants, never touch Phase 2's).
- MODIFY `crates/pipeline/Cargo.toml` — confirm `engine`/`timing` path deps present (Phase 2 likely
  adds them first in the same wave; if this phase lands first, add them here instead — additive,
  not conflicting, since it's the same two dependency lines either way).

## Implementation Steps
1. Implement `strip_think`: find `<think>` / `</think>` markers; if both present, remove everything
   between and including them (handle multiple occurrences by stripping all); if only an opening
   tag with no closing tag (truncated at 512-token cutoff), strip from `<think>` to end-of-string
   and return the empty/whatever-precedes-it remainder — document this choice as the intentional
   handling of the truncation case.
2. Implement `parse_and_validate`: `serde_json::from_str::<serde_json::Value>` first (catch parse
   errors as `PipelineError::JsonParseError`), then check it's an object, then check key set exactly
   equals `options` (as a set), then check every value is a JSON number, finite, in `[0,1]` — collect
   into `GenerateResult.probabilities`.
3. Implement `run_generate`: build the prompt (ChatML-formatted per Phase 0's chat-template
   decision — hand-rolled string if `llama-cpp-2` has no template wrapper), tokenize, `decode_prompt`
   once, then loop: call `Engine::sample_greedy` (Phase 1's primitive) for the next token, append to
   output buffer, stop on EOG or 512-token cap, else `Engine::decode_next` and continue; decode
   final token buffer to string via `llama-cpp-2`'s `token_to_str`. Callers are responsible for
   calling `Engine::reset_context` before this if a prior pipeline run shares the same `Engine`
   instance (Phase 4/5's orchestration does this).
4. Call `strip_think` then `parse_and_validate` on the decoded string; record timing for
   tokenize/generate-loop/strip+parse as separate `Instant` deltas.

## Todo List
- [ ] `strip_think` implemented, handles present/absent/unterminated/multiple `<think>` blocks
- [ ] `parse_and_validate` implemented, rejects extra/missing keys, non-numeric/out-of-range values
- [ ] `run_generate` implemented: prompt build, greedy loop via `Engine::sample_greedy`/
      `decode_next` (max 512 or EOG), timing recorded
- [ ] `PipelineError` variants cover parse failure, schema mismatch, out-of-range value

## Success Criteria
- `strip_think`/`parse_and_validate` never panic on any adversarial string input (garbage, empty,
  truncated, nested tags) — always `Ok` or structured `Err`.
- Given a real loaded model (Qwen3-0.6B) and a simple question, `run_generate` produces output that,
  after strip+parse, is either valid schema-matching JSON or a clearly-categorized `Err` (manual
  sanity check — model may not always produce perfect JSON, that's expected real-world behavior to
  surface, not to hide).
- Generation loop respects the 512-token cap (verify via a forced-long-output test case if feasible,
  or code inspection confirming the loop-counter check is unconditional).
- CPU latency reference (informational only, not a hard threshold): record wall-clock time for
  `run_generate` on the dev machine as a sanity baseline. This environment is CPU-only — do not
  compare against the original browser tool's GPU/WebGPU numbers (docs/research §1's "~5.33s on
  RTX 3090" figure is a different hardware class entirely). Real target-hardware latency (32-vCPU
  Linux server) is measured and tuned in Phase 7.

## Test Strategy & Quality Gate
Lane: normal.
- Spec: Goal = parse model-generated `<think>`-wrapped JSON into a validated probability map, never
  panicking on malformed output. AC: (1) Given raw text with a well-formed `<think>...</think>`
  prefix followed by valid JSON, When stripped+parsed, Then returns `Ok` with matching keys. (2)
  Given text with an unterminated `<think>` tag, When stripped, Then returns a defined (documented)
  result, never panics. (3) Given JSON with a key not in `options`, When validated, Then returns
  `Err(SchemaMismatch)`. (4) Given a value outside `[0,1]`, When validated, Then returns
  `Err(ValueOutOfRange)`. I/O contract: see Architecture signatures. Out-of-scope: readout path
  (Phase 2), CLI/HTTP surfacing (Phase 4/5).
- Pyramid ~70/20/10: unit tests for each explicit edge case in R4 §3 (no tags, missing close,
  nested/duplicate tags, JSON-inside-think, malformed JSON, truncated-at-cutoff, schema mismatch,
  empty/whitespace input) as example-based tests; proptest property "for any string, strip+parse
  either returns Ok(valid-schema) or a structured Err — never panics" fuzzing random strings around
  the tag/JSON structure (R4 §3's recommended fuzz property); 1 integration test (feature-gated,
  real Qwen3-0.6B) as e2e — "given a real prompt, generation completes within 512 tokens and
  produces a parseable-or-cleanly-erroring result."
- Coverage target: ≥90% line / ≥75% branch on `crates/pipeline/src/generate.rs`.
- Evidence commands: `cargo test -p pipeline generate::` (unit + proptest), `cargo test -p pipeline
  --features integration generate::` (e2e scenario), diff-coverage tool report for this file.

## Risk Assessment
- Risk: greedy decoding on a real model may rarely emit perfectly-schema-matching JSON for
  ambiguous prompts — this is expected/real behavior per the original tool's own comparison premise
  (that's WHY it compares against constrained readout); do not "fix" by loosening validation, surface
  as `Err(SchemaMismatch)` faithfully.
- Risk: none remaining on sampler plumbing — `Engine::sample_greedy`/`reset_context` are owned and
  implemented by Phase 1's `engine` crate (a hard dependency, wave-ordered before 2/3), so this
  phase only ever calls an already-existing primitive.

## Security Considerations
- `serde_json::from_str` on untrusted model output is memory-safe (no eval/exec); still bound
  parsing to the 512-token cap already enforced upstream so pathological output size is capped.

## Next Steps
- Phase 4 (`apps/cli`) and Phase 5 (`apps/server`) both call `run_generate` and surface
  `GenerateResult` + `Timings`, alongside Phase 2's `ReadoutResult`, for the side-by-side comparison
  the original tool presents.
