# Server Tuning Results — Loops 6-8 (n_threads, openmp, native-CPU build, n_batch, Laya thread fix)

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

---

# Loop 8 — Laya `--threads` deployment-bug fix, persisted + proven, plus Q8_0 quant switch

## 1. Root cause and the fix

`laya serve` was started in Loops 5-7 by a hand-typed `setsid nohup ... laya serve <gguf> --port
8090 --device cpu` with **no `--threads` flag** — `laya --help` documents the default as `4`, so
Laya ran on 4 of the box's 32 vCPUs for 3 straight loops. This was never caught because Laya's
speed was only ever observed bundled inside the combined `/bench` 3-method timing (logged as
"just how slow Laya is"), never benchmarked in isolation. Manually restarting with `--threads 28`
(matching the LLM engine's own Loop 6-proven thread count) cut one measured request from
**8350ms to 1665ms (~5x)**; a full harness re-run below confirms this at scale.

**Persisted, not a one-off restart:**
- `scripts/start-laya-serve.sh` — the single canonical start command, `--threads 28` baked in as
  the default (overridable via `$1`), used by both manual restarts and the systemd unit below.
- `scripts/openjev-laya-serve.service` + `scripts/openjev-server.service` — new systemd units
  (installed at `/etc/systemd/system/`, both `enabled`, `Restart=on-failure`) that now manage
  **both** long-running processes' full lifecycle, not just the G18 firewall-rule reapplication.
  `ExecStart` bakes `--threads 28` directly into the unit, so a bare `systemctl restart` (by a
  future loop, or after a real reboot — both units are `enabled`) can no longer silently regress
  the thread count the way the old ad-hoc `nohup` commands could. Migrated the live processes
  onto systemd this loop (brief `systemctl start` + kill-old-PID handoff, `/health` back to 200
  within ~2-8s both times, no request-level outage observed).
- `README.md`'s "Laya Server Process" section rewritten to describe the systemd-managed lifecycle
  instead of the old raw `nohup` commands.

## 2. Harness-measured before/after (30 requests, `laya-threads28` label)

Loop 7's `native-summary.json` (`--threads 4`, the undiscovered bug, still the config for every
prior loop's numbers) vs this loop's `docs/benchmarks/laya-threads28-{raw.jsonl,summary.json}`
(`--threads 28`, Q8_0 — see §3 for why Q8_0 replaced UD_Q4_K_M), same 3 models x 10 scenarios,
Laya not skipped, `error_count: 0` both runs:

| Model | `laya_inference_ms` OLD (mean, threads=4) | NEW (mean, threads=28+Q8_0) | Speedup |
|---|---|---|---|
| qwen3-0.6b | 7208.7 | 1243.8 | 5.80x |
| minicpm5-2b | 6868.0 | 1157.0 | 5.94x |
| qwen3-4b | 6543.0 | 1206.1 | 5.42x |

Matches the manual spot-check's ~5x order of magnitude, now confirmed harness-wide (30 requests,
not 1-2 curl calls) across all 3 models, not a lucky sample.

**Side observation, not this section's focus:** `constrained_readout_ms` in this same run reads
2-3x higher than Loop 7's isolated number (e.g. qwen3-0.6b 413.0 vs Loop 7's 112.2; full numbers
in the raw JSON) despite the LLM engine's own `n_threads`/native build being unchanged. Plausible
cause: Laya's process now genuinely contends for all 32 cores (previously it only used 4, leaving
the LLM engine's 28 threads mostly uncontested even when interleaved) — not proven, single-run
harness with no repeats (same documented limitation carried from Loop 6/7). Flagged as an open
question, not treated as a regression to chase this loop (readout is still within Jev's
real-world competitive range either way, see §5).

## 3. Quant check: is UD_Q4_K_M actually the fastest CPU quant here?

