---
loop: 14
status: pending
preservation_constraints:
  - "openjev-server stays live and correct throughout — this changes its core concurrency architecture, so verify at every step, not just at the end"
  - "laya serve (now the fixed -O3 binary) unaffected"
  - "All 3 LLM models + Laya still correct via apps/cli and apps/server"
  - "G13 respawn-supervisor pattern must be preserved PER WORKER in the new pool architecture — a panic in one worker must not take down the others or hang the server"
  - "reset_context still called correctly per-request in every worker"
  - "cargo build --workspace (debug+release) and cargo test --workspace still pass"
  - "DO NOT touch crates/laya-native/** this loop — Loop 13 may still be working there"
---

## Objective
Empirically benchmark worker/thread topologies, then implement a multi-worker pool in
`apps/server` to use idle RAM/cores for concurrent throughput instead of serializing all
requests through one worker.

`researcher-11-resource-utilization.md` found: `mmap` is already enabled by default (no work
needed there), real request-level batching is high-effort/low-certain-benefit (skip for now),
SSD isn't the bottleneck (skip), but a multi-worker pool (~4 workers × ~7 threads each instead
of 1 worker × 28 threads) is a promising, moderate-effort way to let independent concurrent
requests actually run in parallel instead of queueing behind a single serialized worker —
using the abundant idle RAM (62GB, currently using only a few GB) to hold multiple
model-engine copies instead of one.

## Tasks
1. **Benchmark topologies empirically BEFORE committing to an architecture**: using the
   existing `apps/server` (still single-worker, unchanged for this step), can't directly test
   multi-worker throughput yet — instead, build a small standalone throughput probe: spawn N
   OS threads, each loading its own `Engine` instance with `n_threads=T` (for a few (N,T)
   combinations where N*T stays near 28-32, e.g. 1×28, 2×14, 4×7, 8×4), fire concurrent
   requests at each pool synthetically (a tight loop calling `run_readout`/`run_generate`
   directly, not through HTTP), measure aggregate throughput (requests/sec) and per-request
   p50/p95 latency at concurrency levels 1, 2, 4, 8. This can be a throwaway test binary/bench,
   doesn't need to be production code yet.
2. **Pick the best topology from real data** (not the researcher's N≈4 guess if the data says
   otherwise) — report the actual measured numbers for all tested combinations, then justify
   the chosen (N, threads-per-worker).
3. **Design + implement the multi-worker pool in `apps/server`**: replace the single
   `mpsc`+`oneshot` worker with a pool of N workers, each with its own `HashMap<String, Engine>`
   cache and its own `EngineConfig{n_threads: T}`. Load-balance incoming jobs across the pool
   (simplest correct approach: round-robin, or least-recently-used-worker if trivial — don't
   over-engineer). PRESERVE the existing G13 respawn-supervisor pattern per-worker (each
   worker's supervisor independently catches panics and respawns that worker with an empty
   cache, without affecting the other N-1 workers) — this is the highest-risk regression point,
   test it explicitly (fault-inject one worker, confirm the others keep serving).
4. **Handle the lazy-load-and-cache tradeoff across workers**: with N workers each caching
   models independently, a naive round-robin could mean N separate copies of the SAME model
   get loaded (using N× the RAM one model needs) if requests for that model get routed to
   different workers. Decide and implement a policy — e.g. route by consistent hashing on
   model name so the same model always goes to the same worker (bounded worst-case: at most
   min(N, num_distinct_models) copies loaded, not N per model) — and document the choice.
5. **Verify concurrent throughput improvement for real**: fire multiple concurrent `/bench`
   requests (different models, to exercise the routing) via external curl and confirm they
   complete in genuinely overlapping wall-clock time (not serialized) — this is the actual
   point of the whole change, prove it empirically, don't just trust the design.
6. **Full regression**: 3 models × 3 methods via CLI + external curl, G13 fault-injection
   (now per-worker — test at least 2 workers, fault-inject one, confirm the other still serves
   and the faulted one self-heals), Laya-outage graceful degrade, G17/G18 unaffected.

## Preservation
See frontmatter.

## Validation Requirements
- Given the topology probe, When run at concurrency 1/2/4/8 for each tested (N,T) combination,
  Then real throughput (req/s) and latency percentiles are reported for all combinations, not
  just the winner.
- Given the implemented pool, When 2+ concurrent requests for different models arrive, Then
  they complete with overlapping wall-clock time (verify via timestamps, not just "it didn't
  error").
- Given a fault-injected panic in one worker, When it occurs, Then only that worker respawns
  (empty cache, brief unavailability for requests routed to it) while requests routed to other
  workers succeed normally throughout.
- Given the same model requested repeatedly, When routed across time, Then it's NOT reloaded
  redundantly across multiple workers (verify via `model_load_ms` staying near 0 after first
  load, consistent with the routing policy).

## Out-of-scope
- `crates/laya-native` — untouched this loop, Loop 13's territory.
- True request batching (single forward pass over multiple sequences) — deferred per the
  research's own recommendation, high effort/uncertain payoff.
- Dynamic pool resizing / auto-scaling worker count based on load — a fixed pool size chosen
  from this loop's benchmark data is sufficient for now.
