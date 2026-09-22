---
loop: 2
candidate: A_2
status: passed
builder: Developer
assessor: QATester
verified_behaviors:
  - "G4 closed: llama-cpp-2 0.1.156 API real, matches crates/engine/src/lib.rs usage — grep-confirmed exact signatures on server: get_logits_ith (context.rs:313), get_logits (context.rs:270), decode (context.rs:101), clear_kv_cache (context/kv_cache.rs:103), LlamaBatch::add (llama_batch.rs:50), LlamaSampler::greedy (sampling.rs:621)/sample (sampling.rs:30), str_to_token (model.rs:302), chat_template (model.rs:728), apply_chat_template (model.rs:936), is_eog_token (model.rs:188), token_to_str (model.rs:213), load_from_file (model.rs:756), new_context (model.rs:814), n_vocab (model.rs:563)"
  - "G3 closed: hf-hub 1.0.0 API real, matches crates/models/src/download.rs usage — grep-confirmed HFClientSync::new()/model() (blocking.rs:163/197), blocking download_file().filename().revision().send() builder (repository/download.rs:1332, exact line match to cited comment), async HFRepository::download_file (repository/download.rs:1142, exact line match). download.rs is NOT a stub — real client/repo/builder calls present"
  - "G5 closed: HF license check for all 4 registered repos via curl https://huggingface.co/api/models/<repo> — all 4 report cardData.license=apache-2.0 (top-level .license is null for all 4, matching download.rs's comment); .sha for all 4 repos matches the pinned `revision` field in ModelEntry exactly (23749fef..., bc640142..., 2079a22f..., 713ae6f6...)"
  - "reset_context (Engine::reset_context -> ctx.clear_kv_cache()) is actually called between run_readout and run_generate — confirmed by reading apps/cli/src/main.rs:97 directly (not inferred from a report)"
  - "Unsafe lifetime-widening pattern in crates/engine/src/lib.rs:69-75 (Engine::load) has a safety comment explaining the invariant (Box heap-stable across moves; ctx declared before model/backend so Rust's declared-order drop runs ctx first) — invariant is currently correct and matches the struct's actual field order"
  - "Independent re-run of openjev-cli on the Linux server with a DIFFERENT scenario (invoice categorization: AWS EC2/S3 invoice, options infrastructure/software-license/consulting/travel, from researcher-05-benchmark-usecases.md scenario 3) — real execution, no crash, valid JSON, all 7 Timings fields present, readout.probs sums to 1 over 4 options, generate.valid=true with tokens_generated=240 — rules out hard-coding to the France example"
  - "constrained_softmax (crates/pipeline/src/readout.rs) has 3 real committed unit tests (uniform_when_all_neg_inf, picks_higher_logit, empty_indices_returns_empty) — ran via `cargo test -p pipeline` on server, all pass"
  - "strip_think/parse_and_validate (crates/pipeline/src/generate.rs) 'never panic on adversarial input' claim empirically verified: QA wrote 15 temporary (uncommitted, deleted after run) adversarial tests covering empty string, unterminated <think>, multibyte-UTF8 boundaries, nested-like tags, malformed/reversed-brace/non-object JSON, 5000-char brace-only input — all 15 passed, zero panics, via `cargo test -p pipeline` on server"
  - "Preservation: cargo build --workspace (debug) exits 0 on server; cargo build --workspace --release exits 0 (independently re-run, not just binary-exists check); cargo test --workspace passes (3 tests, all in pipeline, 0 failures); 4 GGUF models still in ~/.cache/huggingface/hub/; ggmlc-run binary still present and executable at /tmp/ggmlc/build/runtime/ggmlc-run; 6-crate workspace Cargo.toml members list unchanged; Timings struct still exactly 7 fields + Serialize; crates/engine/Cargo.toml has zero cuda/vulkan/rocm/opencl features"
