---
loop: 13
status: pending
preservation_constraints:
  - "openjev-server and laya serve (now the fixed -O3 binary) both stay live and unaffected — crates/laya-native remains standalone, NOT wired into production this loop"
  - "The reference case (capital of France) and all 7+ previously-validated benchmark scenarios still produce correct, matching-within-tolerance results after every change"
  - "cargo build --workspace (debug+release) and cargo test --workspace still pass"
  - "Correctness gate is non-negotiable: any speed optimization that regresses the <1e-4 match achieved in Loop 11 must be reverted, not kept 'because it's faster'"
---

## Objective
Push `crates/laya-native`'s speed below the now-fixed C++ baseline (92.7ms) without losing
the correctness Loop 11 achieved.

Current state: 95.1ms (native Rust) vs 92.7ms (`ggmlc` built correctly, `-O3`/`-march=native`)
— roughly at parity, Rust ~3% behind. Per explicit user direction, this is a baseline to beat,
not a stopping point. Use the new `crates/timing`'s perf logger (built in Loop 11b) to find
real hotspots rather than guessing.

## Candidate optimization directions (investigate, don't assume any of these will pay off —
measure each with the logger before and after)
1. **Graph node count**: Loop 11 built ~1430 nodes (grew from Loop 10's 970 while adding the
   real head/scorer). Profile which ops dominate node count and wall time — are there
   redundant `view`/`reshape`/`cont` nodes that could be fused or eliminated? `ggml` has a
   graph-optimization pass in some configurations (check if `ggmlc`/`llama.cpp` expose one you
   can reuse, e.g. via `ggml_graph_optimize` or similar if it exists in the bound API).
2. **Memory allocator**: Loop 10 used a flat `ggml_init` arena with every intermediate
   materialized (~2.7GB at seq=512). Check whether `ggml_gallocr` (graph allocator with buffer
   reuse across non-overlapping-lifetime tensors) is bound in the available FFI and would both
   reduce memory AND improve cache locality / reduce allocation overhead — measure, don't
   assume smaller-memory automatically means faster.
3. **RoPE approach**: Loop 11 fixed correctness by using GGUF-baked cos/sin lookup tables
   instead of `ggml_rope_ext`'s runtime computation. Check whether the baked-table approach is
   also the FASTER one, or whether a correctly-parameterized `ggml_rope_ext` call (matching the
   exact theta/scaling the baked tables encode) would be faster while remaining correct — don't
   assume the correctness fix and the speed-optimal approach are the same thing.
4. **Debug scaffolding overhead**: the various `LAYA_RS_DUMP`/`RS_FULL`/`PERTURB`/
   `INJECT_HIDDEN`/`LEAK_CHECK`/`CASE`/`TENSOR_*` env-var checks Loop 11 added are meant to be
   env-gated/cheap, but verify this with the logger — do they add measurable overhead even when
   unset? If so, consider a compile-time `cfg(feature = "debug-dump")` gate instead of a runtime
   env check, so release builds pay zero cost.
5. **Batch/sequence padding strategy**: Loop 9 found production padded to length buckets
   [64,128,256,512] — check what `crates/laya-native` currently does and whether tighter padding
   (pad only to the next multiple of some smaller alignment, not full power-of-2 buckets) saves
   real work on typical short questions without hurting correctness.
6. **Thread count for THIS specific workload**: Loop 6-8's `n_threads=28` tuning was for the LLM
   engine (multi-GB models, long sequences). Laya's encoder is much smaller (421M params,
   sequences ≤512) — verify whether 28 threads is actually optimal for it specifically, or
   whether a different thread count reduces overhead for this smaller workload (sweep a few
   values with the actual `laya-native` binary, not assumed from the LLM tuning).

## Tasks
1. **Instrument `crates/laya-native` with the Loop 11b perf logger** (`timing::perf_span!`) at
   the graph-build, per-layer-group (e.g. every 4 layers), and head/scorer stages — this was
   explicitly deferred from Loop 11b's scope, do it now.
2. **Run the reference case + a couple of benchmark scenarios with logging on**, analyze via
   `scripts/analyze-perf-logs.sh`, identify the actual top 2-3 time-consuming spans (don't guess
   — the candidate list above is a starting menu, not a conclusion).
3. **Try the most promising 1-2 candidates from the list above** (or others the profiling data
   suggests), measuring real before/after latency AND re-validating correctness (reference case
   + at least 3 of the 7 benchmark scenarios from Loop 11) after each change — one change at a
   time, don't stack unvalidated changes.
4. **Report honestly** whether the 92.7ms C++ baseline was beaten, matched, or not — if not
   beaten after reasonable effort, say so plainly and note what was tried and why it didn't
   help, rather than declaring victory on a marginal or noisy improvement.
5. **Update `docs/benchmarks/server-tuning-results.md`** with this loop's findings (whichever
   direction they go).

## Preservation
See frontmatter.

## Validation Requirements
- Given each optimization attempt, When applied, Then re-validated against the reference case
  AND ≥3 benchmark scenarios for correctness before being kept.
- Given the final state, When benchmarked (multiple runs, not a single sample — noise was
  flagged repeatedly in prior loops), Then a real mean/median is reported and compared honestly
  to 92.7ms.
- Given the logger instrumentation, When the benchmark harness runs against a probe/test
  binary, Then `logs/perf/` shows real span data for the new instrumentation points.

## Out-of-scope
- Wiring into production — still gated on genuinely beating the C++ baseline first.
- `act_head` implementation.
- The resource-utilization/multi-worker research (separate track, `researcher-11`) — this loop
  is single-request latency optimization only.
