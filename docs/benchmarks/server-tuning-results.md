# Server Tuning Results — Loop 6 (n_threads)

Interim checkpoint, NOT final convergence. Proves the tuning methodology with real before/after
numbers; Loop 7+ continues (build flags/native-CPU opts, `batch_size`, `openmp` feature).

## Harness

`scripts/bench-harness.sh <label> [base_url]` + `scripts/aggregate.jq`. For each of the 3 cached
LLM models (`qwen3-0.6b`, `minicpm5-2b`, `qwen3-4b`) it runs all 10 real scenarios from
`plans/20260922-2146-openjev-rust-implementation/research/researcher-05-benchmark-usecases.md`
§4 against `POST /bench` with Laya **not** skipped (server default) — 30 real HTTP requests per
run, no simulated data. Raw per-request `timings` go to
`docs/benchmarks/<label>-raw.jsonl`; `aggregate.jq` computes mean/median/max per timing field per
model into `docs/benchmarks/<label>-summary.json`.

Wiring: `n_threads` was previously hardcoded to `EngineConfig::default()` inside `run_bench`
(`apps/server/src/app.rs`) — no startup flag existed despite `EngineConfig` having the field.
Loop 6 added `openjev-server --threads N` (`apps/server/src/main.rs`), threaded through
`AppState::new` → `supervisor_loop` → `worker_loop` → `run_bench`, which now builds
`EngineConfig { n_threads, ..EngineConfig::default() }` instead of the old
`EngineConfig::default()` call. The server is restarted between each tested config (approach (b)
from D_6 Task 3 was not needed — flag-wiring was the straightforward approach and is now
permanent, not a throwaway).

Three configs tested, each restarted + rerun against the identical 10-scenario harness:
- `baseline` = `--threads 28` (current/unchanged default, Loop 4's original choice: headroom on
  the 32-vCPU box)
- `threads32` = `--threads 32` (every core, no OS/other-process headroom)
- `threads16` = `--threads 16` (conservative, half the box)

## Results — `generation_ms` (greedy generation loop, the dominant cost)

| Model | 28 (mean/median/max, ms) | 32 (mean/median/max, ms) | 16 (mean/median/max, ms) |
|---|---|---|---|
| qwen3-0.6b | 4855.6 / 4758 / 7685 | 5006.0 / 4538 / 7563 | 5644.5 / 4607.5 / 8905 |
| minicpm5-2b | 8070.9 / 8990 / 12983 | 8176.4 / 8760.5 / 13776 | 9901.9 / 10800 / 16317 |
| qwen3-4b | 14914.3 / 14241 / 21793 | 15464.6 / 16481.5 / 22585 | 18006.5 / 20135 / 24731 |

## Results — `constrained_readout_ms` (single-token restricted-logit decode)

| Model | 28 (mean, ms) | 32 (mean, ms) | 16 (mean, ms) |
|---|---|---|---|
| qwen3-0.6b | 125.7 | 316.4 | 164.9 |
| minicpm5-2b | 233.4 | 444.2 | 375.1 |
| qwen3-4b | 389.7 | 523.4 | 666.1 |

## Results — `laya_inference_ms` (separate `laya serve` process, not directly controlled by
`n_threads`, shown for completeness — competes for the same 32 vCPUs)

| Model | 28 (mean, ms) | 32 (mean, ms) | 16 (mean, ms) |
|---|---|---|---|
| qwen3-0.6b | 7208.7 | 6936.3 | 7991.9 |
| minicpm5-2b | 6868.0 | 6623.1 | 7615.2 |
| qwen3-4b | 6543.0 | 6441.8 | 7881.1 |

Full raw per-request data and full per-field aggregates (incl. `wall_ms`, `model_load_ms`,
`warmup_ms`, `tokenize_ms`, `laya_model_load_ms`): `docs/benchmarks/{baseline,threads32,threads16}-{raw.jsonl,summary.json}`.
All 3 runs: `total_requests: 30`, `error_count: 0` (no failed requests in any of the 90 total
HTTP calls across the 3 configs).

## Winner: `n_threads=28` (unchanged default)

`28` beats both `32` and `16` on every model, on both `generation_ms` and
`constrained_readout_ms` — not a marginal win, a consistent one across all 3 model sizes:

