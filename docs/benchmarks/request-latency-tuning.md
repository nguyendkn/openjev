# Request-latency tuning — method selection + Laya parallelization (Loop 21, 2026-09-23)

Answers "response time still feels slow" with real production evidence, then two shipped
fixes: opt-in method selection (skip engine work you don't need) and Laya/LLM parallelization
(stop paying Laya's round-trip serially). All numbers below are real `/bench` calls against
103.146.166.46 (Xeon Gold 5320, 32 vCPU, no GPU), not projected.

## Root cause (Task 1: real perf-log evidence, not one synthetic sample)

`scripts/analyze-perf-logs.sh 0` over 63 real log files / 148 real `server::run_bench` calls
(mixed traffic across Loops 18-21, all 3 models):

| span | count | p50 | mean | max |
|---|---|---|---|---|
| `pipeline::run_readout` | 149 | 679ms | 1018ms | 5.8s |
| `pipeline::run_generate` | 148 | 3685ms | 6227ms | 43.6s |
| `models::laya::score` | 75 | 138ms | 836ms | 7.8s |

`server::run_bench`'s own total (mean 9187ms) ≈ the sum of the three above — confirms the
sequential-execution hypothesis across real traffic, not just the one-sample spot-check. A
caller wanting only `laya` (~110-150ms typical) or only `readout` (~300-700ms typical) was
paying `generate`'s multi-second tax for nothing.

## Task 2: opt-in method selection

`BenchRequest.methods: Option<Vec<String>>` (any of `"readout"`/`"generate"`/`"laya"`).
`None` (field omitted) = all 3, byte-identical to pre-Loop-21 behavior — regression-diffed
against the pre-Loop-21 binary running side-by-side on a scratch port, `readout`/`generate`/
`model`/`prompt`/`options` fields matched exactly, full JSON key shape matched exactly.
`readout`/`generate` response fields are now `Option<T>` (were bare `T`) — serializes
identically to before when populated, `null` when skipped, same pattern `laya` already used.
Engine work is actually skipped, not hidden: a `methods:["laya"]` request never loads the
model, never tokenizes, never decodes.

Real measured (warm model, `1_email_routing` scenario, qwen3-0.6b):

| call | wall time (3 reps) | what ran |
|---|---|---|
| `methods:["laya"]` | 123-136ms | laya HTTP call only |
| `methods:["readout"]` | 356-357ms | readout decode only |
| default (all 3) | 4971-5456ms | all 3, parallelized (Task 3) |

Invalid entries (`methods:["bogus"]`) and an empty/all-excluded list both return `400`
(`InvalidOptions`) rather than silently running nothing. `skip_laya:true` still wins over a
`methods` list naming `"laya"` (one authoritative switch, not two that can disagree).

`apps/cli` got symmetric `--skip-readout`/`--skip-generate` flags (existing `--skip-laya`
pattern); `CliOutput.readout`/`.generate` also became `Option<T>`.

## Task 3: Laya/LLM parallelization

`run_laya` (a plain synchronous, stateless HTTP client call — confirmed not already async) is
now spawned on its own `std::thread` at the very start of `run_bench`, before any engine work,
and joined only after `generate` finishes. Confirmed safe under G13: `Engine` (the only `!Send`
state) never leaves its worker's home thread; the spawned thread only ever touches
`prompt`/`options` clones and the stateless Laya HTTP client. `laya_inference_ms` is measured
inside the spawned closure around the call itself (not spawn-to-join wall time), so it still
reports the real HTTP round-trip duration, not however long readout+generate happened to take.

Real measured savings (isolated scratch instance, single worker, warm model, same scenario,
back-to-back with an isolated pre-Loop-21 instance for a controlled comparison — the shared
production box has real concurrent traffic that makes plain wall-clock diffs across separate
runs noisy, so the reliable measurement is within a single parallelized request: `wall_ms` vs
`readout_ms + generate_ms + laya_ms + ~130ms` measured HTTP/process overhead):

| rep | readout | generate | laya | predicted-if-sequential | actual wall | savings |
|---|---|---|---|---|---|---|
| cold | 307 | 4287 | 125 | 5734+130=5864 (incl. model_load 934, warmup 81) | 5740 | ~124ms |
| warm | 538 | 4312 | 655 | 5635 | 4987 | ~648ms |
| warm | 597 | 5058 | 1064 | 6849 | 5790 | ~1059ms |

Savings track `laya_inference_ms` closely in every rep — Laya's cost is essentially fully
hidden behind the much-longer `generate` step. The full post-deploy 30-request regression
(10 scenarios x 3 models, default/all-3-methods) shows the same pattern in real mixed
production conditions: qwen3-0.6b saved ~561ms, qwen3-4b ~250ms, minicpm5-2b ~422ms
(`sum(readout+generate+laya) - wall_ms`, per-model means, see `loop21-post-deploy-summary.json`).
No G13/degradation regression (drilled separately, see below).

## Task 4: `-rtrp`/online-repack evaluation — already active, no lever to pull

The research doc's `-rtrp`/`--runtime-repack` is a stale flag name. In the actual bundled
llama.cpp snapshot (`llama-cpp-sys-2` v0.1.156, matching `llama-cpp-2` v0.1.156, this
project's pinned version), that CLI flag was renamed to `--repack`/`-nr`/`--no-repack` and
lives in `common/arg.cpp` (`common_params`), which only the `llama-cli`/`llama-server`
example binaries consume — **not** part of the core `llama.h` C API that `llama-cpp-2`'s Rust
bindings wrap, so there's no CLI-flag-shaped lever available to this project even in
principle.

The real underlying mechanism is `llama_model_params.use_extra_bufts` (`llama.h:347`,
"use extra buffer types (used for weight repacking)"). Its C-level default,
`llama_model_default_params()` (`llama-model.cpp:2802`), is **`true`**. Confirmed by direct
source read that `llama-cpp-2`'s `LlamaModelParams::default()` calls straight through to that
C function and does not override the field — and `crates/engine/src/lib.rs:111`
(`LlamaModelParams::default().with_n_gpu_layers(0)`) never touches it either. **Weight
repacking is already on, in production, for every model this project runs, and has been since
whichever loop the current `llama-cpp-2` version was pinned** — there was nothing to turn on
this loop, and no code change was possible or needed.

Applicability confirmed for all 3 models' actual quant formats: `ggml-cpu/repack.cpp` has
registered AVX-512 tile-repack kernels for `Q8_0` (qwen3-0.6b), `Q5_K` (qwen3-4b), and `Q4_K`
(minicpm5-2b's underlying block type; the model ships as `Q4_K_M`). Hardware confirmed via
`/proc/cpuinfo`: `avx512f/dq/cd/bw/vl` + `avx512_vnni` all present on this Xeon Gold 5320.

**Gap, honestly flagged**: `LlamaModelParams`'s underlying `params` field is `pub(crate)` in
the `llama-cpp-2` v0.1.156 crate — there is no public builder to flip `use_extra_bufts` off,
so a clean in-repo A/B ("how much would we lose without repacking") was not attempted; doing
so would require vendor-patching the pinned dependency, disproportionate scope for what this
task needed to establish (confirm-and-deploy-if-a-win). The honest finding — already on,
nothing to deploy — is itself the validated result. Real current tok/s with repacking active
(from this loop's own live regression, qwen3-0.6b): 162 tokens / 4118ms generation ≈ 39
tok/s.

## Task 5: `ik_llama.cpp` — correctly not attempted

Per the dev-doc's explicit out-of-scope, not touched this loop. Flagged (per Loop 21's
research doc §1) as the one remaining engine-level lever if the current wins aren't enough:
fork-specific hand-tuned AVX-512 kernels (+21-25% pp512 on Cascade Lake, same non-AMX AVX-512
family as this box's Ice Lake, but explicitly unverified-on-transfer per the fork author) —
real maintenance-burden tradeoff (pins off mainline `llama-cpp-2` cadence), needs the user's
explicit go-ahead as its own loop.

## Task 6: deployed

`apps/server` (Tasks 2+3) and `apps/cli` (Task 2 symmetry) rebuilt in an isolated
`CARGO_TARGET_DIR=/tmp/loop21-target`, `cargo build --workspace` (debug) and
`--release` both clean, `cargo test --workspace` all green (8 tests, 0 failures, 0 skipped
besides 1 pre-existing ignored doctest). Backup: `/root/backups/loop21/openjev-server.pre-loop21.bin`
(md5 `9f4b8d0af949cc851b8cf6c131d3767d`), deployed via atomic rename (no "text file busy"),
`systemctl restart openjev-server`, live-verified via `/health` (200) and a real `/bench` call
before declaring done. Task 4 had no code to deploy (see above).

## Task 7: full regression

- `cargo test --workspace`: all green, re-confirmed against the final deployed tree.
- 30-request harness (10 scenarios x 3 models, default/all-3-methods, the backward-compat
  path) against the live deployed server: **0 errors**, results in
  `docs/benchmarks/loop21-post-deploy-raw.jsonl` / `-summary.json`.
- Backward compatibility: default response (methods omitted) diffed field-by-field against the
  pre-Loop-21 backup binary run side-by-side on a scratch port for the same request — `readout`,
  `generate`, `model`, `prompt`, `options` matched exactly; full JSON key shape (top-level +
  `timings`/`readout`/`generate` nested keys) matched exactly.
- `methods` feature: `["laya"]` and `["readout"]` fast paths verified (see Task 2 table),
  invalid/empty `methods` correctly rejected with `400`, `skip_laya` still authoritative.
- G13 drill: sentinel-panic fault injection on an **isolated scratch copy** (`/tmp/g13drill`,
  port 8198, own `CARGO_TARGET_DIR` — never the live artifact). Triggering request got a clean
  `500` (`"engine worker 0 dropped the response"`), server process stayed alive (same PID), a
  following normal request succeeded (worker respawned, fresh model reload visible in logs).
  Confirms Task 3's `std::thread::spawn` for Laya does not interact with or weaken G13's
  per-worker `catch_unwind` supervision. Scratch copy + its target dir fully removed after.
- Laya-outage live drill: `systemctl stop openjev-laya-serve` -> default `/bench` call returns
  200 with `laya:null` (readout/generate unaffected) -> `methods:["laya"]`-only call also
  correctly returns 200 with `laya:null` (not a 500) -> `systemctl start openjev-laya-serve` ->
  recovered within ~2s (`laya_inference_ms:503` on the first post-recovery call, real answer
  returned).
- G17/G18 firewall: `openjev-laya-firewall.service` active; `iptables -L INPUT -n -v` shows
  `ACCEPT` on `lo` only + `DROP` on all other interfaces for port 8090, both rules live
  (2794 accepted loopback packets, 8 dropped external attempts logged).

## Task 8: this document

New file (`jev-comparison.md` covers a different comparison and was left alone per YAGNI/DRY —
this loop's numbers are request-latency-shape, not the Jev head-to-head).
