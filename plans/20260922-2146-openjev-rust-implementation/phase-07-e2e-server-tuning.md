---
phase: 7
name: E2E Test + Performance Tuning on Real Linux Server
status: pending
depends_on: [1, 2, 3, 4, 5]
---

## Context Links
- `../plan.md` § Decisions Locked (#7, #8, #9), § Architecture (`timing` crate), § Validation
  Summary (SSH gate pre-approved for this dev server)
- `phase-01-engine-model-management.md` (`EngineConfig.n_threads`/`batch_size`, `models::laya`)
- `phase-04-cli-bench.md`, `phase-05-http-api-serve.md` (the two binaries this phase deploys and
  runs)
- `phase-06-test-suite-ci.md` (parallel phase, disjoint files, no dependency between them)

## Overview
- Priority: P1, parallel with Phase 6 (disjoint files), both gated on Phases 1-5 being complete.
- Status: pending.
- Deploy the built `openjev-cli`/`openjev-server` binaries to a real Linux server over SSH, confirm
  all 3 LLM models AND Laya run a full round-trip successfully, then run an iterative
  benchmark→tune→benchmark loop to find the fastest configuration on that hardware, producing a
  runbook + a final results/recommendation report covering all 4 models × 3 comparison methods
  (readout, generation, Laya).

## Key Insights
- **Environment change vs Phases 0-6**: those phases build/test on the Windows dev machine. This
  phase's target is a **Linux server**: Intel Xeon Gold 5320, 32 vCPU, 62GB RAM (60GB free), 4GB
  swap (unused), 295GB disk (276GB free). RAM/disk are not constraints for any of the 4 models
  (largest quantized GGUF is a few GB) — the user has explicitly authorized using the full server
  resources to reach the best achievable performance; no resource conservatism required.
- Linux build of `llama-cpp-2`/`llama-cpp-sys-2` (and `ggmlc`, Laya's runtime — same CMake/C++17
  toolchain family) is expected to be simpler than Windows (gcc/clang + cmake via `apt`, no
  MSVC/vcpkg per plan.md Decisions Locked #7) — this was NOT separately deep-researched for either
  toolchain; if a real build problem surfaces on the server, resolve it here with direct
  investigation (read the actual error), don't block on pre-emptive research.
- The `Timings` struct (`crates/timing`, Phase 1) is the objective comparison instrument across
  tuning iterations — every benchmark run in this phase's loop must be captured through
  `openjev-cli --format json` (or the HTTP `POST /bench` equivalent) output, not eyeballed
  wall-clock. This includes Laya's `laya_model_load`/`laya_inference` fields.
- Tuning variables to sweep (not exhaustive — stop once returns clearly diminish):
  1. `n_threads` — try values around 16, 24, 32 (via `--threads`/`threads` knob, LLM engine only —
     Laya has no equivalent knob).
  2. CPU-native build flags — check `/proc/cpuinfo` on the server for AVX2/AVX512/FMA support,
     rebuild `llama-cpp-2`'s (and `ggmlc`'s) native dependency with `-march=native` (or the
     equivalent CMake flag each build system exposes) if supported, to use the CPU's full
     instruction set.
  3. Batch size for constrained-readout (via `EngineConfig.batch_size`).
  4. The `openmp` Cargo feature on `llama-cpp-2`/`llama-cpp-sys-2` (R1 §3 confirmed this feature
     exists) if available/working on the Linux build.
  5. Laya has no tunable knobs of its own in v1 (single subprocess call per score) — its "tuning"
     is limited to confirming it runs and recording its baseline CPU latency; do not invent tuning
     parameters that don't exist.
- This is fundamentally a manual, SSH-driven operational exercise with a documented runbook — not
  an automated CI test. Treat it accordingly in the Test Strategy section below (deviates from the
  standard unit/integration/e2e pyramid used elsewhere in this plan).
- **SSH execution gate**: per the `/vk-plan-validate` interview (plan.md § Validation Summary),
  this is a confirmed dev/test server under the user's own infrastructure — **no per-session or
  per-command confirmation gate is required** before running SSH commands within this phase's
  documented scope (build/run/benchmark this project, read `/proc/cpuinfo`, adjust this project's
  own build flags). Normal safety judgment still applies: no destructive/irreversible operations
  outside that scope without flagging to the user first, and no real IP/hostname/credentials
  written into this plan or the runbook — use the placeholder `target server (SSH, provided by
  user at execution time)` throughout, since the connection details are supplied out-of-band at
  execution time regardless of the confirmation-gate question.

## Requirements
### Functional
- All 4 models (Qwen3-0.6B, MiniCPM "2B", Qwen "4B" — all 3 via `openjev-cli`/`openjev-server`'s
  readout+generation paths — and Laya-en via the same binaries' Laya path) load and complete a
  full round-trip successfully on the Linux server.
- At least one full tune iteration is recorded: baseline benchmark → configuration change →
  re-benchmark → documented before/after `Timings` comparison, for each of the 3 LLM models. Laya's
  baseline is recorded (no tuning variables to sweep for it, per Key Insights).
- A runbook document and a final results/recommendation report are produced (see Related Code
  Files) — not just ad-hoc terminal output. The results report includes a 3-methods × 3-LLM-models
  comparison table PLUS Laya's own baseline row (4 models × up-to-3-methods-each, Laya only has 1
  method).

### Non-functional
- **Safety**: this phase's real execution (not plan authoring) touches a live server via SSH as
  root, on the user's own confirmed dev/test infrastructure (plan.md § Validation Summary — no
  per-command confirmation gate required for this specific server). Do not hardcode the server's
  real IP/hostname/credentials repeatedly through this plan or the runbook — use the placeholder
  `target server (SSH, provided by user at execution time)` throughout regardless.