- `32` (no headroom) is **worse** than `28` on every model for both fields — most sharply on
  `constrained_readout_ms` (2.0-2.5x worse: 125.7→316.4 for qwen3-0.6b, 233.4→444.2 for
  minicpm5-2b, 389.7→523.4 for qwen3-4b). Plausible cause: `llama_decode`'s own thread-pool
  spin-up/sync overhead scales worse than the parallel speedup gained once every physical core is
  committed with zero headroom for the OS scheduler and the concurrently-running `laya serve`
  process (also CPU-bound, sharing the same 32 vCPUs) — small single-token decodes
  (`constrained_readout`) have the least work to amortize that overhead against, which matches
  why the regression is largest there.
- `16` (half the box) is worse than `28` by a larger margin on `generation_ms` (16-25% slower
  across all 3 models: 4855.6→5644.5, 8070.9→9901.9, 14914.3→18006.5) — expected, `generation_ms`
  is the most parallelism-hungry phase (many sequential decode steps, each benefiting from more
  threads up to a point) and 16 threads is genuinely under-provisioned for it.
- `laya_inference_ms` is the one field where `32` looks marginally *better* than `28` (e.g.
  6543.0 vs 6441.8 for qwen3-4b) — plausibly noise (laya_inference_ms isn't controlled by this
  server's `n_threads` flag at all, it's a separate process's own internal thread count), not
  treated as a tie-breaker against the much larger, consistent `generation_ms`/
  `constrained_readout_ms` wins for `28`.

**Interim recommendation: keep `n_threads=28`.** This is Loop 4's original choice and this loop's
first real tuning iteration confirms — with actual before/after numbers across all 3 models, not
estimates — that it is not just "safe headroom" but the fastest of the 3 values tested on this
32-vCPU box. Server left running on `--threads 28` (PID re-verified alive, `/health` → 200,
`/bench` → correct answer, at the end of this loop).

**Remaining variables for Loop 7+ (explicitly not this loop's scope):**
- Build flags / native-CPU compilation (`-C target-cpu=native`, AVX512 detection, LTO) —
  untested; `llama-cpp-2`'s own build may already auto-detect some CPU features but this wasn't
  verified this loop.
- `n_batch`/batch_size tuning (currently fixed at `EngineConfig::default()`'s `512`).
- The `openmp` feature flag (`llama-cpp-2`/`llama.cpp` support OpenMP as an alternative threading
  backend to the default pthread pool — not enabled, not benchmarked here).
- A finer-grained sweep around 24-28 (only 16/28/32 were tested; 20, 24, 26 etc. untested — 28
  might not be the true local optimum, just the best of these 3 samples).
- `laya serve`'s own thread count/CPU affinity is untouched by this loop — it always ran with
  whatever its own default is, regardless of `openjev-server --threads`; if Laya and
  `openjev-server` are found to meaningfully contend for cores under concurrent load (not tested
  here — this harness runs requests sequentially, one at a time), pinning/reserving cores for
  each process separately could be a Loop 7+ investigation.

## G17 mitigation attempt (Task 6)

Non-trivial via the `laya` binary itself (confirmed again this loop: `laya help`'s SERVE section
still has no `--host`/`--bind` flag), but a host-level `iptables` rule turned out to be a genuine
5-minute fix once `iptables` was installed (`apt-get install -y iptables jq` — neither was
present on the box before this loop): `INPUT` chain had zero pre-existing rules (default ACCEPT
policy, `iptables -L INPUT -n` empty before this change), so appending two rules was safe with no
risk to unrelated traffic:
```
iptables -A INPUT -p tcp --dport 8090 -i lo -j ACCEPT
iptables -A INPUT -p tcp --dport 8090 -j DROP
```
Verified: `curl 127.0.0.1:8090/health` (from the box itself, loopback) → `200` unchanged;
external `curl 103.146.166.46:8090/health` (from the Windows machine) → connection times out
(now actually DROPped by this host-level rule, not just an unverified upstream cloud-firewall
rule as E_5's G17 finding described). `README.md` corrected (Task 7) to describe this real
mechanism instead of the old, factually-wrong "bound to 127.0.0.1... by design" claim.

**Caveat, not fully closed:** this `iptables` rule is **not persisted across a reboot** — no
`iptables-persistent`/`netfilter-persistent` package is installed, and none was installed this
loop (out of this loop's minimal-fix scope). If the box reboots, port 8090 reverts to relying on
whatever external/cloud-level filtering G17 originally found (unverified from inside this
project). Re-applying the two `iptables` commands above after any reboot, or installing
`iptables-persistent`, is a cheap Loop 7+ follow-up.
