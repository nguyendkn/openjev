---
loop: 7
status: pending
preservation_constraints:
  - "openjev-server and laya serve both stay live and reachable throughout, ending on the best config found"
  - "All 3 LLM models + Laya still correct via apps/cli and apps/server"
  - "reset_context / G13 respawn-supervisor / Laya graceful degradation / G17 firewall rule all still functional"
  - "cargo build --workspace (debug+release) and cargo test --workspace still pass"
  - "n_threads stays at 28 (Loop 6's proven winner) unless this loop finds solid evidence otherwise"
---

## Objective
Final tuning pass (batch_size, CPU-native build flags, verify openmp) + full regression check.

This is planned as the closing optimization loop (not necessarily the literal last loop ever,
but the DoD's item 5 is already met with real evidence from Loop 6 — this loop pushes further
per the user's "performance cao nhất" framing before final wrap-up). Test the variables Loop 6
flagged as untested, using the SAME benchmark harness/10-scenario methodology for an
apples-to-apples comparison against the `n_threads=28` baseline.

## Tasks
1. **Verify `openmp` is actually active**, not just a declared default feature: check the
   build log for OpenMP compiler flags actually being passed (`-fopenmp` or similar), or check
   `OMP_NUM_THREADS`/runtime behavior. If it's declared but not actually linked/active for some
   reason, note that as a finding — don't assume the Cargo.toml feature list equals runtime
   behavior.
2. **Test CPU-native build flags**: rebuild with `RUSTFLAGS="-C target-cpu=native"` for the
   Rust side, AND check whether `llama-cpp-sys-2`'s internal CMake build of llama.cpp has an
   env var / Cargo feature to pass `-march=native`/`-DGGML_NATIVE=ON` (check its `build.rs`
   or documented env vars — don't guess the exact mechanism, verify from source/docs).
   Run the harness at `n_threads=28` with the native build vs. the current non-native build.
3. **Test 2 alternative `n_batch` values** (current default 512 — try e.g. 256 and 1024) at
   `n_threads=28`, same harness, same 10 scenarios × 3 models.
4. **Update `docs/benchmarks/server-tuning-results.md`**: append this loop's results, state
   the FINAL recommended production config (threads/batch/build-flags) for this specific
   32-vCPU hardware, backed by the accumulated evidence across Loops 6-7.
5. **Full regression pass**: run all 3 LLM models × both readout+generate, Laya included, via
   BOTH `apps/cli` and external curl to `apps/server`, confirm everything still correct on
   whatever final config this loop lands on. Also re-verify `/health`, G13 fault-injection
   (yet again — this is cheap insurance now that it's a proven repeatable test), and Laya
   outage graceful degrade one more time, since this loop may rebuild/restart multiple times.
6. **If time permits**, quick look at G18 (iptables persistence) — a simple systemd unit or
   `/etc/rc.local`-equivalent that reapplies the 2 rules on boot; if the box's init system
   doesn't make this trivial, document the gap and move on, don't burn the loop on it.

## Preservation
See frontmatter.

## Validation Requirements
- Given each tested variable (build flags, 2 batch_size values), When compared against the
  Loop 6 baseline (`n_threads=28`, default batch, non-native build) using the SAME harness,
  Then `server-tuning-results.md` shows real recorded numbers for each, with a stated
  final-recommended config.
- Given the loop ends, When `curl http://103.146.166.46:80/health` and a full `/bench` (all 3
  methods) are called, Then the server is live and correct on the final chosen config.
- Given a repeat of the G13 fault-injection test, When triggered, Then the server still
  self-heals as proven in Loops 4-6 (no regression from any rebuild this loop did).

## Out-of-scope
- Further hardware-level tuning beyond what's listed (e.g. NUMA pinning, huge pages) unless
  trivially cheap — this is meant to be a wrap-up loop, not an open-ended search.
- The Jev-compatible `/v1/systemone` endpoint — still optional/future, not DoD-required.
- G10/G12/G14/G16/G18 test-coverage/doc/persistence gaps — track, don't block on them.
