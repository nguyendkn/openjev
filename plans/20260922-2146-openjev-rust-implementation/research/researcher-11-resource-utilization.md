# Research: RAM/SSD utilization vs single-worker CPU serialization

Scope: `apps/server` serializes every `/bench` through 1 engine-worker thread
(`apps/server/src/app.rs`), each request using `n_threads=28` (`crates/engine/src/lib.rs`). Target
box: 32 vCPU (Xeon Gold 5320), 62GB RAM, 276GB SSD, RAM ~unused beyond a few GB model cache.
Read-only research, no code changed (parallel agent owns `ggmlc`/docs).

## Current architecture (verified)

- 1 `mpsc`-fed worker thread owns `HashMap<String, Engine>`; `bench_handler` sends job+`oneshot`
  and awaits — every `/bench` fully serializes, one job at a time (`Engine` is `!Send`, wraps raw
  FFI/self-referential `LlamaContext<'static>`).
- `EngineConfig::default()`: `n_threads=28`, `n_ctx=4096`, `n_batch=512`; `n_threads` feeds both
  `with_n_threads`/`with_n_threads_batch` — same count for prefill and per-token generation.
- `LlamaBackend` is a process-wide `'static` singleton (`shared_backend()`) — nothing blocks
  multiple `LlamaModel`/`LlamaContext` instances coexisting in one process; a multi-worker pool is
  architecturally unblocked today.
- Read `llama-cpp-2-0.1.156/src/model/params.rs` (installed crate): **`use_mmap` defaults to
  `true`**; engine code never calls `.with_use_mmap(false)`. mmap is already active, just with only
  1 worker to exploit shared pages.
- Registry sizes (`crates/models/src/download.rs`): `qwen3-0.6b` Q8_0 (~0.7GB), `qwen3-4b` Q4_K_M
  (~2.5GB), `minicpm5-2b` Q4_K_M (~1.5GB), `laya` q4_k_m (small) — matches the ~0.6-2.3GB/model
  figure in the brief.
- `LlamaBatch::add(token, pos, seq_ids: &[i32], logits)`/`new(n_tokens, n_seq_max)` confirmed
  multi-sequence-capable (`llama_batch.rs`) — but `Engine::decode_prompt`/`decode_next` hardcode
  `seq_ids = &[0]` (single sequence only) today.

## 1. Multi-worker pool (N workers, fewer threads each)

**Do this — highest-value change.** Replace the single worker thread with a small pool (e.g. N=4)
of identical threads, each with its own `HashMap<String, Engine>` and `EngineConfig{ n_threads:
28/N }`, dispatched from `bench_handler`.

Why: llama.cpp CPU decode is not latency-linear past ~8-12 threads on this hardware class —
token-by-token generation is a dependent chain per sequence, so per-request threads only help the
matmul *inside* one step; beyond a point they mostly buy sync overhead (llama.cpp community
knowledge, **not measured in this repo** — flagged as inferred). 1 worker × 28 threads ties up
28/32 cores for one request's whole readout+generate+laya cycle, queuing everyone else even with
cores idle. 4×7 trades per-request peak speed for real concurrency — throughput under concurrent
load (the actual production shape) should rise even without linear thread scaling.

Trade-offs:
- **RAM**: per-worker `HashMap`, not shared. Worst case (every worker loads all 4 models) ≈ N ×
  ~5GB ≈ 20GB at N=4 — fits 62GB easily even at N=8. mmap (§2) shrinks *resident* cost below this
  since weight pages are shared read-only via OS page cache; the HashMap duplication is
  address-space bookkeeping, not necessarily N× physical RAM.
- **Latency**: a single isolated request gets slower (fewer threads). Acceptable — throughput under
  concurrency is the stated goal, not single-request speed (already tuned separately in Loop 6).
- **Routing**: naive round-robin risks N workers each redundantly loading the same popular model.
  Cheap mitigation: hash `req.model` → worker (consistent routing) instead of round-robin, bounding
  how many workers hold a given model, at the cost of imperfect load balance if one model
  dominates — tune once real request distribution is observed.