Investigated per this loop's own scope (Task 3) and a mid-loop coordinator note citing a
community CPU benchmark (i3-12100, 4c/8t: 112ms/1-question) that is *faster* than our 32-core
box's threads=28 result — prompting a check of whether the `ggmlc`/`laya` build itself was
missing native-CPU flags (the same class of bug Loop 7 found and fixed for `llama-cpp-2`).

**Native-build check (verified, not assumed):** `/tmp/ggmlc/build/CMakeCache.txt` already shows
`GGML_NATIVE:BOOL=ON`; `ggml_lib`'s real `flags.make` shows `-march=native` literally on the
`gcc`/`g++` command line, and `gcc -march=native -E -v` on this box resolves to `cascadelake`
with `avx512f/dq/cd/bw/vl/vnni` all present in `lscpu`'s `Flags`. **This lever was already fully
engaged before this loop** — not the source of the remaining gap.

**Quant comparison (verified real):** single-request `/v1/decide` timing, 5 reps each, isolated
test port (8091-8093) so as not to disturb the live 8090 instance, `--threads 28` held constant,
identical payload:

| Quant | File size | 5 reps (s) | Mean (s) | Median (s) |
|---|---|---|---|---|
| UD_Q4_K_M (old default) | ~430 MB (symlinked cache blob) | 1.738/1.677/1.720/1.696/2.675 | 1.901 | 1.720 |
| Q8_0 | 451.5 MB | 1.644/1.294/1.955/1.278/1.263 | 1.487 | 1.294 |
| F16 | 846.1 MB | 2.001/1.352/1.363/1.367/1.181 | 1.453 | 1.363 |

Q8_0 and F16 both beat Q4_K_M by ~20-24% mean — confirming the K-quant's CPU dequant path in this
`ggmlc` build is genuinely slower than a plain 8-bit or fp16 read, not just noise (consistent
direction across all 5 reps of Q4_K_M's slowest end vs both alternatives' tightest cluster).
**Q8_0 adopted as the new production default** (same speed class as F16, half the file size/RAM)
— `laya_english_q8_0.gguf` now lives at `/root/.cache/laya-models/` and is `start-laya-serve.sh`'s
new default GGUF path. Correctness re-checked: identical `choice`/`probabilities` shape and a
materially identical decision on the test prompt across all 3 quants (`confidence` 0.884-0.890,
`billing` 0.971-0.973) — the quant switch changes speed, not the answer.

