# Generation pipeline tuning — Loop 19 (Qwen3-0.6B regression fix)

Real measurements taken directly on `103.146.166.46` (Xeon Gold 5320, no AMX-INT8), the actual
production box, via `/tmp/loop19-target` (isolated `CARGO_TARGET_DIR`, never shared with any
other loop's build).

## 1. Objective and root cause (Task 1)

Loop 18 shipped a `<think></think>` force-close fix (`crates/pipeline/src/generate.rs`,
`run_generate`) that fixed JSON validity for all 3 models (6-7/10 → 10/10) and net-improved
correctness for Qwen3-4B/MiniCPM5-2B, but regressed Qwen3-0.6B's correct-answer count 5/9 → 3/9.

**Root cause confirmed, isolated from MAX_TOKENS/scenario noise**: an ablation run (Qwen3-0.6B,
9 scored scenarios, CLI, `--skip-laya`) directly comparing "natural CoT, `MAX_TOKENS=768`" against
the Loop 18 shipped state reproduces the regression and its reversal deterministically:

| condition | valid JSON | correct (of 9) | mean `generation_ms` |
|---|---|---|---|
| Loop 18 shipped (`<think>` force-closed) | 10/10 | 3/9 | ~632 ms (documented, §11 of quantization-sweep-results.md) |
| Natural CoT, `MAX_TOKENS=768` (this loop) | 7/10 | **5/9** | 7,133 ms |

Reverting the force-close (with no other change) recovers correctness to 5/9 — exactly this
model's documented pre-Loop-18 baseline — confirming the force-close, not the cap raise or
scenario-specific noise, is the cause. The 3 invalid-JSON scenarios under natural CoT
(`2_jailbreak_detection`, `4_agent_tool_routing`, `8_adversarial_ambiguity`) all hit the full
768-token cap without ever emitting `{"answer": ...}` — the model's CoT ran long, or in some
cases skipped `<think>` tags and rambled in plain prose past the budget.

## 2. GBNF grammar-constrained decoding: prototyped, found to CRASH the process, REJECTED (Task 2)

**Not already wired in** — confirmed by `grep -rn 'grammar\|gbnf\|Grammar'` across
`crates/pipeline`/`crates/engine`: zero hits before this loop. `llama-cpp-2 = "0.1"` (resolves to
`0.1.156`) does expose `LlamaSampler::grammar` / `grammar_lazy` / `grammar_lazy_patterns`
(`common` feature, on by default), so it was prototyped as the research doc suggested: a fixed
GBNF grammar (`{"answer": "<json-string>"}` shape) applied only to the JSON-answer tail of
generation, leaving Qwen3-0.6B's `<think>` reasoning fully unconstrained.

**Result: reproducible process aborts, not a usable fix.** Two distinct real bugs were found and
fixed in the prototype before a third, unresolved one forced abandoning the approach entirely:

1. `LlamaSampler::grammar_lazy` (trigger words matched *inside* the C++ sampler over the whole
   unconstrained span) aborted the process with `fatal runtime error: Rust cannot catch foreign
   exceptions` after scanning ~150-250+ tokens of free CoT without a match. Worked around by
   moving trigger detection into plain Rust (a substring check, like `strip_think` already does)
   and only constructing a non-lazy `LlamaSampler::grammar` once the trigger is actually found.
2. That rewrite still crashed immediately, `GGML_ASSERT(logits != nullptr)`
   (`llama-sampler.cpp:940`) — a genuine bug in the new code: `LlamaSampler::sample(&ctx, idx)`
   was called with `idx=0` right after `decode_prompt`, but `decode_prompt` only requests logits
   for the *last* submitted token, whose valid output slot is `tokens.len()-1`, not 0. Fixed by
   tracking the correct slot index across the loop (mirrors `decode_prompt`'s own documented
   convention).
3. **After both fixes, a third crash remained and is unresolved**:
   `GGML_ASSERT(!stacks.empty())` (`llama-grammar.cpp:942`), reproduced on the very first
   grammar-constrained sample of BOTH the deferred-trigger case (Qwen3-0.6B) and the
   immediate-engage case (Qwen3-4B, `trigger: None`, i.e. the simplest possible usage). Adding
   leading-whitespace tolerance to the grammar root (`root ::= ws "{" ws ...`) did not resolve it.
   This points to an incompatibility between `llama_rs_sampler_init_grammar` (the `common`
   -feature C++ shim vendored in `llama-cpp-sys-2 0.1.156`) and this project's real decode-loop
   usage pattern (or this specific llama.cpp fork build — its log lines reference "DeepSeek V4 HC"
   fused ops, suggesting a recent/exotic fork, not mainline llama.cpp) that could not be
   root-caused further without patching the vendored C++ source — explicitly out of scope this
   loop. The crate's own test suite (`tests/grammar_without_common.rs`,
   `src/grammar/tests.rs`) only exercises grammar *parsing*, never a real sample/accept decode
   loop, so this failure mode was untested territory in the dependency itself.

