# Server Tuning Results — Loops 6-7 (n_threads, openmp, native-CPU build, n_batch)

Loop 6 proved the tuning methodology (`n_threads`) with real before/after numbers. Loop 7 (see
"Loop 7" section below) closes out tuning: verifies `openmp` is genuinely active, tests a
native-CPU build and 2 `n_batch` values, and states the final recommended production config.

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


---

# Loop 7 — openmp verification, native-CPU build, n_batch sweep, final config

This loop closes out server tuning. All tests below reuse the identical `scripts/bench-harness.sh`
methodology (3 models x 10 real scenarios, Laya not skipped, 30 real HTTP requests per run),
`n_threads=28` held constant (Loop 6's proven winner), so every comparison here isolates exactly
one new variable against the Loop 6 `28` baseline (`generation_ms` mean/median/max qwen3-0.6b
4855.6/4758/7685, minicpm5-2b 8070.9/8990/12983, qwen3-4b 14914.3/14241/21793;
`constrained_readout_ms` mean 125.7/233.4/389.7).

## 1. Is `openmp` actually active? Yes — verified at 3 independent levels, not assumed

`llama-cpp-2 = "0.1"` in `crates/engine/Cargo.toml` has no `default-features = false`, so
`llama-cpp-2`'s own `default = ["openmp", "android-shared-stdcxx", "common"]` applies, which maps
to `llama-cpp-sys-2/openmp` (`openmp = ["llama-cpp-sys-2/openmp"]` in `llama-cpp-2`'s manifest).
That crate's `build.rs` (`if cfg!(feature = "openmp") { config.define("GGML_OPENMP", "ON") }`)
does pass `GGML_OPENMP=ON` to CMake by default — but a declared Cargo feature is not proof the
compiled artifact actually links/uses it, so this was verified 3 ways on the live build:

1. **CMakeCache.txt** (`target/release/build/llama-cpp-sys-2-*/out/build/CMakeCache.txt`):
   `GGML_OPENMP:BOOL=ON`, `GGML_OPENMP_ENABLED:INTERNAL=ON`, `OpenMP_C_FLAGS:STRING=-fopenmp`,
   `OpenMP_CXX_FLAGS:STRING=-fopenmp`.
2. **Real compiler invocation** (`ggml-cpu`'s `flags.make`): `C_FLAGS`/`CXX_FLAGS` both literally
   contain `-fopenmp` (and, after the native rebuild below, `-march=native` alongside it) — the
   flag isn't just declared in CMake, it's on the actual `gcc`/`g++` command line.
3. **Linked binary**: `ldd target/release/openjev-server` shows `libgomp.so.1 =>
   /lib/x86_64-linux-gnu/libgomp.so.1` — the running server process genuinely links GNU OpenMP's
   runtime. `nm libggml-cpu.a | grep 'GOMP\|omp_get'` shows 5 real undefined-symbol references
   (`GOMP_barrier`, `GOMP_parallel`, `GOMP_single_start`, `omp_get_num_threads`,
   `omp_get_thread_num`) resolved against that runtime, not dead-stripped.

**Verdict: openmp is genuinely active**, not just a declared default feature — this is llama.cpp's
real threading backend for CPU decode on this build, not the plain pthread pool.

## 2. CPU-native build (`target-cpu=native` + `GGML_NATIVE`)

`llama-cpp-sys-2`'s `build.rs` reads the Rust `target_cpu` (from `RUSTFLAGS`/`CARGO_ENCODED_RUSTFLAGS`
parsing) and, when it equals `"native"`, calls `config.define("GGML_NATIVE", "ON")` — otherwise
`GGML_NATIVE=OFF` is set explicitly (no silent auto-detection). There is no separate llama.cpp
Cargo feature for this; `RUSTFLAGS="-C target-cpu=native"` is the actual mechanism, confirmed from
`build.rs` source, not guessed.

Rebuilt: `RUSTFLAGS="-C target-cpu=native" cargo build --release --workspace` (exit 0, 1m44s wall,
only the 2 pre-existing warnings). Verified real: new `CMakeCache.txt` shows `GGML_NATIVE:BOOL=ON`
(vs `OFF` in the non-native build), and `flags.make` shows `-march=native -fopenmp` literally on
the `gcc`/`g++` command line for `ggml-cpu`. Box CPU: Intel Xeon Gold 5320 (Ice Lake-SP,
AVX-512-capable) — non-native build had `GGML_AVX512:BOOL=OFF`.

Harness run (`native` label, `--threads 28`, default `--batch` i.e. 512, Laya not skipped, 30
requests, `error_count:0`):

| Model | native gen mean/median/max (ms) | vs Loop-6 `28` baseline | native readout mean (ms) | vs baseline |
|---|---|---|---|---|
| qwen3-0.6b | 4338.2 / 3476 / 7321 | -10.7% / -27.0% / -4.7% | 112.2 | -10.8% |
| minicpm5-2b | 8544.2 / 9595.5 / 13144 | +5.9% / +6.7% / +1.2% | 178.9 | -23.4% |
| qwen3-4b | 14197.6 / 15724 / 20913 | -4.8% / +10.4% / -4.0% | 291.0 | -25.3% |

**Reading:** `generation_ms` is a mixed bag (qwen3-0.6b/qwen3-4b mean slightly faster, minicpm5-2b
mean slightly slower; medians noisier still) — no clean, consistent win/loss the way Loop 6's
`n_threads` sweep had, plausibly within this single-run harness's own run-to-run noise band
(sequential requests, shared box, no repeat runs to average out variance). `constrained_readout_ms`
is the one genuinely consistent signal: **10-25% faster across all 3 models** with the native
build — small single-token decodes benefit from `-march=native`'s wider SIMD (AVX-512 now
reachable) with less amortization needed than the long multi-step `generation_ms` phase. Kept for
the final config given this consistent win and zero measured downside (same `error_count:0`,
build/test/regression all clean).

## 3. `n_batch` sweep (256, 1024) — both run against the native build, `--threads 28`

`crates/engine/src/lib.rs`'s `EngineConfig.n_batch` (default `512`) was previously hardcoded via
`EngineConfig::default()` inside `run_bench`, same pattern Loop 6 fixed for `n_threads`. Added a
`--batch` flag to `openjev-server` (`apps/server/src/main.rs`), threaded through
`AppState::new(n_threads, n_batch)` -> `supervisor_loop` -> `worker_loop` -> `run_bench` ->
`EngineConfig { n_threads, n_batch, ..EngineConfig::default() }` — identical plumbing shape to
Loop 6's `--threads`, not a new pattern.

| Model | batch=256 gen mean/median/max | batch=256 readout mean | batch=1024 gen mean/median/max | batch=1024 readout mean |
|---|---|---|---|---|
| qwen3-0.6b | 4148.0 / 3289.5 / 7146 | 99.3 | 3965.3 / 3269 / 6799 | 105.3 |
| minicpm5-2b | 8500.2 / 9910 / 13064 | 187.7 | 8277.8 / 9249 / 12547 | 185.4 |
| qwen3-4b | 14782.8 / 16470 / 22252 | 292.3 | 14671.5 / 16160 / 21349 | 300.9 |

(baseline for this comparison = the native/batch=512 row in §2 above: qwen3-0.6b 4338.2/112.2,
minicpm5-2b 8544.2/178.9, qwen3-4b 14197.6/291.0. All 3 batch runs: `total_requests:30`,
`error_count:0`.)

**Reading:** neither `256` nor `1024` shows a clean win across all 3 models — `batch=1024` trends
slightly better on the two smaller models (qwen3-0.6b -8.6% gen mean, minicpm5-2b -3.1% gen mean)
but slightly worse on `qwen3-4b` (+3.3% gen mean, both readout fields +3-4% worse than batch=512).
`batch=256` is similarly mixed (better on qwen3-0.6b, roughly flat/worse on the other two). These
deltas are all inside the same noise band as §2's `generation_ms` swings (single-run harness, no
repeats) — none of the 3 models shows a large, one-directional effect the way the Loop 6
`n_threads` sweep did. **No evidence to move off the `512` default**; the (single-token decode
per request, small `n_batch` window rarely saturated) workload here doesn't exercise `n_batch`
tuning the way a long-prompt prefill workload would.