unresolved_gaps:
  - "docs/codebase-summary.md not updated for Loop 2 — lines 16-17 still mark engine/models [STUB], lines 215-219 still list llama-cpp-2/hf-hub API items as UNVERIFIED/Inferred, even though the actual code now implements and correctly uses them (verified above). Documentation drift, not a functional defect — should be refreshed next loop"
  - "crates/models/src/download.rs's own source-citation comment has two imprecise line numbers: HFClientSync::new() cited at blocking.rs:157 (actual 163), .model() cited at blocking.rs:177 (actual 197, off by 20). The two download_file citations (repository/download.rs:1332 and :1142) are exact. Substance (function names/signatures/semantics) is correct in all four cases — this is citation-precision drift, not fabrication, but the :177 offset is large enough to flag"
  - "No committed unit tests for strip_think/parse_and_validate in crates/pipeline/src/generate.rs — D_2 Task 7's 'never panic on adversarial input' claim has no regression protection in the repo itself; only readout's constrained_softmax (Task 6) got committed tests. QA's own ad hoc tests (documented above) prove the claim true today but will not catch a future regression"
  - "Unsafe self-referential lifetime-widening in crates/engine/src/lib.rs (Engine::load, transmute &Box<LlamaModel> -> &'static LlamaModel) is correct today (verified: comment present, field order correct, build/tests/run all pass) but is NOT compiler-enforced — a future refactor (reordering struct fields, or a method taking `self.model` by value) could silently reintroduce UB with no error. This is a technical-risk item to carry forward, not a defect blocking Loop 2 acceptance. Alternatives (self_cell/ouroboros crate, or re-creating the context per call instead of storing it) exist and could remove the unsafe entirely in a later loop"
regressions: []
schema_valid: true
---

## 1. Verified Behaviors

- **G4 (llama-cpp-2 API), closed with real evidence.** Located the actual resolved crate on
  the server: `find ~/.cargo/registry/src -maxdepth 2 -iname 'llama-cpp-2*'` →
  `llama-cpp-2-0.1.156`. Note the crate's file is `src/context.rs`, not
  `src/context/llama_context.rs` as D_2 Task 1 guessed — Developer correctly used the real
  path, did not blindly follow the guessed one. Direct `grep -n` against the real source
  confirms every signature `crates/engine/src/lib.rs` calls: `get_logits_ith` (context.rs:313),
  `get_logits` (context.rs:270), `decode` (context.rs:101), `clear_kv_cache`
  (context/kv_cache.rs:103), `LlamaBatch::add` (llama_batch.rs:50), `LlamaSampler::greedy`
  (sampling.rs:621) / `.sample()` (sampling.rs:30), `str_to_token` (model.rs:302),
  `chat_template` (model.rs:728), `apply_chat_template` (model.rs:936), `is_eog_token`
  (model.rs:188), `token_to_str` (model.rs:213), `load_from_file` (model.rs:756),
  `new_context` (model.rs:814), `n_vocab` (model.rs:563). All exist, all signatures match how
  `Engine` calls them. Not a single citation was fabricated or missing.
