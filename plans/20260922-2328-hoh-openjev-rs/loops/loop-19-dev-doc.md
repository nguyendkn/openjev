---
loop: 19
status: pending
preservation_constraints:
  - "openjev-server and laya serve stay live and correctly serving throughout; any deploy uses backup+rollback discipline (Loop 12/18 precedent)"
  - "Do NOT regress Qwen3-4B or MiniCPM5-2B's Loop 18 wins (10/10 valid JSON, improved net correct-count) while fixing Qwen3-0.6B"
  - "G13, Laya graceful degrade, G17/G18 firewall all still functional"
  - "cargo build --workspace (debug+release) and cargo test --workspace still pass"
  - "Use a dedicated CARGO_TARGET_DIR, separate from any other concurrent work"
---

## Objective
Loop 18 shipped a chain-of-thought suppression fix (`<think></think>` force-close in
`crates/pipeline/src/generate.rs`) that fixed JSON validity (6-7/10 -> 10/10) and net
improved correctness for Qwen3-4B and MiniCPM5-2B, but caused a REAL regression on
Qwen3-0.6B: correct-answer count dropped 5/9 -> 3/9 (the smallest model appears to lean
on visible CoT for correctness more than the larger two). This shipped anyway as an
accepted trade-off but is an open gap. Fix it without regressing the other two models'
wins.

## Context
- `docs/research/cpu-inference-optimization-2026.md` (a parallel research pass, same
  run) flagged **GBNF grammar-constrained decoding** for the JSON output step as a
  "near-free win, already built into llama.cpp" — check if it's already in use in
  `crates/pipeline`/`crates/engine`. If not, this may be a BETTER fix than blanket CoT
  suppression: grammar-constrain the JSON structure so output is always valid, WITHOUT
  forcing `<think></think>` empty (i.e. let Qwen3-0.6B keep its reasoning, which Loop 18
  found it needs for correctness, while still guaranteeing valid JSON at the end).
- Also flagged in that research: Qwen3's native `/no_think` control and "NOWAIT"
  (partial CoT suppression via logit-bias filler suppression, ~27-51% CoT reduction
  rather than 100%) — a middle ground between "full CoT" (accurate but slow, pre-Loop-18
  state) and "zero CoT" (fast but Qwen3-0.6B-inaccurate, current shipped state).
- Loop 18's full findings + the exact code change: `plans/20260922-2328-hoh-openjev-rs/
  run.md` (tail, Loop 18 entry) and `crates/pipeline/src/generate.rs` (current shipped
  state, git history has the Loop 18 diff).
- 2 additional flipped answers on the LARGER models (Qwen3-4B `2_jailbreak_detection`,
  MiniCPM5-2B `9_compliance_gating`) are lower-priority secondary findings — investigate
  if time permits after Qwen3-0.6B is fixed, since Loop 16 already flagged this
  scenario class (security/compliance) as sensitive.

## Tasks
1. **Root-cause the Qwen3-0.6B regression precisely**: confirm (don't assume) that the
   `<think></think>` force-close is really the cause (vs. e.g. the raised MAX_TOKENS=768
   interacting badly, or scenario-specific noise) — re-run the specific flipped
   scenarios with CoT force-closed vs. natural CoT, isolate the variable.
2. **Check whether GBNF grammar-constrained JSON decoding is already wired in**
   (`crates/pipeline`/`crates/engine`, search for "grammar"/"gbnf"/`LlamaGrammar`
   equivalents in `llama-cpp-2`). If not, prototype it for the `generate` pipeline's
   JSON output step and test whether it alone (WITHOUT forcing `<think></think>`
   closed) fixes JSON validity for Qwen3-0.6B while preserving its natural-CoT accuracy.
3. **If grammar-constraining alone isn't sufficient or isn't practical in the time
   budget**, try a per-model policy: apply the Loop 18 CoT-suppression fix only to
   Qwen3-4B and MiniCPM5-2B (where it's a proven net win), leave Qwen3-0.6B on natural
   CoT + grammar-constrained JSON (or a bounded partial-CoT budget if natural CoT alone
   still risks the original JSON-truncation problem for this model). `crates/models`
   already has a per-model registry — extending it with a per-model generation policy
   flag is a reasonable, minimal-footprint change.
4. **Re-validate all 3 models across the 10 benchmark scenarios**: valid-JSON rate AND
   correct-answer count must not regress from Loop 18's post-fix numbers for Qwen3-4B/
   MiniCPM5-2B, and Qwen3-0.6B's correct-answer count must recover toward its pre-Loop-18
   baseline (5/9) without losing Loop 18's JSON-validity win (10/10) if at all possible —
   report honestly if a full win on all 3 axes isn't achievable and explain the
   remaining trade-off.
5. **Deploy the fix** with backup/rollback discipline (Loop 12/18 precedent) if it's a
   net improvement over the current shipped state, verified via real curl.
6. **Time-permitting**: investigate the 2 flipped security/compliance answers on the
   larger models — are they also fixable by grammar-constraining instead of full CoT
   suppression? Report findings even if not fully resolved this loop.
7. **Full regression**: 3 models × 3 methods, G13, Laya degrade (try again for a real
   live drill this time if the permission block from Loop 18 can be worked around
   safely — e.g. a short, announced maintenance-window stop/restart of `laya serve`
   rather than an unbounded kill; if still blocked, note it honestly again), G17/G18
   firewall, `cargo test --workspace`.
8. **Update `docs/benchmarks/quantization-sweep-results.md` or a new
   `docs/benchmarks/generation-pipeline-tuning.md`** documenting the final fix and
   per-model policy.

## Preservation
See frontmatter.

## Validation Requirements
- Given the fix, When the 10 benchmark scenarios are re-run for all 3 models, Then
  Qwen3-4B and MiniCPM5-2B's Loop 18 wins (10/10 valid JSON, net-improved correctness)
  are NOT regressed, and Qwen3-0.6B's correctness is measurably improved from Loop 18's
  shipped 3/9 — with valid-JSON rate reported honestly even if a full return to 10/10
  isn't achievable simultaneously with correctness recovery.
- Given any deploy, Then backup/rollback path is documented and the regression suite
  passes clean.

## Out-of-scope
- Laya F16 swap (still blocked on `crates/laya-native`).
- MiniCPM5-2B Q8_0 (already correctly rejected in Loop 18 on real hardware).
- ik_llama.cpp fork adoption or the `-rtrp` online-repack flag (separate, lower-priority
  research findings — not this loop's job).