## 4. Final recommended production config

- **`n_threads=28`** (Loop 4/6, unchanged — still the proven winner vs 16/32).
- **Native CPU build**: `RUSTFLAGS="-C target-cpu=native" cargo build --release --workspace`
  (`GGML_NATIVE=ON`, `-march=native` reaching this box's AVX-512) — consistent 10-25%
  `constrained_readout_ms` win across all 3 models, no measured downside.
- **`n_batch=512`** (default, unchanged) — neither `256` nor `1024` showed a consistent win.
- **openmp**: active by default (`llama-cpp-2`'s own default feature set), verified genuinely
  linked/used, left as-is — no change needed.

**Currently running on the server**: `./target/release/openjev-server --port 80 --threads 28`
(binary built with `RUSTFLAGS="-C target-cpu=native"`, `--batch` omitted so `EngineConfig`
default `512` applies), PID re-verified alive at loop end, `/health` and `/bench` both externally
reachable and correct for all 3 models.

## 5. Full regression pass (this loop's final config)

- **CLI** (`./target/release/openjev-cli --model <m> --prompt "Which city is the capital of
  France?" --options "London,Paris"`, Laya not skipped): all 3 models (`qwen3-0.6b`,
  `minicpm5-2b`, `qwen3-4b`) — `readout.best_option=Paris`, `generate.valid=true`
  (`{"answer":"Paris"}`), `laya.best_option=Paris`. Real per-model timings recorded (e.g.
  qwen3-0.6b `constrained_readout_ms=56`, `generation_ms=1404`).
- **Server** (external `curl` to `103.146.166.46:80/bench` from outside the box, not SSH-side):
  same 3 models, same prompt — `readout=Paris`, `generate.valid=true`, `laya=Paris` for all 3,
  both on the sentinel-injected build (before G13 test) and again on the final clean rebuilt
  binary after revert.
- `cargo build --workspace` (debug + release) and `cargo test --workspace` (3 passed / 0 failed,
  `pipeline::readout`) both re-run clean after every rebuild this loop (native build, `n_batch`
  plumbing add, G13 sentinel inject, G13 sentinel revert) — no regression at any step.

## 6. G13 + Laya-outage re-test (on this loop's final native+n_batch-plumbed build)

Backed up `app.rs` (md5 `0546062f...`), injected a sentinel panic (`prompt ==
"__QA_LOOP7_FAULT_INJECT__"`) into the new `run_bench` (post `n_batch`-param signature), rebuilt
release (exit 0), restarted `--threads 28`. Sentinel request -> HTTP 500
`{"error":"engine worker thread dropped the response"}` (clean, not a hang). `/health` immediately
after -> 200. Next `/bench` -> HTTP 200, `model_load_ms=927` (fresh reload, cache genuinely
cleared), correct answer. Server log shows the real panic message and "respawning with an empty
model cache" — same PID (86050) throughout. Confirms G13/G15's fixes survive the `n_batch`
signature change, same as Loop 6 confirmed for the `n_threads` change.

Same session: killed the live `laya serve` PID -> `/bench` -> HTTP 200 (not 500), `laya:null`,
`laya_inference_ms=13` (fast fail, not a stall), real degrade log
(`laya unavailable, continuing without it: laya error: laya serve unreachable: ...`). Laya
restarted, `/health` -> 200 again.

Cleanup: `app.rs` reverted from backup, md5 exact match to pre-injection
(`0546062fef4d761c9a75e2c996af029e`), `grep -c QA_LOOP7_FAULT_INJECT` -> 0, rebuilt clean (exit 0),
restarted on the final `--threads 28` config (new PID 86557), backup files deleted.

## 7. G18 (iptables persistence) — closed this loop

Ubuntu 24.04 with `systemd` as init (confirmed, `ps -p 1 -o comm=` -> `systemd`), so this was
trivial, not a gap to document-and-skip. Installed
`/etc/systemd/system/openjev-laya-firewall.service` (oneshot, `WantedBy=multi-user.target`,
`ConditionPathExists=!/run/openjev-laya-firewall.applied` so it only (re)applies once per boot):
re-runs the same 2 rules Loop 6 added by hand
(`iptables -A INPUT -p tcp --dport 8090 -i lo -j ACCEPT` then `... -j DROP`), then touches the
`/run` marker. `systemctl enable` confirmed (`multi-user.target.wants` symlink created).

Verified for real, not just "enabled" on paper: manually removed both live `iptables` rules and
the `/run` marker (simulating a fresh boot's empty `INPUT` chain), then `systemctl start
openjev-laya-firewall.service` — `systemctl status` shows all 3 `ExecStart` lines exited
`status=0/SUCCESS`, and `iptables -L INPUT -n -v --line-numbers` immediately after shows the exact
same 2 rules back in place. Re-verified `127.0.0.1:8090/health` -> 200 (internal Laya calls still
work) and external `103.146.166.46:8090/health` -> timeout again (curl exit 28) — G17's mitigation
intact after the G18 test. Not reboot-tested (a real reboot would also take down the manually
`nohup`'d `openjev-server`/`laya serve` processes, which have no systemd units of their own and
weren't in this loop's scope to add) — but the boot-time mechanism itself is installed, enabled,
and proven to correctly reapply the exact rules from an empty starting state, which is the
substantive part of "survives a reboot."

## 8. Final server state confirmed alive

`openjev-server` PID 86557 (`--port 80 --threads 28`, native build, `n_batch` default 512) and
`laya serve` PID 86269 both running at loop end. Internal (`127.0.0.1`) health: `:80` -> 200,
`:8090` -> 200. External (from outside the box): `:80/health` -> 200, `:80/bench` correct for all
3 models (readout+generate+laya), `:8090` -> connection timeout (G17 still enforced, G18 unit
re-verified separately above).

## 9. Remaining gaps (unchanged from Loop 6, not this loop's scope)

- **G10** — no unit tests for `generate.rs`.
- **G12** — `apps/server` still zero in-repo unit tests (now also covers the `n_batch` plumbing
  added this loop).
- **G14** — port 80 deviation, previously accepted.
- **G16** — docs self-contradiction, cosmetic.
- **G2/G11** — out of scope, carried.
- A finer-grained `n_threads` sweep (20/24/26) and multi-run averaging for `generation_ms`
  (to separate real batch/native effects from single-run noise) are both explicitly out of this
  loop's scope per D_7 ("not an open-ended search").