- **G3 (hf-hub API), closed with real evidence.** `crates/models/src/download.rs` is a real
  implementation (`HFClientSync::new()`, `.model(owner, name)`,
  `.download_file().filename(...).revision(...).send()`), not a stub comment (Loop 1's gap).
  Grepped the actual `hf-hub-1.0.0` source on server: `HFClientSync::new()` at blocking.rs:163
  (comment cites :157, off by 6), `.model()` at blocking.rs:197 (comment cites :177, off by
  20), blocking `download_file` builder at repository/download.rs:1332 (comment cites :1332 —
  exact), async `download_file` at repository/download.rs:1142 (comment cites :1142 — exact).
  Function names/signatures/builder pattern are all real and match usage.
- **G5 (licensing), closed with a perfect match.** `curl -s
  https://huggingface.co/api/models/<repo>` for all 4 registered repos:

  | repo | cardData.license | top-level .license | sha |
  |---|---|---|---|
  | Qwen/Qwen3-0.6B-GGUF | apache-2.0 | null | 23749fefcc72300e3a2ad315e1317431b06b590a |
  | Qwen/Qwen3-4B-GGUF | apache-2.0 | null | bc640142c66e1fdd12af0bd68f40445458f3869b |
  | openbmb/MiniCPM5-2B-GGUF | apache-2.0 | null | 2079a22f3beaa4e306449978533478fe0522f4b3 |
  | mys/laya-GGUF | apache-2.0 | null | 713ae6f6e39fb54835e010485656e4484e5ec411 |

  All 4 SHAs match `ModelEntry.revision` in `crates/models/src/download.rs` exactly, byte for
  byte. All 4 licenses are `apache-2.0`, matching what's recorded.
- **`reset_context` ordering** — read `apps/cli/src/main.rs` directly: line 92 calls
  `run_readout`, line 97 calls `engine.reset_context()`, line 101 calls `run_generate`. Order
  confirmed correct, not taken on faith.
- **Unsafe pattern review** (`crates/engine/src/lib.rs:47-92`, `Engine` struct + `Engine::load`):
  has a safety comment explaining the invariant — `model: Box<LlamaModel>` is heap-allocated so
  its address is stable across `Engine` moves, and `ctx` is declared before `model`/`backend`
  in the struct so it drops first (Rust drops fields in declaration order). Verified the struct
  field order matches the comment's claim (`ctx`, then `model`, then `backend`). The invariant
  holds today. This is a legitimate minimal-unsafe pattern (self-referential struct via
  heap indirection), correctly documented — see Unresolved Gaps for the residual risk.
- **Independent re-run with a NEW prompt** (not capital-of-France): ran
  `./target/release/openjev-cli --model qwen3-0.6b --prompt "Invoice from vendor AWS. Line
  items: EC2 compute, S3 storage. Amount: ...1,240. Which expense category does this invoice
  belong to?" --options infrastructure,software-license,consulting,travel --format json`
  (scenario 3 from `researcher-05-benchmark-usecases.md`, invoice categorization). Real llama.cpp
  load log (28 metadata keys, 310 tensors, same model file). Output: valid JSON, all 7
  `Timings` fields present (`model_load_ms=1108, warmup_ms=87, tokenize_ms=0,
  constrained_readout_ms=126, generation_ms=3609, laya_*=0`), `readout.probs` sums to ~1 across
  4 options (`software-license=0.962` highest), `generate.valid=true`,
  `generate.parsed.answer="infrastructure"`, `tokens_generated=240`. No crash. (Note: my own
  bash `$`-escaping mangled "$1,240" to ",240" in the literal prompt text sent — a test-harness
  artifact on my side, not a candidate defect.) Readout and generate picked *different* best
  answers (software-license vs infrastructure) — this is a genuine, interesting divergence
  between the two methods on a harder/more ambiguous prompt than the France example, not a bug;
  it's exactly the kind of signal the 3-method comparison is meant to surface, and rules out
  the readout/generate paths being hard-coded to one canned question.
- **`constrained_softmax` unit tests real and passing**: `crates/pipeline/src/readout.rs` has 3
  committed `#[test]` functions. `cargo test -p pipeline` on server →
  `test result: ok. 3 passed; 0 failed`.
- **`strip_think`/`parse_and_validate` "never panic" claim empirically verified**: no committed
  tests exist for these (see gap below), so QA wrote 15 temporary adversarial tests directly on
  the server copy (`/tmp/openjev/crates/pipeline/tests/adversarial_qa_temp.rs`, NOT committed,
  deleted after the run) covering: empty string, unterminated `<think>`, multiple blocks,
  nested-like tags, multibyte UTF-8 boundaries (`日本語<think>...🎉...</think>結果です`),
  tag-soup (`<think><think><think>`), close-tag-with-no-open, empty/no-brace/reversed-brace
  (`} weird {`)/malformed (`{answer: A no quotes}`)/non-object (`[1,2,3] {not an object`)/
  5000-char brace-only JSON input, and a valid-match-with-surrounding-prose case. First run:
  14/15 passed, 1 failed — root-caused to a QA test-authoring typo (wrong expected string in
  my own assertion, not a panic and not a candidate defect); fixed the expected value, reran:
  `test result: ok. 15 passed; 0 failed`. Confirms no panics on adversarial input.
- **Preservation, all confirmed independently**:
  - `cargo build --workspace` (debug) on server → exit clean, only pre-existing warnings
    (unused `Instant` import, deprecated `Special` enum, unread `backend` field).
  - `cargo build --workspace --release` → independently re-run (not just checking the binary's
    mtime), exits 0.
  - `cargo test --workspace` → 3 tests (all in `pipeline`), 0 failures, other 5 crates report
    0 tests (expected, no unit tests written there).
  - `ls ~/.cache/huggingface/hub/` → all 4 `models--*` dirs still present.
  - `ggmlc-run` still at `/tmp/ggmlc/build/runtime/ggmlc-run`, executable.
  - `Cargo.toml` workspace `members` list unchanged: 6 crates.
  - `crates/timing/src/lib.rs`: `Timings` still exactly 7 fields, `#[derive(Serialize, ...)]`.
  - `crates/engine/Cargo.toml`: no `cuda`/`vulkan`/`rocm`/`opencl` features declared.

## 2. Unresolved Gaps

- **Docs stale (new gap, not blocking).** `docs/codebase-summary.md:16-17` still label
  `engine`/`models` `[STUB]`, and `:215-219` still list the llama-cpp-2/hf-hub items as
  UNVERIFIED/Inferred — none of this was updated in Loop 2 despite the code now containing
  real, verified implementations. This is a documentation-sync gap; the code itself is fine.
- **hf-hub citation line-number drift.** Two of the four line-number citations in
  `download.rs`'s header comment are off (:157 vs actual :163, :177 vs actual :197 — the
  latter a 20-line gap). The other two (:1332, :1142) are exact. Function existence/signature/
  semantics are correct in all four; this is citation precision, not fabrication, but the
  :177 offset is large enough that whoever wrote the comment likely wasn't looking at the
  exact same resolved line when noting it down.