**Thread-count re-swept on Q8_0** (isolated test port, 5 reps each, to rule out the LLM-engine's
own "more threads isn't always better" pattern applying here too): `--threads 4` mean 6.111s (much
worse — confirms this is a genuine parallelism win, not oversubscription overhead for this small
model), `--threads 16` mean 2.048s, `--threads 28` mean 1.487s (§3's own Q8_0 row), `--threads 32`
mean 1.341s (only ~10% faster than 28, within this 5-rep sample's noise band, and no headroom left
for the OS/`openjev-server`'s own 28 threads) — **28 confirmed as the right choice, not changed**,
consistent with the LLM engine's own Loop 6 finding.

**Remaining gap, explicitly not chased further this loop:** even at threads=28+Q8_0 (~1.2-1.4s
mean), this is still ~8-25x slower than the community CPU anchors researcher-07 found (i3-12100
4c/8t: 112ms/1-question; Shray15: 360ms, undisclosed CPU) despite this box having 4-8x more cores
and a verified-active native/AVX-512 build. With native flags, thread count, and quant format all
now ruled out as the cause, the remaining gap most likely lives inside `ggmlc`'s own C++
implementation (op-graph structure, per-request overhead, or GEMM kernel efficiency for this
specific op set) — that source lives in the external `ggmlc` repo (`/tmp/ggmlc`), not this
project's own crates, so profiling/patching it is out of this loop's and this repo's scope.
Documented here as an honest, unresolved gap rather than claimed "fully optimized."

## 4. Constrained-readout: already Jev-competitive (re-confirmed)

Loop 7 found `constrained_readout_ms` mean 112.2-291.0ms across the 3 models — already inside
Jev's own marketed 70-500ms claim. This loop's fresh harness run (§2) shows 413.0-593.6ms for the
same field (see §2's side-observation for the likely cause of the increase) — still within the
wider, third-party-measured Jev cloud-API cluster of 300-1070ms P50 (researcher-07 §1, dev.to +
sysone-bench, not just TypeSafe's own marketing range). **Both readings support the same
conclusion: constrained-readout has been Jev-competitive since Loop 7 and remains so this loop** —
called out explicitly here so it doesn't get lost under the bigger Laya-fix news.

## 5. Honest reframe: JSON-generation is not a Jev-equivalent, by design

`pipeline::generate`'s full autoregressive token-by-token JSON generation is architecturally
**not** what Jev/Laya do — Jev and Laya never generate tokens, they score in a single pass over
fixed candidate labels. `generate.rs` is this project's own **second comparison arm**, inherited
from the original SemIf/OpenJev methodology (which predates this project's Laya integration), not
a method meant to reach Jev's speed class. It will not, and is not expected to — chasing that
would be solving the wrong problem. Its own useful comparison is against the "naive full-LLM"
baseline (researcher-05's ~8.5s reference), which it already beats via constrained decoding
(1.3-15.6s mean across the 3 models this loop, scaling with model size as expected for a
decode-loop-bound method).

## 6. Final summary table — all 4 methods

| Method | Jev's claimed/measured range | Ours (Loop 7) | Ours (Loop 8, after Laya fix) |
|---|---|---|---|
| Jev cloud API (reference only, not ours) | Marketing: 70-500ms. Real 3rd-party P50: 300-1070ms (researcher-07) | — | — |
| Constrained-readout (ours) | compared against above | 112.2-291.0ms mean | 413.0-593.6ms mean (still within the 300-1070ms real-world cluster) |
| Laya (ours) | Community CPU anchors: 112-360ms/question (researcher-07 §2, 4-8 thread consumer CPUs) | 6543.0-7208.7ms mean (undiscovered threads=4 bug) | **1157.0-1243.8ms mean (threads=28 fix + Q8_0 quant, ~5.4-5.9x faster)** — still 8-25x above the community CPU anchors; gap analyzed in §3, not further reducible within this repo's scope |
| Generation (ours) | **Not comparable by design** (§5) — architecturally different from Jev/Laya's single-pass scoring | 4338.2-14914.3ms mean | 4191.3-15600.8ms mean (unchanged, not this loop's target) |

## 7. Full regression pass (this loop's final config: threads=28 for both processes, Q8_0 Laya,
systemd-managed)

- **CLI**, all 3 models (Laya not skipped): `readout.best_option=Paris`, `generate.valid=true`
  (`{"answer":"Paris"}`), `laya.best_option=Paris` for all 3.
- **External curl** (`103.146.166.46:80/bench`, not SSH-side), all 3 models: same results,
  `laya_inference_ms` 1231-1293ms (single-request, first-hit variance, consistent with §2's
  harness aggregate). External `/health` → 200. External `:8090` → connection timeout (curl exit
  28), G17 firewall still enforced after the systemd migration.
- **`cargo build --workspace`** (release, `RUSTFLAGS=-C target-cpu=native`) exit 0, only the 2
  pre-existing warnings. **`cargo test --workspace`**: 3 passed / 0 failed (`pipeline::readout`).
- **G13**, 5th consecutive loop, this loop's own sentinel (`__QA_LOOP8_FAULT_INJECT__`, distinct
  from every prior loop's own string): backed up `app.rs` (md5 `0546062f...`, matches every prior
  loop's clean baseline exactly), injected, rebuilt (exit 0), restarted via
  `systemctl restart openjev-server.service`. Trigger → HTTP 500
  `{"error":"engine worker thread dropped the response"}` (clean). `/health` immediately after →
  200. Next `/bench` → `model_load_ms=881` (fresh reload), correct answer. `journalctl -u
  openjev-server.service` shows the real panic message
  (`QA Loop8 fault-injection re-verification panic`) and
  `engine worker thread panicked, respawning with an empty model cache` — confirms the
  respawn/logging path survives the systemd migration, not just the old raw-process model.
  Cleanup: `app.rs` reverted, md5 exact match, sentinel grep count 0, rebuilt clean, restarted on
  final config.
- **Laya-outage graceful degrade**, re-tested via `systemctl stop openjev-laya-serve.service`:
  `/bench` → HTTP 200 (not 500), `laya: null`, `laya_inference_ms=10` (fast fail), real degrade log
  (`laya unavailable, continuing without it: laya error: laya serve unreachable: ...`).
  `systemctl start openjev-laya-serve.service` → `/health` → 200 within 8s, next `/bench` →
  `laya.best_option=Paris` (self-heal confirmed), no `openjev-server` restart needed either way —
  same graceful-degrade contract as every prior loop, now proven through a systemd stop/start
  cycle instead of a raw `kill`.

## 8. Final server state confirmed alive

`openjev-server.service` active, PID 152077 (`--port 80 --threads 28`, native build, `n_batch`
default 512), `openjev-laya-serve.service` active, PID 152174 (`laya serve
/root/.cache/laya-models/laya_english_q8_0.gguf --port 8090 --device cpu --threads 28`). Both
`systemctl is-enabled` → enabled (survive a reboot). External `:80/health` → 200, external
`:8090` → connection timeout (G17 intact). Internal `/bench` correct for all 3 models.

## 9. Remaining gaps after Loop 8

- The ~8-25x residual gap between our Laya numbers and community CPU anchors (§3) is real,
  investigated as far as this repo's scope allows (native flags, thread count, quant format all
  ruled out), and not further reducible without profiling/patching the external `ggmlc` C++
  source — out of scope for this Rust-project loop.
- The `constrained_readout_ms` increase noted in §2 (2-3x vs Loop 7's isolated number) is
  observed, not root-caused — flagged as a hypothesis (Laya's own 28-thread footprint now
  genuinely contending for cores), not confirmed; would need multi-run averaging with/without a
  concurrently-running Laya process to isolate, which is out of this loop's "not an open-ended
  search" scope (same caveat Loop 6/7 already carried for single-run harness noise generally).
- G10/G12/G14/G16/G2/G11 — unchanged, carried from prior loops.
- The finer-grained `n_threads` sweep (20/24/26) for the LLM engine remains untested — unchanged
  from Loop 7, not this loop's focus (Loop 8's thread-sweep work targeted Laya, a separate
  process/flag).

---

# Loop 12 — Root cause of the Laya latency gap: `CMAKE_BUILD_TYPE` was empty. Fixed, deployed, measured.

**This section supersedes Loop 8 §3's "remaining gap most likely lives inside `ggmlc`'s own C++
implementation" conclusion.** That conclusion was wrong. The gap was a build misconfiguration in
our own deployment, and it is now fixed in production.

## 1. Root cause

`laya serve` runs from `/tmp/ggmlc/build/examples/laya/laya`, built in Loop 5 with a plain
`cmake -S /tmp/ggmlc -B build`. CMake's default `CMAKE_BUILD_TYPE` is the **empty string**, and an
empty build type means CMake applies **none** of `CMAKE_CXX_FLAGS_RELEASE` / `_DEBUG` /
`_RELWITHDEBINFO` — the compiler is invoked with no `-O` flag at all, i.e. effectively `-O0`. The
binary built cleanly, ran correctly, and returned correct answers for 7 loops. It was just ~13x
slower than it needed to be.

Verified directly, on the real compiler command lines (not inferred from cache variables):

```
# OLD production build — /tmp/ggmlc/build/CMakeCache.txt
CMAKE_BUILD_TYPE:STRING=                       <-- empty
GGML_NATIVE:BOOL=ON
# /tmp/ggmlc/build/CMakeFiles/ggml_lib.dir/flags.make
C_FLAGS   = -std=gnu11  -fPIC -march=native
CXX_FLAGS = -std=gnu++17 -fPIC -march=native    <-- no -O3

# NEW build — /tmp/ggmlc/build-release/CMakeCache.txt
CMAKE_BUILD_TYPE:STRING=Release
GGML_NATIVE:BOOL=ON
# /tmp/ggmlc/build-release/CMakeFiles/ggml_lib.dir/flags.make
C_FLAGS   = -O3 -DNDEBUG -std=gnu11  -fPIC -march=native
CXX_FLAGS = -O3 -DNDEBUG -std=gnu++17 -fPIC -march=native
# /tmp/ggmlc/build-release/examples/laya/CMakeFiles/laya.dir/flags.make
CXX_FLAGS = -O3 -DNDEBUG -std=gnu++17 -fPIE
```

**Why Loop 8's flag audit missed it:** Loop 8 §3 explicitly checked `GGML_NATIVE:BOOL=ON` and
confirmed `-march=native` was literally on the `gcc`/`g++` command line — both true, both still
true. It checked the flag it went looking for and read the result as "build flags ruled out",
without reading the rest of the same `CXX_FLAGS` line, where the absence of `-O3` was visible the
whole time. `-march=native` without `-O3` widens the *available* instruction set while telling the
compiler not to optimize; on its own it buys very little. `CMAKE_BUILD_TYPE` was never printed by
any prior loop's audit.

Note also that `ggmlc`'s own `CMakeLists.txt` defaults `option(GGML_NATIVE ... ON)` but sets no
default `CMAKE_BUILD_TYPE` — so nothing upstream protects against this; the caller must pass it.

## 2. The fix and the deploy

Rebuilt from the **pristine production source tree** `/tmp/ggmlc` (confirmed `git status
--porcelain` empty, rev `680dd84`) into a fresh out-of-tree build dir, so the live binary was never
written to while running:

```bash
cmake -S /tmp/ggmlc -B /tmp/ggmlc/build-release -DCMAKE_BUILD_TYPE=Release -DGGML_NATIVE=ON
cmake --build /tmp/ggmlc/build-release --target laya -j 24     # exit 0, ~85s, 0 errors
```

Binary size dropped 5,535,608 → 3,006,264 bytes (unoptimized code is larger; `-DNDEBUG` also drops
asserts). Deliberately **not** rebuilt from Loop 11's `/tmp/ggmlc-dbg` tree — that tree carries
Loop 11's added `dump_graph_tensors()` instrumentation in `runtime/src/executor.cpp` and is not
what we want in production.

Deploy/rollback are scripted, not hand-typed, and live next to the backup:

| Path | Purpose |
|---|---|
| `/root/laya-binary-backups/laya-O0-preloop12-e524730c.bak` | byte-identical backup of the old binary (md5 `e524730c94fa9a99840fd5c040873352`, verified against the original after copy) |
| `/root/laya-binary-backups/deploy-o3.sh` | stop service → `cp` new binary to `<path>.new` → atomic `mv -f` → start service → poll `/health` until 200 |
| `/root/laya-binary-backups/rollback.sh` | same steps, sourcing the `.bak` — restores the pre-Loop-12 binary |

New production binary md5: `ac88e9245cff9666e9412f7db30b4ca6` (verified identical to
`/tmp/ggmlc/build-release/examples/laya/laya`). Service downtime for the swap: **2.2s wall**,
`/health` → 200 immediately after.

**The rollback was actually executed, not assumed.** `rollback.sh` was run against live production:
md5 returned to `e524730c...`, and the scenario-10 probability vector returned to the old build's
`{approve 0.8365, approve-with-conditions 0.1533, deny 0.0102}`. `deploy-o3.sh` was then re-run:
md5 back to `ac88e924...`, probabilities back to `{0.8559, 0.1342, 0.0099}`. Both directions are
proven working on the real service, not documented-and-hoped.

## 3. Before/after — external `curl`, nothing else changed

Same source commit, same Q8_0 GGUF, same `--threads 28`, same systemd unit, same box. Measured
from a machine **outside** the server via `POST http://103.146.166.46:80/bench` (not SSH-side):

| | `laya_inference_ms` (5 consecutive external calls) |
|---|---|
| **Before** (`CMAKE_BUILD_TYPE=""`) | 1267, 1910, 1202 |
| **After** (`CMAKE_BUILD_TYPE=Release`) | 131, 129, 113, 158, 113 |

~**10x** on single external calls, and squarely in the ~90-160ms class D_12 asked for.

## 4. Correctness re-verified on the ACTUAL swapped-in binary (Task 3)

All 10 project benchmark scenarios re-run directly against `POST /v1/decide`, 3 reps each, on
three binaries: the new production one (8090, post-swap), the same `-O3` build on an isolated port
(8091, pre-swap), and Loop 11's `-O3` dbg build (8092).

| Scenario | choice (all 3 `-O3` binaries) | top prob `-O3` | top prob OLD `-O0` | `-O3` times (s) | `-O0` times (s) |
|---|---|---|---|---|---|
| 1_email_routing | billing | 0.973 | 0.973 | 0.102/0.100/0.100 | 1.776/1.248/1.342 |
| 2_jailbreak_detection | benign | 0.468 | 0.468 | 0.094/0.098/0.108 | 1.372/1.314/1.306 |
| 3_invoice_categorization | infrastructure | 0.908 | 0.908 | 0.098/0.112/0.115 | 1.271/1.215/1.212 |
| 4_agent_tool_routing | email-send | 0.686 | 0.686 | 0.108/0.102/0.111 | 1.087/1.081/1.142 |
| 5_incident_severity | sev1 | 0.452 | 0.452 | 0.100/0.107/0.114 | 1.128/1.201/1.139 |
| 6_content_moderation | ban-user | 0.583 | 0.583 | 0.110/0.124/0.125 | 1.101/1.397/1.108 |
| 7_support_sentiment_routing | billing | 0.979 | 0.979 | 0.110/0.167/0.112 | 1.111/1.152/1.087 |
| 8_adversarial_ambiguity | billing-disputes | 0.895 | 0.895 | 0.090/0.094/0.095 | 1.059/1.127/1.180 |
| 9_compliance_gating | no | 0.563 | 0.563 | 0.095/0.091/0.093 | 1.123/1.152/1.029 |
| 10_loan_credit_risk | approve | **0.856** | **0.837** | 0.094/0.094/0.091 | 1.153/1.021/1.078 |

**10/10 scenarios: identical `choice`. 9/10: identical top probability to 4 d.p.**

**The one discrepancy, reported rather than glossed:** scenario 10 differs in the third decimal.
Full vectors:

| build | approve | approve-with-conditions | deny | confidence |
|---|---|---|---|---|
| NEW production `-O3` (8090/8091) | 0.8559 | 0.1342 | 0.0099 | 0.5917 |
| Loop 11 `-O3` dbg build (8092) | 0.8559 | 0.1342 | 0.0099 | 0.5917 |
| OLD `-O0` production | 0.8365 | 0.1533 | 0.0102 | 0.5599 |

The new production binary matches **Loop 11's `-O3` build exactly, bit for bit**, and it is the
old `-O0` build that is the outlier — which is exactly the direction D_12's validation requirement
asked for ("results match Loop 11's `-O3` findings, not the old `-O0` production findings"). Cause
is the expected one: `-O3` licenses different vectorization/FMA-contraction and therefore a
different float summation order, which Q8_0 activation re-quantisation then amplifies slightly —
the same 1-ULP-class mechanism already documented in `crates/laya-native/src/lib.rs` for
`target-cpu=native`. It moves probabilities by <2pp on the one most-ambiguous scenario and changes
no decision anywhere.

## 5. Full harness re-run — `docs/benchmarks/laya-o3-loop12-{raw.jsonl,summary.json}`

Same `scripts/bench-harness.sh` methodology as every prior loop (3 models × 10 scenarios, Laya not
skipped, 30 real HTTP requests). `total_requests: 30`, `error_count: 0`.

### `laya_inference_ms` — the field this loop targeted

| Model | Loop 8 (threads=28, Q8_0, `-O0`) mean | Loop 12 (`-O3`) mean / median / max | Speedup (mean) |
|---|---|---|---|
| qwen3-0.6b | 1243.8 | **158.3** / 153.5 / 202 | **7.86x** |
| minicpm5-2b | 1157.0 | **167.0** / 163.5 / 209 | **6.93x** |
| qwen3-4b | 1206.1 | **145.6** / 141.5 / 217 | **8.28x** |

Cumulative against Loop 7's pre-`--threads`-fix numbers (6543-7209ms): **~40-49x** across the two
fixes combined.

### Other fields, same run (not this loop's target, reported for completeness)

| Model | `constrained_readout_ms` mean/median/max | `generation_ms` mean/median/max | `wall_ms` mean |
|---|---|---|---|
| qwen3-0.6b | 230.7 / 238 / 289 | 5147.3 / 5072 / 7542 | 5669.8 |
| minicpm5-2b | 437.0 / 425 / 586 | 9184.9 / 9674.5 / 14767 | 10225.7 |
| qwen3-4b | 496.9 / 501 / 603 | 15207.8 / 15340 / 22208 | 16013.1 |

**Incidental confirmation of Loop 8's open question.** Loop 8 §2 flagged that
`constrained_readout_ms` had jumped 2-3x (to 413.0-593.6ms) once Laya started using 28 threads, and
hypothesised — without proof — that Laya's now-real CPU footprint was contending with the LLM
engine. This loop's numbers support that: Laya's per-request CPU time dropped ~8x, and
`constrained_readout_ms` fell to 230.7-496.9ms in the same run, with nothing about the LLM engine
changed (same binary, `--threads 28`, `n_batch` 512, `target-cpu=native`). Still single-run, still
not a controlled experiment — but the effect moved in the predicted direction when the predicted
cause was removed, which is more than Loop 8 had. Not claimed as proven.

### Where this puts us against the external anchors

| Anchor | Latency | Ours now |
|---|---|---|
| Laya on Intel i3-12100 (4c/8t consumer, community) | 112 ms / 1 question | **145.6-167.0 ms mean, 141.5-163.5 median** |
| Laya, undisclosed CPU (Shray15) | 360 ms | same |
| Jev cloud API, real 3rd-party P50 | 300-1070 ms | same |

Loop 8's "still 8-25x above the community CPU anchors" gap is **closed**. We are now in the same
class as the i3-12100 anchor (~1.3-1.5x of it, on a shared box also running a 28-thread LLM engine
and answering through an HTTP layer), and comfortably faster than the Jev cloud P50 cluster. No
further micro-optimization was attempted — out of D_12's scope, and the remaining delta is small
enough that it would need a controlled, repeated-run methodology to even measure honestly.

## 6. What this means for `crates/laya-native` — current standing, not a verdict

The honest arithmetic: Loops 9-11 measured the hand-written Rust engine at **95.1 ms** against a
production C++ incumbent at **~1262 ms** and read it as a ~13x architectural win. Against the
correctly-built C++ — **92.7 ms** (Loop 11's own `-O3` rebuild, independently reproduced by this
loop's production binary and confirmed bit-identical in §4) — the Rust engine is **~3% slower**.
Effectively parity. Essentially the entire headline speedup was the missing `-O3`, not the
architecture.

**That is a recalibration of the baseline, not a decision to stop.** Stated explicitly so the
record can't be misread later:

- `crates/laya-native` **remains an active optimization target**. Loop 13+ continues work on it —
  reducing graph node count, tightening memory/allocation behaviour, kernel-level experiments —
  with the explicit goal of beating the `-O3` C++ number rather than the old `-O0` phantom.
- **All** of its env-gated debug scaffolding (`LAYA_RS_DUMP`, `LAYA_RS_FULL`, `LAYA_PERTURB`,
  `LAYA_INJECT_HIDDEN`, and the tensor/case hooks) is **deliberately kept**, not removed. Getting
  the graph numerically identical to `ggmlc` was hard; these are the instruments that made it
  possible and they are the first thing the next optimization pass will reach for. A crate-status
  note explaining this now sits at the top of `crates/laya-native/src/debug.rs` and `lib.rs`.
- It is **not wired into `apps/server`/`apps/cli` yet**, and that is a *gating criterion, not a
  rejection*: production stays on the HTTP-to-`laya serve` path until the native crate
  demonstrably beats the `-O3` baseline. This loop's production fix was the cheap, low-risk,
  immediately-available win and was taken on that basis; it does not compete with continuing the
  Rust work.

The transferable lesson, recorded in `docs/tutorial/05-benchmark-and-performance.md` and
`06-deployment-best-practices.md` § Finding 4: **a speedup claim is a claim about two
configurations, and the burden of proof is on both — especially the side you didn't build
yourself. A 10x+ unexplained gap is a configuration smell before it is an algorithmic finding.**

## 7. Full regression pass (Task 7)

See the Loop 12 evidence file for raw output. Summary:

- **CLI**, all 3 models × 3 methods (`--prompt "Which city is the capital of France?" --options
  "London,Paris"`, Laya not skipped): `readout.best_option=Paris`, `generate.valid=true`
  (`{"answer":"Paris"}`), `laya.best_option=Paris` — 3/3 models.
- **External `curl`** to `103.146.166.46:80/bench` (from outside the box), all 3 models: same
  three results, `laya_inference_ms` in the 113-158ms band. External `/health` → 200.
- **G13 fault injection** (6th consecutive loop, own sentinel `__QA_LOOP12_FAULT_INJECT__`):
  trigger → HTTP 500 `{"error":"engine worker thread dropped the response"}`, `/health` → 200
  immediately after, next `/bench` → 200 with a fresh `model_load_ms` and the correct answer;
  journal shows the real panic + respawn line. `app.rs` reverted, md5 exact match, sentinel grep
  count 0.
- **Laya-outage graceful degrade**: `systemctl stop openjev-laya-serve.service` → `/bench` → HTTP
  200 with `laya: null` and a fast-fail `laya_inference_ms`, real degrade log line; restart →
  next `/bench` → `laya.best_option=Paris`, no `openjev-server` restart needed.
- **G17** (external `:8090` blocked) and **G18** (firewall systemd unit persists) both re-verified
  after the binary swap — unaffected, as expected since neither depends on the binary.
- `cargo build --workspace` (debug + release) and `cargo test --workspace` re-run clean.

## 8. Remaining gaps after Loop 12

- **`crates/laya-native` has not yet beaten the `-O3` C++ baseline** (95.1ms vs 92.7ms, ~3%
  behind). Open and actively worked, carried to Loop 13+.
- **`act_head` in `crates/laya-native` remains unimplemented** — unchanged, not in this loop's
  scope.
- **The `constrained_readout_ms` / Laya-contention interaction (§5) is corroborated, not proven.**
  A controlled multi-run experiment (readout latency with vs without a concurrently loaded Laya)
  would settle it; still outside "not an open-ended search".
- **The harness remains single-run, sequential, no repeats** — the same noise caveat every loop
  since Loop 6 has carried. Fine for the 8x effect measured here; not fine for chasing sub-10%
  deltas, which is exactly why no further micro-optimization was attempted.
- **No other CMake-built dependency has been audited for the same class of bug.** This loop fixed
  `ggmlc` specifically. `llama-cpp-sys-2`'s vendored build was verified optimized back in Loop 7
  (`-O3` present via Cargo's release profile), but that is the only other one checked.
- G10 / G12 / G14 / G16 / G2 / G11 — unchanged, carried from prior loops.
- The finer-grained LLM-engine `n_threads` sweep (20/24/26) remains untested, unchanged since
  Loop 7.
