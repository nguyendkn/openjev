---
loop: 21
status: pending
preservation_constraints:
  - "openjev-server and laya serve stay live and correctly serving throughout; deploy uses backup+rollback discipline (Loop 12/18/19/20 precedent)"
  - "Do NOT change any of Loop 18/19/20's model-policy decisions (suppress_think, think_budget, quant choices) — this loop is about request-handling architecture and engine build flags, not generation policy"
  - "The existing /bench request/response SHAPE must stay backward compatible: default behavior (no new fields set) must be byte-identical to today — any method-selection feature is strictly additive/opt-in"
  - "Do NOT reattempt GBNF grammar-constrained decoding (Loop 19 proved it crashes the server)"
  - "G13, Laya graceful degrade, G17/G18 firewall all still functional"
  - "cargo build --workspace (debug+release) and cargo test --workspace still pass"
  - "Use a dedicated CARGO_TARGET_DIR, separate from any other concurrent work"
---

## Objective
The user reports real-world response time still feels slow. Investigate root cause with
real production evidence first, then fix the highest-leverage, lowest-risk items found.
Two concrete hypotheses to check (both already partially confirmed by the Runtime via
direct code read — verify with real data, then act):

1. `/bench`'s handler (`apps/server/src/app.rs::bench_impl` or equivalent) runs all 3
   methods (readout, generate, laya) **strictly sequentially** in every request, and has
   NO way to request only a subset — `skip_laya` exists but there's no `skip_readout` /
   `skip_generate` equivalent. A caller who only needs one fast answer (e.g. just
   `laya` at ~111ms, or `readout` at ~379ms) is forced to also pay `generate`'s
   ~1.4-4.4s tax sequentially, inflating perceived latency by ~4-10x for no reason if
   they don't actually need all 3 methods compared.
2. The `laya` HTTP call (`run_laya`, a call to the separately-running `laya serve`
   process) has ZERO data dependency on `readout`/`generate`'s results, yet runs
   strictly AFTER them today. It could run concurrently with the LLM pipeline work,
   saving its ~100-150ms from the critical path for free.

## Context
- Live real numbers (Runtime's own spot-check, 2026-09-23, `1_email_routing` scenario,
  qwen3-0.6b): `constrained_readout_ms=379`, `generation_ms=3387`, `laya_inference_ms=111`
  — sums to the full request latency, confirming sequential execution.
- `docs/research/cpu-inference-optimization-2026.md` has 2 engine-level levers not yet
  tried: llama.cpp's online-repack AVX-512 flag (`-rtrp` / equivalent runtime flag, zero
  code change) and the `ik_llama.cpp` fork's Cascade-Lake-tuned kernels (+21-25% pp512
  reported, unverified on this project's actual Ice Lake hardware, real maintenance-risk
  tradeoff — treat as a bigger, separate decision, not a quick win).
- Full history: `plans/20260922-2328-hoh-openjev-rs/run.md` tail (Loops 18-20 + the new
  Jev-comparison section).
- Perf logging infra already exists: `crates/timing/src/logger.rs` (`PerfSpan`,
  `perf_span!` macro, minute-rotating JSONL under `logs/perf/` on the server) and
  `scripts/analyze-perf-logs.sh` — use these for real traffic analysis, don't just rely
  on synthetic single-call spot-checks.

## Tasks
1. **Analyze real production perf logs** (`logs/perf/` on the server, via
   `scripts/analyze-perf-logs.sh` or direct `jq` if the script doesn't cover this) to
   confirm the sequential-execution hypothesis holds across REAL traffic (not just one
   synthetic call), and get a real distribution (not just one sample) of
   `constrained_readout_ms`/`generation_ms`/`laya_inference_ms` across all 3 models over
   whatever real window of traffic exists.
2. **Add opt-in method selection to `/bench`**: extend `BenchRequest` with a way to
   request a subset of methods (e.g. `methods: Option<Vec<String>>` accepting any of
   `"readout"`/`"generate"`/`"laya"`, defaulting to all 3 when omitted — MUST be
   backward-compatible, omitted = today's exact behavior). Skip the engine work for
   methods not requested (not just hide them in the response — actually skip the
   decode calls to save real time). This directly lets a latency-sensitive caller get a
   ~111-500ms response instead of several seconds, without any accuracy/model change.
   Update `apps/cli` similarly if reasonable (already has `--skip-laya`, consider
   `--skip-readout`/`--skip-generate` for symmetry) — keep small, don't over-engineer.
3. **Parallelize the Laya HTTP call with the LLM pipeline** where the request wants both
   (e.g. spawn `run_laya` on a separate thread/task at the START of the handler,
   `.join()`/await it after `generate` finishes instead of calling it after). Real
   savings = Laya's ~100-150ms shaved off requests that want all 3 methods. Verify this
   doesn't break G13's per-worker model (the LLM work still happens on the worker's
   dedicated `!Send` actor thread; the Laya call is a separate stateless HTTP client, so
   this should be safe to run on a spawned task — check the actual pool/actor code
   before assuming).
4. **Evaluate the `-rtrp`/online-repack llama.cpp flag** (or its real current equivalent
   — check the actual crate version's docs/changelog since the research doc's
   information may already be slightly stale) as a build-time or runtime flag for the
   LLM engine specifically targeting `generation_ms` (the dominant remaining cost).
   Test on an isolated build against the 10 benchmark scenarios + `llama-bench`-style
   raw tok/s — deploy ONLY if it's a real, validated win with no accuracy regression.
   Report honestly if it doesn't help or isn't applicable to this llama-cpp-2 version.
5. **Do NOT attempt the `ik_llama.cpp` fork swap this loop** — flag it as a bigger,
   separate decision (different upstream, maintenance burden) for the user to explicitly
   authorize if the simpler wins above aren't enough. Just note its potential in the
   report.
6. **Deploy whatever passes validation** (Tasks 2-4) with backup/rollback discipline.
7. **Full regression**: 3 models × 3 methods (with AND without the new method-selection
   feature), G13, Laya-outage live drill, G17/G18 firewall, `cargo test --workspace`.
8. **Update `docs/benchmarks/jev-comparison.md` or a new
   `docs/benchmarks/request-latency-tuning.md`** with real before/after numbers for a
   "laya-only" and "readout-only" fast-path call now that it's possible, plus the
   parallelization savings for a full 3-method call.

## Preservation
See frontmatter.

## Validation Requirements
- Given the real perf-log analysis, Then it either confirms or corrects the sequential-
  execution hypothesis with real traffic data, not just the one synthetic sample already
  taken.
- Given the method-selection feature, When a request omits `methods` entirely, Then the
  response is byte-identical in shape/content to today's behavior (regression-tested,
  not assumed).
- Given a `methods: ["laya"]` request, Then the response returns in ~100-200ms (real
  measured, not projected) with `readout`/`generate` fields appropriately absent/null and
  their engine work actually skipped (not just hidden).
- Given the Laya-parallelization change, Then a full-3-method request is measurably
  faster by roughly Laya's own inference time, with no G13/degradation regression.
- Given the `-rtrp`/online-repack evaluation, Then a real before/after `generation_ms`
  and tok/s number is reported, deployed only if genuinely faster with no accuracy loss.

## Out-of-scope
- `ik_llama.cpp` fork adoption — flagged for a future, explicitly-authorized loop only.
- Any change to Loop 18/19/20's suppress_think/think_budget/quant decisions.
- GBNF grammar-constrained decoding — closed, do not reattempt.
- NOWAIT logit-bias CoT suppression — separate, lower-priority research item, not this
  loop's job unless Task 4 leaves significant spare time.