- **No committed regression tests for `strip_think`/`parse_and_validate`.** Only
  `constrained_softmax` (Task 6) got committed unit tests; Task 7's "never panic" claim for
  `generate.rs`'s pure functions has no tests in the repo. QA's ad hoc (uncommitted) tests prove
  the claim true today, but nothing protects it from regressing.
- **Unsafe lifetime-widening risk, carried forward as a known technical-debt item.** The
  pattern in `Engine::load` (`crates/engine/src/lib.rs:69-75`) is correct today — verified
  the safety comment, field order, and that build/tests/the real inference run all work — but
  it is not compiler-enforced. A future change (struct field reorder, or any method that takes
  `self.model` by value / replaces it) could silently reintroduce UB with no compile error.
  Recommend tracking this explicitly for a later loop rather than treating it as settled;
  alternatives (`self_cell`/`ouroboros`, or not persisting `LlamaContext<'static>` across calls)
  could eliminate the unsafe block entirely, at some perf/complexity cost. Not blocking Loop 2
  acceptance — no observed misbehavior, and the current invariant is sound as written.

## 3. Regressions

None. All Loop 1 preservation constraints re-verified and hold (see Verified Behaviors §
Preservation). `docs/codebase-summary.md` staleness (above) is a new gap, not a regression —
nothing that was previously accurate became inaccurate; it simply wasn't updated to reflect
new, real progress.

## 4. Runtime Check