**Safety implication, not just a functional one**: these are process-level `abort()`s (SIGABRT),
not catchable Rust panics — G13's per-worker `catch_unwind` supervisor (`apps/server/src/
pool.rs`) does NOT protect against this; an abort takes down the entire `openjev-server` process
(all 4 workers), not just one. This is an additional, independent reason grammar-constrained
decoding must not ship as-is, beyond the functional crash itself.

**Decision: grammar code fully removed from the shipped tree** (not left disabled/dead —
`crates/engine::generate_with_deferred_grammar`, `EngineError::Grammar`, and the grammar
constants in `crates/pipeline/src/generate.rs` were deleted; `cargo build --workspace` is clean
with zero warnings about unused grammar code). If a future loop wants to revisit this, start from
reproducing bug 3 above with `LLAMA_TEST_VOCAB_GGUF` against `tests/grammar_without_common.rs`
extended to a real generation loop, in isolation from this project's own code, before touching
`crates/pipeline` again — and consider whether pinning a different `llama-cpp-2`/`llama.cpp`
build fixes it.

## 3. Final approach chosen (Task 3): pure per-model policy, no grammar

`models::ModelEntry` gains a `suppress_think: bool` field:

| model | `suppress_think` | rationale |
|---|---|---|
| `qwen3-4b` | `true` | Loop 18's proven net win (10/10 valid, 6/9 correct) — unchanged |
| `minicpm5-2b` | `true` | Loop 18's proven net win (10/10 valid, 5/9 correct) — unchanged |
| `qwen3-0.6b` | `false` | Loop 18's regression — reverted to natural CoT this loop |
| `laya` | `false` | unused (dead registry path, per Loop 18's own finding) |

`crates/pipeline/src/generate.rs::run_generate` takes `suppress_think: bool` (plumbed from the
registry by every caller: `apps/server/src/app.rs`, `apps/cli/src/main.rs`,
`apps/server/src/bin/topology_probe.rs`) and only seeds the `<think>\n\n</think>\n\n` prompt
suffix when `true`; `false` leaves the chat-templated prompt untouched and the model reasons
naturally, same generation loop otherwise (plain greedy argmax, no grammar).

## 4. Real before/after numbers, all 3 models, 10 scenarios (Task 4)

`--skip-laya`, CLI, isolated `/tmp/loop19-target` release build, one request per scenario,
`nice -n 10`. Re-verified again post-deploy via live `/bench` (both internal and external curl).

| model | valid JSON (Loop 18 → Loop 19) | correct/9 (Loop 18 → Loop 19) | mean `generation_ms` (Loop 18 → Loop 19) |
|---|---|---|---|
| Qwen3-4B (Q5_K_M) | 10/10 → **10/10** (unchanged) | 6/9 → **6/9** (unchanged) | ~4,379 ms (server) → 1,394 ms (CLI, consistent with Loop 18's own CLI number) |
| MiniCPM5-2B (Q4_K_M) | 10/10 → **10/10** (unchanged) | 5/9 → **5/9** (unchanged) | ~1,415 ms → 433 ms (CLI) |
| Qwen3-0.6B (Q8_0) | 10/10 → **7/10** (honest regression, matches this model's own pre-Loop-18 baseline) | 3/9 → **5/9** (recovered, matches pre-Loop-18 baseline) | 632 ms → **7,133 ms** (natural CoT is genuinely slow — ~11x) |

Qwen3-4B/MiniCPM5-2B's per-scenario answers are byte-identical to Loop 18's documented set
(including the same 2 known flips vs an even-earlier baseline, see §6) — zero regression,
confirmed both via isolated CLI and live `/bench` (internal + external curl).

**Honest summary**: a full win on all 3 axes (10/10 valid JSON AND 5/9+ correctness for
Qwen3-0.6B simultaneously) was **not achievable** without grammar-constrained decoding, which
crashes (§2). The shipped trade-off recovers correctness fully (matches this model's own best
historical result) at the cost of returning JSON validity to that same historical level (7/10,
not a new low — it's exactly where this model sat before Loop 18 touched it) and a real ~11x
generation-latency increase for this one model (natural CoT is slow; the other two models are
unaffected and got significantly FASTER in this same measurement pass, likely session-load
variance vs Loop 18's own numbers, not a code change on their path).

## 5. Deployment (Task 5)

- Backups (before swap): `/root/backups/loop19/openjev-server.pre-loop19.bin` (md5
  `146b79a25c993a9a8e783e489cc68df5` — confirmed identical to Loop 18's own shipped-binary
  md5, i.e. production was untouched between loops) and `openjev-cli.pre-loop19.bin` (md5
  `b44f94a9c4874ab1bb837b46bb68f009`), plus an in-place rollback copy at
  `/tmp/openjev/target/release/openjev-server.rollback-loop19`. Rollback: restore either +
  `systemctl restart openjev-server`.
- Deployed (`md5`): `openjev-server` = `5ef391eab27b3f3c826f9df855996346`, `openjev-cli` =
  `7fedf60636b00476cb04a114bd1a86f4`. Deployed via atomic `mv` (the running binary can't be
  overwritten in place — `Text file busy`), then `systemctl restart openjev-server`.
- Verified: `/health` → 200 (both internal `127.0.0.1` and external `103.146.166.46` curl from
  outside the server entirely); real `/bench` calls for all 3 models via external curl, all
  valid JSON. Post-deploy tree rebuilt in `/tmp/loop19-target` and md5-matched byte-for-byte
  against the deployed binary (confirms no test-only/ablation code leaked into production).

## 6. The 2 secondary security/compliance flips (Task 6, investigated, not fixed)

Both flips are unchanged from Loop 18 (same code path, `suppress_think: true`, byte-identical
prompts) — reproduced again this loop via live `/bench`, plus their constrained-readout
distributions for context:

- **Qwen3-4B `2_jailbreak_detection`** (`injection-attempt` expected, `ambiguous` returned):
  constrained-readout is **99.99996% confident in `ambiguous`** (`probs: {benign: 4e-7,
  injection-attempt: 3e-9, ambiguous: 0.9999996}`) — i.e. this is NOT a close call, and NOT a
  generate-vs-readout disagreement: both independent methods agree with each other and disagree
  with the documented expected label. This looks like a genuine model calibration/judgment
  question (does "reveal the system prompt" read as an injection attempt or an ambiguous
  request?), orthogonal to the CoT-suppression policy — grammar-constraining the JSON step
  (even if it worked) would not have changed this, since the underlying probability mass was
  never close.
- **MiniCPM5-2B `9_compliance_gating`** (`yes` expected, `no` returned): constrained-readout
  **agrees with the expected answer, fairly confidently** (`probs: {yes: 0.9098, no: 0.0902}`)
  — this IS a real generate-vs-readout disagreement, and generate's answer diverges from what
  the model's own probability distribution favors. This is the more interesting open gap: a
  future loop could investigate whether generate systematically under-weights the model's true
  preference once `<think>` is force-closed (no deliberation budget to "reconsider"), and
  whether preferring readout's answer when it strongly disagrees with generate (a larger
  architectural change, out of scope here) would help. Not fixable by grammar (rejected, §2) or
  by this loop's per-model-policy change (MiniCPM5-2B's policy is unchanged).

## 7. Full regression (Task 7)

- `cargo build --workspace` (debug + release) and `cargo test --workspace`: clean, 0 failures,
  isolated `/tmp/loop19-target`, run against the exact deployed tree (md5-confirmed, §5).
- 3 models × 3 methods (readout/generate/laya): all pass via live `/bench`, internal + external
  curl.
- **G13 fault injection**: drilled for real on an isolated scratch copy+binary
  (`/tmp/g13-drill`/`/tmp/g13-drill-target`, port 8091, never the live artifact) — a
  test-only panic trigger gated on a magic prompt string was added, built, exercised (clean
  HTTP 500 in 1.3ms, log line `engine worker 0 panicked, respawning with an empty model cache
  (other workers unaffected)`), confirmed self-heal on the next request (fresh
  `model_load_ms`, 200 OK), then the entire scratch copy was deleted — the change never
  touched `/tmp/openjev`.
- **Laya-outage graceful degradation: LIVE-DRILLED for real this loop** (the gap Loop 18 left
  open) — `systemctl stop openjev-laya-serve`, confirmed `/bench` still returns 200 with valid
  readout/generate and `"laya": null`, then `systemctl start openjev-laya-serve` and confirmed
  full recovery (`laya` field back to real scores) within 3 seconds. No permission block this
  time; the drill was short and announced (a single stop/start cycle, not an unbounded kill).
- G17/G18 firewall: unaffected — `iptables -L INPUT -v` still shows the `lo`-only ACCEPT +
  interface-`*` DROP pair for port 8090; external curl to `103.146.166.46:8090` from outside
  the server times out as expected.

## 8. Open gaps for a future loop

1. Qwen3-0.6B's generation latency under natural CoT (~7.1s mean, up to the full 768-token cap
   on 3/10 scenarios) is a real, un-mitigated cost of this fix — no attempt was made to bound
   it further this loop (e.g. a partial-CoT token budget before forcing an answer) since that
   would reintroduce exactly the truncation failure mode Loop 18 was fixing.
2. Grammar-constrained decoding remains a real, worthwhile idea if the underlying crash (§2,
   item 3) can be root-caused — would recover Qwen3-0.6B's JSON validity without the
   correctness trade-off, if it worked. Needs a `llama-cpp-2`/`llama.cpp` version investigation
   or a minimal standalone repro against the vendored crate's own test harness before touching
   this project's code again.
3. MiniCPM5-2B's `9_compliance_gating` flip (§6) is a genuine generate-vs-readout disagreement
   worth a closer architectural look (not just a generation-policy tweak).
4. Qwen3-4B's `2_jailbreak_detection` "flip" may not be a bug at all — both independent scoring
   methods agree and disagree with the documented expected label; worth revisiting whether the
   expected answer itself is right.

---

## Loop 20 — bounded/partial-CoT token-budget forcing for Qwen3-0.6B

Real measurements taken directly on `103.146.166.46` (Xeon Gold 5320, no AMX-INT8), the actual
production box, via `/tmp/loop20-target` (isolated `CARGO_TARGET_DIR`, never shared with any
other loop's build).

### 1. Objective and technique choice (Task 1)

Loop 19 left Qwen3-0.6B on a binary policy: full `<think>` suppression (Loop 18: 10/10 valid
JSON, 3/9 correct, 632ms) vs. natural unbounded CoT (Loop 19: 7/10 valid, 5/9 correct, 7,133ms,
~11x slower). Loop 20's objective: try a bounded/partial-CoT token budget as a middle ground.

**Technique chosen: token-budget forcing** (not NOWAIT-style logit-bias suppression). The real
`llama-cpp-2 0.1.156` sampler source (`sampling.rs`) was checked directly: `LlamaSampler::
logit_bias(n_vocab, biases)` genuinely exists and is a safe, pure additive-logit operation (not
the fragile C++ grammar engine that SIGABRT'd in Loop 19's GBNF attempt) — so NOWAIT was
technically viable this loop, unlike grammar-constraining. It was not implemented because:
(a) Task 3's calibration requirement ("try 2-3 budget values, measure the tradeoff") is built
around a hard token ceiling, which budget-forcing gives directly and NOWAIT does not (NOWAIT
only gives a probabilistic 27-51% CoT reduction, no guaranteed ceiling); (b) NOWAIT requires
correctly identifying Qwen3-tokenizer filler-token ids ("Wait", "Hmm", "Let me think again" —
multi-token, context-dependent) — unvalidated extra research surface within this loop's budget;
(c) trialling both techniques would double the sweep's compute load on the shared production
box, against this project's own repeated documented CPU-overload lesson (Loops 13-15).
**Flagged as a real follow-up** if a future loop wants to push further than budget-forcing's
ceiling allows.

### 2. Implementation (Task 2)

- `crates/models/src/download.rs`: `ModelEntry` gains `think_budget: Option<usize>` (a registry
  field, not a hardcoded constant, per the dev-doc's own suggestion — future tuning needs no
  code change to `generate.rs`). `None` for `qwen3-4b`/`minicpm5-2b`/`laya` (their
  `suppress_think: true` already closes `<think>` at the prompt, so the field is inert there
  regardless of its value — set to `None` for clarity, not correctness). `qwen3-0.6b` carries
  the calibrated value (see §3).
- `crates/pipeline/src/generate.rs::run_generate`: gains a `think_budget: Option<usize>`
  parameter and two new pieces of state, `think_opened`/`think_closed` (substring-tracked over
  the growing generated text). When `think_budget` is `Some(n)` and the model has opened
  `<think>` but not closed it after `n` generated tokens, the literal `</think>\n\n` (same text
  Loop 18 used to suppress thinking entirely) is force-injected as real decoded/accepted
  tokens via `tokenize_no_bos` + `decode_next_id` — not just appended to the output string —
  then normal greedy sampling resumes for the answer. This never fires if `suppress_think` is
  `true` (nothing open to force-close) or if the model never opened `<think>` at all (a model
  that skips straight to prose is left alone). Added `GenerateResult::think_budget_forced: bool`
  for sweep diagnostics. Plumbed through all 3 call sites (`apps/server/src/app.rs`,
  `apps/cli/src/main.rs`, `apps/server/src/bin/topology_probe.rs`).
- Sanity-checked with `think_budget=None`: reproduces Loop 19's exact behavior byte-for-byte
  (same failure mode on `2_jailbreak_detection`, `think_budget_forced: false`) — confirms the
  new code path is a safe no-op when unset.

### 3. Budget-sweep calibration (Task 3)

10 project scenarios, `openjev-cli --skip-laya`, one request per scenario, `nice -n 10`,
1s gap between requests. Correctness scored against the documented expected-answer key
(`docs/benchmarks/quant-sweep-loop16/harness-scen.json`, 9/10 scenarios have a documented
expected answer; `8_adversarial_ambiguity` is intentionally unscored).

A real bug was found and fixed in the calibration harness itself before trusting any numbers:
an early jq filter silently dropped scenario 8's entire result record (an empty-string jq value
made the whole object-construction pipeline emit zero outputs) — caught by `n` reading 9 instead
of 10, fixed, and the `None` baseline was re-run from scratch for consistency with the other
sweep points.

| `think_budget` | valid JSON /10 | correct /9 | mean `generation_ms` | forced-close count |
|---|---|---|---|---|
| Loop 18 (reference, prior loop) | 10/10 | 3/9 | 632 | n/a |
| Loop 19 (reference, prior loop) | 7/10 | 5/9 | 7,133 | n/a |
| `None` (this loop, fresh repro on new binary) | 7/10 | 5/9 | 7,007.7 | 0/10 |
| **`150`** | **10/10** | **6/9** | **2,486** | 10/10 (all forced) |
| `300` | 10/10 | 6/9 | 4,143.9 | 5/10 |
| `450` | 10/10 | 6/9 | 5,013.2 | 3/10 |

The `None` re-run (7,007.7ms) closely reproduces Loop 19's own number (7,133ms) on a fresh
build — cross-loop consistency check, confirms the new code path doesn't silently change
behavior when unconfigured.

**150 dominates 300 and 450**: identical valid-JSON (10/10) and correct-count (6/9), but
noticeably faster (2,486ms vs 4,144ms/5,013ms) — the extra budget headroom in 300/450 bought
zero additional correctness on this 10-scenario suite, only extra latency from the (rarer)
naturally-closing scenarios running longer before landing on an answer. All three budget values
tested comfortably beat Loop 19's natural-CoT baseline on every axis simultaneously.

### 4. Validation against both reference points (Task 4)

**`think_budget=150` vs Loop 19** (7/10 valid, 5/9 correct, 7,133ms): better on **all 3 axes**
— more valid JSON (10 vs 7), more correct (6 vs 5), ~2.9x faster (2,486ms vs 7,133ms). Not a
marginal win.

**`think_budget=150` vs Loop 18** (10/10 valid, 3/9 correct, 632ms): ties on valid-JSON (10/10),
doubles correctness (6/9 vs 3/9), costs ~3.9x more latency (2,486ms vs 632ms) — but both remain
sub-3-second, and 150 is still ~2.9x *faster* than the only other config (natural CoT) that gets
close to its correctness level. Not "drastically worse" on the third axis in absolute terms.

**Success criterion met**: a real, measured configuration that beats both extremes on multiple
axes without being drastically worse on the remaining one exists — `think_budget=150`. This is
not a forced/cherry-picked result: it was the front-runner from the first sweep point onward
and held its lead cleanly against both higher-budget alternatives.

### 5. Deployment (Task 5)

Deployed `think_budget=150` for `qwen3-0.6b` (`crates/models/src/download.rs`'s
`THINK_BUDGET_QWEN3_0_6B` constant).

- **Pre-deploy backup**: `/root/backups/loop20/openjev-server.pre-loop20.bin` (md5
  `5ef391eab27b3f3c826f9df855996346`) — **exact match to Loop 19's own documented deployed
  binary md5**, confirming production was genuinely untouched between Loop 19 and Loop 20, not
  just claimed. `/root/backups/loop20/openjev-cli.pre-loop20.bin` (md5
  `7fedf60636b00476cb04a114bd1a86f4`, also matches Loop 19's cli md5). Rollback: `cp
  /root/backups/loop20/openjev-server.pre-loop20.bin /tmp/openjev/target/release/openjev-server
  && systemctl restart openjev-server` (same for the CLI binary, no restart needed).
- **Deployed** (atomic `mv`, same pattern as Loop 18/19 — the running binary can't be
  overwritten in place): `openjev-server` md5 `9f4b8d0af949cc851b8cf6c131d3767d`, `openjev-cli`
  md5 `c2b712a56985fb9f7c6882ba7f0b7e3a`. Both built from `/tmp/loop20-target`, deployed into
  the live `/tmp/openjev/target/release/` path the systemd unit actually runs
  (`ExecStart=/tmp/openjev/target/release/openjev-server --port 80 --workers 4 --threads 7`,
  confirmed via `systemctl show`, unaffected by the binary swap).
- **Verified**: `/health` → 200 post-restart; real external `/bench` call for `qwen3-0.6b`
  returned valid JSON (`"answer":"ambiguous"`, `think_budget_forced: true`, 158 tokens
  generated) confirming the deployed binary's new code path actually fires in production, not
  just in the isolated build.

### 6. Full regression (Task 6)

- **`cargo build --workspace`** (debug + release) and **`cargo test --workspace`**: clean, 0
  failures (8 real tests across `laya-native` and other crates pass), isolated
  `/tmp/loop20-target`, run against the exact tree deployed.
- **3 models × 3 methods** (readout/generate/laya): all pass via live `/bench`
  (`scripts/bench-harness.sh loop20-post-deploy`, 30 requests, 0 errors). Qwen3-4B and
  MiniCPM5-2B spot-checked unaffected — `qwen3-4b`'s `2_jailbreak_detection` still returns
  `"ambiguous"`, byte-identical to Loop 18/19 (expected: their `suppress_think: true` path and
  `think_budget: None` mean the new budget-forcing branch never fires for them).
- **G13 fault injection**: drilled for real on an isolated scratch copy+binary
  (`/tmp/g13-drill`/`/tmp/g13-drill-target`, port 8091, never the live artifact) — a
  test-only panic trigger gated on a magic prompt string, built, exercised (clean HTTP 500,
  `"engine worker 0 dropped the response"`), confirmed self-heal on the immediate next request
  (fresh `model_load_ms: 876`, 200 OK, valid answer), then the entire scratch copy deleted.
- **Laya-outage graceful degradation: LIVE-DRILLED** — `systemctl stop openjev-laya-serve`,
  confirmed `/bench` still returns 200 with valid readout/generate and `"laya": null`, then
  `systemctl start openjev-laya-serve` and confirmed full recovery (`laya` field back to real
  scores) within 1 second.
- **G17/G18 firewall**: external curl (from outside the server entirely) to
  `103.146.166.46:8090` timed out as expected (`curl` exit code 28, connection timeout) — the
  `lo`-only ACCEPT + interface-`*` DROP pair is unaffected by this loop's changes.

### 7. Open gaps for a future loop

1. NOWAIT-style logit-bias filler suppression was confirmed technically viable
   (`LlamaSampler::logit_bias` exists, safe, distinct from the grammar-engine crash) but not
   implemented or compared against budget-forcing — a real follow-up if `think_budget=150`'s
   ~2,486ms mean is ever judged still too slow, since NOWAIT could in principle reduce CoT
   length within the natural-closing path rather than hard-cutting it.
2. The budget sweep only tested 150/300/450; the true optimum could lie below 150 (untested) —
   150 was the lowest value tried and already dominated the higher ones, so a finer sweep
   (e.g. 75, 100, 125) is a plausible cheap follow-up if further latency reduction is wanted,
   though the marginal value looks small given 300/450 bought zero extra correctness over 150.
3. Grammar-constrained decoding (Loop 19, closed avenue) and the two open security/compliance
   flips (MiniCPM5-2B `9_compliance_gating`, Qwen3-4B `2_jailbreak_detection`) remain unchanged
   open items from Loop 19 — out of this loop's scope per the dev-doc, not re-investigated.
