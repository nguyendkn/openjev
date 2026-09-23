---
loop: 20
status: pending
preservation_constraints:
  - "openjev-server and laya serve stay live and correctly serving throughout; deploy uses backup+rollback discipline (Loop 12/18/19 precedent)"
  - "Do NOT regress Qwen3-4B or MiniCPM5-2B (they stay on suppress_think=true, unaffected by this loop)"
  - "Do NOT reattempt GBNF grammar-constrained decoding — Loop 19 proved it crashes the server process (SIGABRT deep in the vendored llama-cpp-sys-2 C++ grammar engine); that avenue is closed unless a future loop specifically root-causes the upstream library bug"
  - "G13, Laya graceful degrade, G17/G18 firewall all still functional"
  - "cargo build --workspace (debug+release) and cargo test --workspace still pass"
  - "Use a dedicated CARGO_TARGET_DIR, separate from any other concurrent work"
---

## Objective
Loop 19 shipped a binary per-model policy for Qwen3-0.6B: full CoT suppression (Loop 18,
fast+valid JSON but 3/9 correct) vs. no suppression (Loop 19, back to 5/9 correct but
7/10 valid JSON and ~7.1s mean generation latency, ~11x slower than suppressed). Neither
extreme is great. Try a middle ground — a BOUNDED/PARTIAL chain-of-thought budget —
to get closer to "mostly correct, mostly valid, not 11x slower" for this one model.
Qwen3-4B and MiniCPM5-2B are untouched (they stay on the Loop 18/19 suppress_think=true
policy, already a proven net win there).

## Context
- Read `docs/research/cpu-inference-optimization-2026.md` §3 (CoT/thinking-budget
  reduction) FIRST — it documents the candidate techniques: Qwen3's native
  `/no_think`-adjacent budget controls, the "NOWAIT" technique (reported 27-51% CoT
  token reduction via logit-bias-based filler-token suppression during the thinking
  phase, inference-time only, no retraining/fine-tuning), and Chain-of-Draft prompting.
  This is the starting point — don't re-derive from scratch, but DO verify any claim
  against `llama-cpp-2`'s actual bound API before relying on it (same "read the real
  API" discipline this project has used throughout — logit-bias support, token-by-token
  callback hooks, etc. — check what's actually exposed).
- Loop 19's shipped state: `crates/models/src/download.rs` has `ModelEntry::
  suppress_think: bool` (Qwen3-0.6B currently `false` = natural/unbounded CoT).
  `crates/pipeline/src/generate.rs` has the force-close logic Loop 18 added (currently
  gated by the policy flag). Full history: `plans/20260922-2328-hoh-openjev-rs/run.md`
  tail (Loops 18-19 entries).
- The generation loop already does manual token-by-token decoding (to strip `<think>`
  blocks and validate JSON) — it's a reasonable place to add a TOKEN-COUNT-BASED forced
  close: let Qwen3-0.6B think naturally up to a budget (e.g. 150-300 tokens — pick based
  on real measurement, don't guess blindly), then inject `</think>\n\n` to force the
  transition to the answer, same mechanism Loop 18 used for the always-immediate case.
  This is a simpler, lower-risk starting point than the NOWAIT logit-bias approach if
  time is tight — implement whichever you can validate is actually better, and if you
  try both, report which won and why.

## Tasks
1. Read the research doc section + re-confirm the real `llama-cpp-2` API surface for
   whatever technique you pick (token budget forcing is simplest/lowest-risk; logit-bias
   NOWAIT-style suppression is the research-recommended approach if the bound API
   supports it cleanly — your call, but justify it).
2. Implement a bounded/partial-CoT mode for Qwen3-0.6B specifically (leave the other 2
   models' `suppress_think=true` path untouched). Consider making the budget itself a
   per-model registry field (extending `ModelEntry`) rather than hardcoding, so future
   tuning doesn't require another full loop.
3. **Calibrate the budget empirically**: try at least 2-3 different budget values
   against the 10 benchmark scenarios, measure the 3-way tradeoff (valid-JSON rate,
   correct-answer count, mean generation_ms) for each, and pick the one with the best
   real balance — report the full sweep, not just the winner, so the tradeoff curve is
   visible for a future loop to revisit.
4. **Validate against Loop 19's shipped numbers** (JSON 7/10, correct 5/9, mean
   generation_ms 7,133) as the baseline to beat, and against Loop 18's numbers (JSON
   10/10, correct 3/9, mean generation_ms 632) as the other reference point. Success =
   a real, measured point that's a genuinely better trade than both extremes on at least
   2 of the 3 axes without being drastically worse on the third — report honestly if no
   such point exists and the binary choice from Loop 19 turns out to be near-Pareto-
   optimal already (a valid, useful outcome, don't force a win).
5. **Deploy** the best-found configuration with backup/rollback discipline if it's a
   real improvement over Loop 19's shipped state. If no configuration beats Loop 19's
   trade-off meaningfully, do NOT deploy a change for its own sake — report that finding
   and leave Loop 19's binary policy in place.
6. **Full regression**: 3 models × 3 methods, G13 (isolated scratch, never the live
   artifact), Laya-outage live drill (Loop 19 already proved this is safe to do for
   real — repeat it as part of standard regression now that it's established practice),
   G17/G18 firewall, `cargo test --workspace`.
7. **Update `docs/benchmarks/generation-pipeline-tuning.md`** (Loop 19's new file) with
   a Loop 20 section: the budget sweep table, chosen value + reasoning, before/after vs
   both reference points.

## Preservation
See frontmatter.

## Validation Requirements
- Given at least 2-3 budget values tested, When compared against Loop 19's shipped
  numbers (7/10 valid, 5/9 correct, 7,133ms) and Loop 18's numbers (10/10 valid, 3/9
  correct, 632ms) for Qwen3-0.6B, Then a full honest comparison table is produced —
  real measured numbers, not projected/rounded.
- Given any deploy, Then backup/rollback path is documented and the full regression
  suite passes clean, including a live Laya-outage drill.
- Given no configuration beats Loop 19's trade-off, Then this is reported plainly as a
  valid outcome (Loop 19's binary choice stands) rather than a forced/inflated "win."

## Out-of-scope
- Qwen3-4B / MiniCPM5-2B — unaffected, do not touch their policy.
- GBNF grammar-constrained decoding — closed per frontmatter, do not reattempt.
- Root-causing the upstream `llama-cpp-sys-2` grammar-engine SIGABRT — separate,
  higher-effort investigation for a future loop if ever prioritized.
- MiniCPM5-2B's `compliance_gating` readout-vs-generate divergence, Qwen3-4B's
  `jailbreak_detection` label question — Loop 19's open items, not this loop's job.