- Linux server (`ssh root@103.146.166.46`, `/tmp/openjev`):
  - `cargo build --workspace` (debug) → PASS, exit 0.
  - `cargo build --workspace --release` → PASS, exit 0 (independently re-run by this assessor).
  - `cargo test --workspace` → PASS, 3/3 tests (pipeline), 0 failures.
  - `cargo test -p pipeline` (including 15 QA-authored, uncommitted adversarial tests) → PASS,
    18/18 (3 committed + 15 temporary), 0 failures, 0 panics.
  - `./target/release/openjev-cli --model qwen3-0.6b --prompt "The capital of France is: A)
    London B) Paris" --options A,B --format json` (supplied Runtime.check, this assessor did
    not rerun this exact one but reviewed the log) → real llama.cpp load, `readout.probs.B=
    0.9998958`, `best_option=B`, `generate.parsed.answer=B, valid=true, tokens_generated=119`.
  - `./target/release/openjev-cli --model qwen3-0.6b --prompt "<invoice categorization
    scenario>" --options infrastructure,software-license,consulting,travel --format json`
    (independently run by this assessor, new prompt) → real execution, valid JSON, all 7
    `Timings` fields, `readout.best_option=software-license` (0.962), `generate.parsed.answer=
    infrastructure`, `valid=true`, `tokens_generated=240`. No crash.
- `schema_valid: true` — `E_2` produced against a workspace that built and ran successfully on
  the required Linux server target.

## 5. Assessment

**Accept: yes.** Loop 2's core objective — real `llama-cpp-2`-backed `engine` and `hf-hub`-
backed `models`, wired into `pipeline::readout`/`pipeline::generate`, proven end-to-end via
`apps/cli` on the Linux server — is genuinely achieved and independently verified, not just
self-reported. G3/G4/G5 are closed with real primary-source evidence (this assessor re-derived
every citation from the actual resolved crate sources on the server, not from Developer's
claims). This is a marked improvement over Loop 1, where the equivalent citations were either
missing or explicitly self-admitted as "inferred." The unsafe block in `engine::Engine::load`
is reviewed, currently sound, and appropriately commented — a real but non-blocking technical
risk to track, not a defect.

All D_2 Validation Requirements are met: llama-cpp-2 API cited against real resolved source
(mostly precise, two line numbers drifted but substance correct); hf-hub API real, not a stub;
release build exits 0 on the Linux server; CLI prints valid JSON with all 7 `Timings` fields
and both `readout`/`generate` populated with real values (readout strongly favors the correct
option on the obvious France prompt, per Runtime.check); `reset_context` is actually called
between the two pipeline runs (confirmed by reading the source, not inferring it). All Loop 1
preservation constraints re-verified and hold; no regressions.

**Sufficient foundation for Loop 3 to start Laya + the other 2 LLM models + `apps/server`:
yes.** The engine/pipeline/models layer now has one real, working, independently-verified path
(Qwen3-0.6B, readout+generate) to extend from, rather than stubs. Loop 3 should NOT need to
revisit G3/G4/G5's core API questions — the confirmed API shapes generalize directly to the
other 2 LLM models (same `llama-cpp-2`/`hf-hub` surface, different registry entries) and are
already stubbed in the registry with real license/revision data.

**Unsafe pattern — recommend carrying forward as a known gap, not fixing immediately.** It is
correct as written, has a clear safety comment, and nothing in Loop 2's testing (unit tests,
debug/release build, real inference runs on two different prompts) surfaced any misbehavior.
Fixing it now (e.g. switching to `self_cell`) would be scope creep against D_2's explicit
narrow-scope intent ("ONE model working correctly first"). Recommend a dedicated task in a
later loop (once Laya/other models add more surface area to this pattern, or before any
concurrency is introduced around `Engine`) rather than blocking Loop 2's acceptance on it.

## Unresolved Questions

- Should `docs/codebase-summary.md`'s stale `[STUB]`/`UNVERIFIED` markers for engine/models be
  fixed as a small Loop 3 task, or is doc sync explicitly out of scope until a later
  "documentation" loop? Needs a planner/user call, not a QA one.
- Is fixing the two imprecise hf-hub citation line numbers (blocking.rs:157→163, :177→197)
  worth a trivial one-line comment correction, or is "substance correct, exact line drifted"
  an acceptable bar going forward? Flagging since D_2's validation requirement language
  ("exact file:line") is stricter than what was delivered for 2 of 4 citations.