- No destructive operations on the server beyond what's needed to build/run/benchmark this project
  (no wiping unrelated data, no altering unrelated system config) — flag to the user before any
  action outside "build the project, run the binaries, read `/proc/cpuinfo`, adjust this project's
  own config/build flags," even though no per-command approval gate is required for in-scope
  actions.

## Architecture
This phase produces operational artifacts, not new crate/app code:
```
docs/benchmarks/server-tuning-runbook.md   -- step-by-step: SSH deploy, build, run loop, record results
docs/benchmarks/server-tuning-results.md   -- final report: recommended config + latency per model per method
```
The runbook drives `openjev-cli --threads N --batch-size M --format json` (LLM methods) and the
same binary's Laya path (no extra flags needed beyond the default-on behavior, unless
`--skip-laya` is used to isolate a run), or the `openjev-server` HTTP equivalent (`POST /bench`
with `threads`/`batch_size`/`skip_laya` fields) — no new product code is introduced by this phase;
it exercises what Phases 1-5 already built.

## Related Code Files
- CREATE `docs/benchmarks/server-tuning-runbook.md` — procedure: SSH connect (placeholder host) →
  install Rust/CMake/build deps via `apt` (for both `llama-cpp-2` and `ggmlc`) → clone/copy the
  repo → `cargo build --workspace --release` → clone+build `github.com/monatis/ggmlc` for
  `ggmlc-run` → download all 4 models (via the binaries' own `hf-hub` download path or manually) →
  run `openjev-cli` for each of the 3 LLM models at default config (baseline, Laya included by
  default) → sweep `n_threads`/batch size/native-CPU-flags/`openmp` per Key Insights (LLM models
  only) → record each run's `Timings` JSON, including Laya's fields.
- CREATE `docs/benchmarks/server-tuning-results.md` — final deliverable: table of configurations
  tried, `Timings` per LLM model per method (readout/generation), Laya's baseline `Timings`, and
  the recommended production configuration for this specific hardware (32-vCPU Xeon Gold 5320).
- No `crates/**`/`apps/**` changes expected; if the tuning loop reveals a genuine product bug (e.g.
  `n_threads` override not actually reaching `LlamaContextParams`, or a Laya parsing edge case),
  file it as a fix against the relevant Phase 1/2/4/5 file and re-verify — do not silently patch
  around it only in the runbook.

## Implementation Steps
1. Confirm Phases 1-5 are complete and `cargo build --workspace --release` (Windows) succeeds
   locally as a pre-check that the codebase itself is sound before involving the remote server.
2. SSH to the target server (connection details supplied by user at this point, not written into
   plan/runbook files — no per-command confirmation gate required per plan.md § Validation
   Summary), confirm OS/distro, install build prerequisites (`build-essential`/`gcc`/`clang`,
   `cmake`, `git`, Rust toolchain via `rustup`, matching `rust-toolchain.toml`).
3. Deploy the project source to the server (git clone, or copy), `cargo build --workspace
   --release`. If the Linux build fails, diagnose directly (read the actual compiler/linker error)
   — this is the "resolved live" moment flagged in Key Insights, not a pre-researched risk. Then
   clone `github.com/monatis/ggmlc` and build `ggmlc-run` on the server the same way.
4. Run `./target/release/openjev-cli --model <each of 3> --prompt ... --options ... --format json`
   at default `EngineConfig` (no `--threads`/`--batch-size` override, Laya included by default) for
   each of the 3 LLM models — this is both the "all 4 models start successfully" check (3 LLM
   round-trips + the Laya path each of those 3 runs also exercises) and the tuning loop's baseline.
5. Check `/proc/cpuinfo` for AVX2/AVX512/FMA support; if supported and not already the default,
   rebuild with `-march=native` (or `llama-cpp-sys-2`'s/`ggmlc`'s equivalent build flag) and re-run
   step 4's benchmarks to compare — note explicitly which binary(ies) the flag was applied to.
6. Sweep `--threads` across ~16/24/32 (and any other value showing a clear trend) for at least one
   LLM model per pipeline; sweep `--batch-size` for the constrained-readout pipeline; try the
   `openmp` feature if buildable; record every run's full `Timings` JSON (Laya's fields stay
   constant across these sweeps since it has no equivalent knobs — record its baseline once,
   re-confirm it's still working after each rebuild).
7. Pick the best-performing configuration per Key Insights' stopping rule (diminishing returns);
   re-run all 3 LLM models (+ Laya, still default-on) with that final configuration as a
   confirmation pass.
8. Write `server-tuning-runbook.md` (the reusable procedure, generalized — no real IP/credentials)
   and `server-tuning-results.md` (the actual numbers + recommendation from this run, including the
   4-model comparison table).

## Todo List
- [ ] Build prerequisites installed on server (Rust, CMake, C++17 compiler), `cargo build
      --workspace --release` succeeds on Linux
- [ ] `ggmlc-run` built successfully on the server
- [ ] All 3 LLM models complete a full round-trip (readout + generation) on the server
- [ ] Laya completes a successful scoring round-trip on the server
- [ ] Baseline `Timings` recorded for all 3 LLM models + Laya at default config
- [ ] At least one tuning variable swept with before/after `Timings` comparison recorded (LLM
      models)
- [ ] Final confirmation pass with chosen configuration, all 3 LLM models + Laya
- [ ] `docs/benchmarks/server-tuning-runbook.md` written (no real credentials/IP committed)
- [ ] `docs/benchmarks/server-tuning-results.md` written (final numbers + recommendation, incl.
      Laya row)

## Success Criteria
- (a) All 3 LLM models AND Laya start and complete a full round-trip successfully on the real
  Linux server.
- (b) At least one tuning iteration is recorded with an objective before/after `Timings` comparison
  for the LLM models (not a subjective "felt faster" claim); Laya's baseline is recorded even
  though it has no tuning variables of its own.
- (c) `server-tuning-results.md` states a concrete recommended configuration (n_threads, batch
  size, build flags, openmp on/off) for production use on this specific hardware, backed by the
  recorded numbers, and includes Laya's CPU baseline latency alongside the 3 LLM models' numbers
  for the full comparison the user asked for.

## Test Strategy & Quality Gate
Lane: normal, but this phase is explicitly a **manual, guided e2e/performance validation via a
documented runbook — not an automated unit-test suite**, and is exempted from the standard
unit/integration/e2e pyramid + coverage-percentage format used in Phases 1-6. This deviation is
intentional, not an omission:
- Spec: Goal = prove the product works end-to-end on real target hardware (all 3 comparison
  methods across all 4 models) and establish a performance-tuned production configuration.
  Acceptance Criteria = the 3 Success Criteria above (Given the deployed binaries on the target
  server, When each of the 3 LLM models is benchmarked, Then a full round-trip completes and a
  `Timings` report is produced; Given the same run, When Laya is included (default), Then it also
  completes and reports; Given a baseline config, When at least one tuning variable is changed for
  an LLM model, Then a recorded before/after comparison exists). I/O contract = the runbook's
  documented CLI/HTTP invocations and their JSON `Timings` output (already specified in Phase 4/5).
  Out-of-scope: automated regression testing of performance over time (no perf-CI is being set up
  here — that would be new unrequested scope; this is a one-time tuning exercise producing a
  recommendation document).
- No unit/integration/e2e pyramid ratio or coverage percentage applies — the "test" IS the guided
  runbook execution itself, and its evidence is the `Timings` JSON output captured at each step
  plus the final results document.
- Evidence to produce at phase close: the full sequence of `Timings` JSON blobs from every
  benchmark run in the loop (pasted into `server-tuning-results.md`), plus the runbook document
  itself as the reusable procedure.

## Risk Assessment
- Risk: SSH/root access to a real server is inherently higher-stakes than any other phase in this
  plan — mitigated by scoping all server-side actions to "build/run/benchmark this project" only
  and flagging anything outside that scope to the user, even though the per-command confirmation
  gate itself was lifted for this specific pre-approved dev server (plan.md § Validation Summary).
- Risk: Linux-specific build issues with `llama-cpp-2`/`llama-cpp-sys-2`/`ggmlc` were not
  pre-researched (Key Insights) — if a nontrivial issue surfaces, it may extend this phase's
  timeline; document the actual fix in `server-tuning-runbook.md` so it's not re-discovered on a
  future deploy.
- Risk: `-march=native` binaries are not portable to different CPU hardware — note explicitly in
  `server-tuning-results.md` that a native-flag-optimized build is tied to this specific Xeon Gold
  5320 target and would need rebuilding (not just redeploying) on different hardware.
- Risk: Laya's actual CPU latency turns out much higher than the GPU-only published numbers would
  suggest (no CPU baseline exists anywhere prior to Phase 0's quick sanity check) — this is
  informational, not a blocker; record it faithfully in the results report regardless of how it
  compares to the 3 LLM methods.

## Security Considerations
- SSH credentials/host details are never written into the plan or runbook files — placeholder text
  only (`target server (SSH, provided by user at execution time)`).
- No new network-facing surface beyond what Phase 5 already defined (this phase just deploys and
  exercises it); confirm the server's exposure of the HTTP port (if `openjev-server` is tested
  here) is intentional and scoped by the user, not left open beyond the tuning session.

## Next Steps
- Feeds the recommended production configuration back into any deployment documentation the user
  wants later (out of scope for this plan — not requested); this phase's deliverables
  (`server-tuning-runbook.md`, `server-tuning-results.md`) are the durable record.
