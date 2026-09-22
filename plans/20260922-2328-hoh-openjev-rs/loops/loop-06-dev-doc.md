---
loop: 6
status: pending
preservation_constraints:
  - "openjev-server (port 80) and laya serve (port 8090) both stay live and curl-reachable throughout (brief restarts for config changes OK, must end reachable)"
  - "All 3 LLM models + Laya still correct via apps/cli and apps/server"
  - "reset_context / G13 respawn-supervisor / graceful Laya-outage degradation all still functional"
  - "cargo build --workspace (debug+release) and cargo test --workspace still pass"
  - "6-crate structure, Timings 7 fields, 4 cached models intact"
---

## Objective
Real benchmark harness (10 diverse scenarios) + first tuning pass (n_threads) with recorded before/after.

DoD's last open item: "≥1 real performance-tuning iteration recorded (before/after Timings),
converging toward fastest measured config on the 32-vCPU hardware." Build a proper benchmark
harness first (replacing ad-hoc single "capital of France" curls with the 10 real scenarios
already researched), establish a baseline, then run at least one genuine tuning iteration
(varying `n_threads`) with objective before/after numbers. This loop does NOT need to find the
final optimal config — it needs to prove the tuning METHODOLOGY works with real evidence; Loop
7+ continues iterating (build flags, batch size, more thread values) toward convergence.

## Tasks
1. **Build a benchmark harness**: a small script or Rust helper (your choice — a bash script
   looping `curl` calls + `jq`/parsing is simplest and doesn't require new Rust code; a
   dedicated `apps/cli` subcommand is more integrated but more work — pick pragmatically) that
   runs ALL 10 scenarios from
   `plans/20260922-2146-openjev-rust-implementation/research/researcher-05-benchmark-usecases.md`
   against `apps/server`'s `/bench` endpoint, for EACH of the 3 LLM models (Laya runs
   automatically since it's not skipped by default) — that's 30 requests per full run. Capture
   each response's full `timings` object. Aggregate: mean/median/max per timing field, per
   model, across the 10 scenarios.
2. **Establish baseline**: run the harness once with the CURRENT config (whatever `n_threads`
   `EngineConfig::default()` currently sets — check `crates/engine/src/lib.rs`, Loop 4's report
   mentioned `28` on a 32-core box). Save the raw results + aggregated summary to
   `docs/benchmarks/baseline-<timestamp>.json` (or `.md`, your choice of format, just make it
   inspectable).
3. **Run ≥1 real tuning iteration on `n_threads`**: the current default leaves 4 cores of
   headroom (28/32). Test at least 2 alternative values (e.g. `n_threads=32` — use every core,
   no headroom reserved for the OS/other processes — and one more conservative value like
   `n_threads=16` or `24` for comparison) against the SAME 10-scenario harness. This requires
   either (a) exposing `n_threads` as a server startup flag (`openjev-server --threads N`) if
   not already wired end-to-end from CLI args to `EngineConfig` at server-start time — check
   Loop 2/4's work, `EngineConfig` fields exist but confirm the server's `AppState`/worker
   actually threads a configurable value through, not just the CLI's per-request override which
   D_3 mentioned; wire it if missing — or (b) restart the server between each config with a
   different compiled-in default if flag-wiring is more work than it's worth (document which
   approach you took and why). Restart the server for each config, rerun the harness, save each
   run's results similarly to Task 2.
4. **Write `docs/benchmarks/server-tuning-results.md`**: summarize the baseline vs. each tested
   `n_threads` value — a table of mean/median timings per phase per model, which config won
   (and by how much), and a concrete interim recommendation (this is a checkpoint, not final —
   say so explicitly, note remaining variables to try in Loop 7: build-flags/native-CPU-opts,
   batch_size, `openmp` feature).
5. **Leave the server running on whichever config performed best** from this loop's testing
   (don't leave it on a deliberately-worse config just because it was tested last).
6. **Quick G17 mitigation if trivial**: check if a simple `iptables`/`ufw`-equivalent rule can
   restrict inbound `:8090` to `127.0.0.1` only (the server's own loopback) without needing to
   modify the `laya` binary itself. If a working local firewall tool exists and this is a
   5-minute fix, do it and verify `laya serve` is no longer reachable from outside while still
   reachable from `openjev-server` on the same box. If it turns out non-trivial (e.g. genuinely
   requires the upstream/hypervisor firewall console you don't have access to, per Loop 3's G14
   finding), don't burn the loop's time budget on it — document the attempt and move on; this
   is NOT the loop's primary objective.
7. **Fix the false doc claim** flagged in G17 (README.md says `laya serve` binds
   `127.0.0.1`-only — it doesn't, the binary has no `--host` flag) — one-line correction,
   regardless of whether Task 6's mitigation succeeds.

## Preservation
See frontmatter.

## Validation Requirements
- Given the benchmark harness, When run against the live server, Then it produces real
  per-scenario, per-model timing data for all 3 LLM models × 10 scenarios (30 real requests,
  not simulated).
- Given ≥2 different `n_threads` configs tested with the SAME harness, When compared, Then
  `docs/benchmarks/server-tuning-results.md` shows real before/after numbers (not estimates)
  with a stated winner and margin.
- Given the loop ends, When `curl http://103.146.166.46:80/health` and a `/bench` call are
  made, Then the server is reachable and using the best-performing tested config.
- Given the G17 doc fix, When `README.md` is read, Then it no longer falsely claims
  `127.0.0.1`-only binding for `laya serve`.

## Out-of-scope
- Full convergence to a truly optimal config — this is an interim checkpoint, Loop 7+
  continues (build flags, batch_size, openmp).
- The Jev-compatible `/v1/systemone` endpoint on our own `apps/server` — still a later/optional
  loop, not required by the DoD's literal wording.
- G10/G12/G14/G16 — not this loop's focus unless trivially foldable.