- **Effort**: moderate. The existing actor/channel pattern already isolates `Engine` per thread;
  extending 1→N threads each running today's exact `worker_loop`, behind a shared MPMC receiver
  (`tokio::mpsc::Receiver` isn't cloneable — use `async-channel`/`crossbeam` instead) is incremental.
  `supervisor_loop`'s panic-respawn applies per-worker unchanged.

## 2. mmap (already on — real ask is "let N workers share it")

**No code change needed — already the default.** mmap only pays off once multiple
threads/processes map the *same* file concurrently; today's 1-worker setup leaves this unrealized.
Once §1 exists and workers route to the same GGUF path, Linux's page cache serves the read-only
weight pages once regardless of mmap-view count — OS behavior, nothing for `llama-cpp-2` or this
crate to implement. Caveat (inferred): sharing applies to read-only weights only; each `Engine`'s
KV cache/compute buffers (sized by `n_ctx`/`n_batch`) are per-context, not shared — the real
per-worker marginal cost, small relative to weights, doesn't change the §1 RAM estimate much.
Action item: confirm on the target Linux box, not this Windows dev machine — page-cache sharing is
POSIX `mmap(MAP_SHARED)` behavior; Windows section objects differ. Deployment target is Linux per
the brief, so expected to hold, but unverified against that instance.

## 3. Request batching (continuous/multi-sequence batching)

**Capability exists at the `llama-cpp-2` layer; not worth building now.** `LlamaBatch` already
supports multiple `seq_id`s (confirmed above) — the mechanism llama.cpp's own server uses for
continuous batching. But `Engine` is single-sequence by construction (`seq_ids: &[0]` hardcoded,
`clear_kv_cache()` between every stage/request). Real cross-request batching needs: per-request
seq-id allocation, KV-cache-slot management across concurrently in-flight requests instead of a
clean reset per request, a coalescing window, and reconciling `run_readout`'s constrained logic
plus `run_generate`'s greedy loop across heterogeneous-length prompts in one batch — "build a mini
inference server" scope (what llama.cpp's `server`/vLLM already do), not incremental here. High
effort, unclear payoff for a single-turn bench workload once §1 delivers concurrency. **Defer**;
revisit only if §1 measurably falls short (§5).

## 4. SSD role

**Not a bottleneck, no action needed.** With 62GB RAM and a few GB of models, every GGUF is fully
paged into OS cache after first read regardless of worker count — SSD touched once per model per
cold boot. Swap is explicitly not useful (RAM abundant, not pressured); multi-worker's worst-case
~20GB still leaves >40GB headroom. No SSD work recommended — correctly a non-issue per the brief's
own framing.

## 5. n_threads: throughput vs latency retune + benchmark proposal

Loop 6 tuned `n_threads=28` for single-request latency. Under concurrency that becomes a liability:
N concurrent requests each grabbing 28 threads oversubscribes 32 cores badly. Right value: size
per-worker threads so `N_workers × n_threads_per_worker ≲ 32` (same ~4-core OS/tokio headroom,
divided across workers instead of owned by one).

Proposed benchmark (design only, not run): fix `qwen3-0.6b` (cheapest to iterate), a fixed
prompt/options fixture. Vary factorially: **concurrency N** ∈ {1,2,4,8} simultaneous `/bench` POSTs
(small load-gen script, time wall-clock until all N return) × **worker topology** ∈ {1×28
(baseline), 2×14, 4×7, 8×4}. Record per cell: **aggregate throughput** (N / wall-clock — the
metric the user actually cares about) and **p50/p95 latency** (so the trade-off is visible, not
asserted). Expectation to falsify: baseline 1×28 throughput plateaus/degrades as N rises past 1
(serialized queue), while a matched topology (4×7 at N=4) shows higher aggregate throughput despite
slower individual requests. If that doesn't hold on the real box, multi-worker isn't worth it —
cheap to check before committing to §1.

## Priority recommendation

1. **Multi-worker pool (§1), N≈4, n_threads≈7 each** — do first. Moderate effort, directly answers
   "cores/RAM idle while one request blocks everyone"; §2's mmap benefit comes free once this
   exists.
2. **Benchmark (§5) before finalizing N/thread split** — cheap, de-risks the exact topology; run
   after §1 lands, sweep N, pick empirically per production concurrency level.
3. **Not worth doing now**: batching (§3) — high effort, unclear payoff, revisit only if §1 falls
   short. SSD work (§4) — no bottleneck exists.

## Unresolved / needs verification on the real box

- mmap page-cache sharing across workers is Linux behavior (§2) — assumed true for target, not
  verified on that instance.
- llama.cpp CPU thread-scaling past ~8-12 threads for these exact models/quants is inferred, not
  measured here — §5's benchmark is how to confirm before finalizing the split.
- Real production concurrency level isn't stated anywhere — needed to pick concrete N; N=4 is a
  starting guess (32 cores / ~7-8 threads), not a measured optimum.
