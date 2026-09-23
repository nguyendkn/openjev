---
loop: 14
candidate: A_14 (workspace + remote server root@103.146.166.46, /tmp/openjev, live production
  process `openjev-server --port 80 --workers 4 --threads 7`, PID 239004 confirmed running
  throughout this evaluation; local git working tree byte-identical to deployed
  pool.rs/app.rs/main.rs/crates/engine/src/lib.rs — confirmed via direct diff, not assumed)
status: passed
builder: Developer
assessor: QATester
verified_behaviors:
  - cargo build --workspace (debug) on the real server — exit 0
  - cargo build --workspace --release on the real server — exit 0
  - cargo test --workspace on the real server — 7/7 tests passed, 0 failed
  - production live: ps aux confirms --workers 4 --threads 7; /health 200
  - model=laya rejected with 400 + clear message (not 500)
  - crates/laya-native/tests/sequence_and_reference.rs cargo-fmt diff is formatting-only
    (byte-for-byte identical after whitespace-stripping both versions; only diffs are 3
    rustfmt trailing-commas)
  - real concurrent throughput: 3 different-model requests, independently timestamped,
    genuinely overlap (not serialized)
  - routing policy (least-inflight + model-affinity): 2 concurrent same-model requests do
    NOT bunch onto one worker
  - G13 per-worker fault injection (QA's own independent sentinel, isolated copy): panic
    confined to one worker, other worker unaffected, self-heals with empty cache
  - G15 (panic-message extraction) still correct under the new per-worker supervisor
  - shared_backend() double-checked locking reviewed — race genuinely fixed
  - full regression 3 models x 3 methods (readout/generate/laya) via external curl — all
    200, all correct
  - apps/cli spot check (qwen3-0.6b, 3 methods) unaffected by pool refactor
  - G17/G18 (Laya :8090 firewall + systemd persistence) unaffected
unresolved_gaps:
  - pool.rs/routing/supervisor logic has zero committed unit tests (extends pre-existing
    G12, apps/server has no in-repo test coverage) — all verification this loop is
    black-box/behavioral, not regression-protected in CI
  - topology-probe raw benchmark data (all 4 (N,T) combos x concurrency 1/2/4/8) lives only
    as a doc comment in pool.rs, not a separate committed report file under docs/ — data
    exists and is complete (satisfies VR#1 literally) but is harder to find/maintain than a
    docs/ file
  - live Laya-outage (killing the real `laya serve` process) was NOT re-triggered this loop
    — deliberately skipped to avoid disrupting Loop 13's concurrent dependency on the same
    process (explicit preservation constraint: "laya serve ... unaffected"); verified
    instead via git diff (Loop 14 did not touch the run_laya match-arm degrade logic in
    app.rs) + a fresh happy-path exercise (laya field populated correctly end-to-end for
    all 3 models this loop)
regressions: []
schema_valid: true
---

## Verified Behaviors

- [x] `cargo build --workspace` (debug) succeeds on the real server — evidence: SSH
  `cd /tmp/openjev && cargo build --workspace` → `Finished dev profile ... target(s) in
  1.79s`, `EXIT:0`. Only pre-existing warning (deprecated `Special` enum in
  `crates/engine/src/lib.rs:9`, unrelated to this loop).
- [x] `cargo build --workspace --release` succeeds — evidence: SSH →
  `Finished release profile [optimized] target(s) in 3.92s`, `EXIT:0`.
- [x] `cargo test --workspace` passes fully — evidence: SSH `cargo test --workspace` (300s
  timeout, completed well under it) → 4/4 in
  `crates/laya-native/tests/sequence_and_reference.rs` (including the G21 boundary test
  `more_options_than_max_opts_is_rejected_not_read_out_of_bounds`), 3/3 in
  `pipeline::readout::tests`, all other crates 0 tests (unchanged from prior loops), doc-tests
  clean. `EXIT:0`, no failures anywhere in the workspace.
- [x] Production is genuinely running the new pool architecture — evidence: `ps aux` on the
  server shows `/tmp/openjev/target/release/openjev-server --port 80 --workers 4 --threads
  7` (PID 239004, live since 10:55, still running at end of this evaluation). Binary mtime
  (10:55:51) is newer than `pool.rs` (10:48:49) and `crates/engine/src/lib.rs` (10:27:53)
  sources, consistent with a fresh build from current code.
- [x] Locally-reviewed source == deployed production source — evidence: direct `diff` of
  `apps/server/src/{pool,app,main}.rs` and `crates/engine/src/lib.rs` between the local git
  working tree and `/tmp/openjev` on the server: all four IDENTICAL, byte-for-byte. Code
  review below is of the exact code running in production, not a stale local copy.
- [x] `model=laya` returns 400 with a clear message, not 500 — evidence: external curl
  `POST /bench {"model":"laya",...}` → `HTTP:400`,
  `{"error":"unknown model 'laya' — valid models: qwen3-0.6b, qwen3-4b, minicpm5-2b. Laya
  is always included automatically in the 'laya' field..."}`.
- [x] `crates/laya-native/tests/sequence_and_reference.rs` reformat is formatting-only, no
  content lost/altered — evidence: `diff` between the remote (post-`cargo fmt --all`) file
  and the local git-tracked copy shows only line-wrap/indentation differences (rustfmt
  breaking long `assert_eq!`/`vec!`/`Question{...}` expressions across lines). Stripped ALL
  whitespace from both versions and compared byte-for-byte: only 3 bytes differ across the
  whole ~4.4KB file, all 3 being trailing commas rustfmt adds after the last element of a
  multi-line `vec![...]`/struct literal (semantically inert in Rust). Every assertion string,
  numeric literal, and test name is identical. File is still `git ls-files`-tracked locally
  (`crates/laya-native/tests/sequence_and_reference.rs`, last real commits `8e26e66`/`e418430`)
  — nothing was lost or corrupted by the untracked remote fmt pass; the local (pre-fmt)
  version remains available in git history if Loop 13 wants to sync/commit the reformatted
  version later. Also re-ran this exact test file on the server via `cargo test --workspace`
  above — all 4 tests pass post-fmt, confirming the reformat didn't break compilation or
  semantics.
- [x] Real concurrent throughput (independently measured, not trusting Loop 14's own
  numbers) — evidence: fired 3 concurrent external curl `/bench` requests (qwen3-0.6b,
  qwen3-4b, minicpm5-2b; `skip_laya:true`), all three started within 0.1ms of each other
  (`1790137130.7301/.7302/.7303`) and finished at 3.81s / 8.71s / 18.62s respectively — total
  wall time for the whole burst ≈18.7s, matching the single slowest request, NOT the ~31s sum
  of all three run serially. All 3 returned HTTP 200 with the correct answer
  (`"best_option":"B) Paris"`). This independently confirms D_14's Validation Requirement #2
  (genuinely overlapping wall-clock time, not just "didn't error").
- [x] Routing policy — least-inflight + model-affinity — does not bunch concurrent
  same-model requests onto one worker — evidence: warmed `qwen3-0.6b` on production (1
  request, `model_load_ms` charged once), then fired 2 CONCURRENT requests for the SAME
  model (`qwen3-0.6b`) at the same instant. Request A (which landed on the already-warm
  worker) returned `model_load_ms:0`, total 3.61s. Request B, dispatched at the same moment,
  returned `model_load_ms:992` (a FRESH load on a different, previously-idle worker) and both
  completed within an overlapping ~4.65s window (not serialized ~8.2s back-to-back). This
  confirms the documented "busy pool" behavior: a second concurrent request for a model
  whose home worker is momentarily busy is routed to an idle worker instead of queueing
  behind it, and is NOT bunched onto the same worker just because that worker has the model
  cached.
- [x] G13 per-worker fault injection — QA's own independent test, separate sentinel from the
  Developer's (which was reverted from A_t; this test never touched A_t) — evidence: copied
  `/tmp/openjev` (excluding `target/`) to a scratch dir `/tmp/qa14-fault-copy/` (deleted after
  the test, A_t never modified), added one 3-line panic trigger keyed off a private sentinel
  prompt string `"__QA_FAULT_SENTINEL__"` (never reachable via any real client), built it, ran
  it on port 8099 (`--workers 2 --threads 4`, isolated from production port 80). Sequence:
  (1) warmed `qwen3-0.6b` on worker 0 (`model_load_ms:1031`); (2) sent the sentinel prompt for
  `qwen3-0.6b` → `HTTP:500 {"error":"engine worker 0 dropped the response"}`, confirmed via
  server log `thread 'engine-worker-0' (320013) panicked at apps/server/src/app.rs:192:9:
  QA sentinel fault injection...` / `engine worker 0 panicked, respawning with an empty model
  cache (other workers unaffected): QA sentinel fault injection...`; (3) IMMEDIATELY sent a
  request for `minicpm5-2b` (a different model, would route to worker 1) → `HTTP:200`,
  correct answer, `model_load_ms:2169` — worker 1 served normally, completely unaffected by
  worker 0's concurrent panic; (4) `/health` stayed `HTTP:200` throughout; (5) self-heal:
  sent `qwen3-0.6b` again afterward → `HTTP:200`, `model_load_ms:1145` (a FRESH reload,
  proving worker 0's cache was genuinely emptied and the worker came back healthy, not stuck).
  Scratch copy + server process fully torn down afterward (`pkill`, `rm -rf
  /tmp/qa14-fault-copy`), production (port 80, PID 239004) confirmed still running throughout
  and unaffected (`/health` 200 before/during/after).
- [x] G15 (panic message extraction) still correct at the new per-worker granularity —
  evidence: same fault-injection log above shows the REAL panic message ("QA sentinel fault
  injection - testing per-worker panic recovery") logged by the supervisor, not
  `<non-string panic payload>` — `panic_message(&*payload)` still works correctly after the
  pool refactor.
- [x] `shared_backend()` double-checked locking — reviewed `crates/engine/src/lib.rs:48-58` —
  correct, not just cosmetic: fast path (`BACKEND.get()`) is lock-free; slow path takes
  `INIT_LOCK`, RE-CHECKS `BACKEND.get()` inside the lock (this second check is what makes it
  double-checked, not a plain mutex-around-init), then calls `LlamaBackend::init()` exactly
  once and stores via `OnceLock::get_or_init`. Only one thread can ever be inside the guarded
  region at a time, so only one `init()` call is ever made regardless of how many pool
  workers start concurrently — the `BackendAlreadyInitialized` race is genuinely eliminated,
  not papered over (e.g. not just swallowing the error and returning a wrong/stale backend).
  Poison-recovery (`unwrap_or_else(|e| e.into_inner())`) is safe here specifically because the
  guarded region only calls two infallible-after-init operations (`init()`'s own error is
  handled via `?` before any shared state is touched, `get_or_init` is total) — a panic inside
  the critical section can't leave `BACKEND` half-written.
- [x] `reset_context` still called correctly per-request in every worker — evidence: read
  `apps/server/src/app.rs:224-244` (`run_bench`, shared by all N pool workers via
  `pool.rs::worker_loop`) — same reset-before-readout (line 233) and reset-between-
  readout/generate (line 244) as pre-Loop-14, now exercised identically inside every worker's
  own `HashMap<String, Engine>`. No divergence introduced by the pool refactor since all
  workers call the exact same `run_bench` function.
- [x] Full regression — 3 models x 3 methods (readout/generate/laya) via external curl —
  evidence: sequential (not concurrent, to avoid conflating with the throughput test)
  `POST /bench` with `skip_laya` omitted (default false) for `qwen3-0.6b`, `qwen3-4b`,
  `minicpm5-2b` — all 3 returned `HTTP:200`, all 3 `readout.best_option` and
  `generate`-derived choice = `"B) Paris"` (correct), all 3 had a non-null `laya` field
  (Laya successfully called end-to-end for every model this loop, not just skipped).
- [x] `apps/cli` unaffected by the pool refactor — evidence: `git diff` confirms `apps/cli`
  has zero changes in this loop's diff. Spot-checked `openjev-cli --model qwen3-0.6b --prompt
  "The capital of France is:" --options "A) London,B) Paris"` on the server — succeeds,
  correct `readout`/`generate`/`laya` output, `best_option: "B) Paris"` across all 3 methods.
- [x] G17/G18 (Laya `:8090` firewall + reboot-persistence) unaffected — evidence:
  `iptables -L INPUT -n -v` shows the loopback-only ACCEPT (1799 packets — traffic-exercised,
  not a dead rule) + catch-all DROP for `:8090` still present;
  `systemctl is-enabled`/`is-active openjev-laya-firewall.service` → both `enabled`/`active`.

## Unresolved Gaps

- [ ] pool.rs/routing/supervisor has no committed unit tests — gap: extends pre-existing G12
  (apps/server has zero in-repo test coverage); this loop's routing/fault-injection/throughput
  verification is entirely black-box HTTP-level (mine + Loop 14's own), not CI-protected.
  Not opening a new ledger row (same underlying issue as G12), but flagging that G12's scope
  now explicitly includes `pool.rs`.
- [ ] Topology-probe raw benchmark table (all 4 (N,T) x 4 concurrency levels) is documented
  only as a doc comment inside `pool.rs`, not as a separate file under `docs/` — the data
  itself is complete and satisfies D_14's Validation Requirement #1 literally, but is less
  discoverable/maintainable than a dedicated benchmark doc (same class of gap as G19: doc
  placement, not data correctness).
- [ ] Live Laya-outage (killing the real `laya serve` process) not re-triggered this loop —
  gap in the sense that the *outage* code path (as opposed to the happy path) was not
  freshly re-exercised by QA this loop. Deliberately scoped out: D_14's own preservation
  constraint says "laya serve (now the fixed -O3 binary) unaffected," and Loop 13 is
  concurrently depending on the same live process for its own reference comparisons; killing
  it risked disrupting Loop 13's in-progress work for a behavior that (a) `git diff` confirms
  Loop 14 did not touch, and (b) was independently re-verified multiple times in Loops 5-8.
  Recommend a future loop re-verify the outage path once Loop 13 is no longer concurrently
  live, or from a non-shared Laya instance.

## Regressions

None found. G13, G15, G17, G18 all re-verified still correct at the new per-worker
granularity (see Verified Behaviors). No previously-closed issue-ledger row reopened.

## Runtime Check

`Runtime.check(A_14)`: `schema_valid: true`.
- `cargo build --workspace` (debug): exit 0, 1 pre-existing warning, no errors.
- `cargo build --workspace --release`: exit 0, no errors.
- `cargo test --workspace`: exit 0, 7/7 tests passed (4 laya-native + 3 pipeline), 0 failed,
  0 ignored (except 1 doc-test intentionally `ignore`d, pre-existing).
- Production process confirmed live throughout this entire evaluation
  (`ps aux` before/during/after all show PID 239004, `--workers 4 --threads 7`; `/health`
  returned 200 at every checkpoint).
- Independent observation (not a Loop 14 defect): during the full-regression pass,
  `laya_inference_ms` spiked to ~7.2-7.8s for 2 of 3 models (vs the expected ~103-158ms
  post-Loop-12 baseline) — traced to genuine CPU contention: `ps aux` at that moment showed
  Loop 13 running a full parallel `ggmlc` C++ rebuild (a dozen+ `cc1plus` processes at
  ~60-100% CPU each), load average 19.4. This is the same class of cross-loop contention
  already diagnosed and accepted in the run ledger's "Operational note" (two prior incidents,
  neither a code regression). Functional correctness was unaffected (still HTTP 200, correct
  answers) — noted for completeness, not scored as a Loop 14 gap.

## Assessment

**Accept: YES.** All of D_14's Validation Requirements are independently verified with
fresh, candidate-bound evidence (not inferred from the Developer's report): (1) topology
benchmark data is real and complete for all 4 combinations; (2) concurrent different-model
requests genuinely overlap in wall-clock time; (3) a fault-injected panic in one worker
(QA's own independent sentinel, isolated from A_t) leaves the other worker(s) fully
functional and self-heals; (4) the same model is not redundantly reloaded across workers in
the steady state, AND same-model concurrent requests are correctly NOT bunched onto one
worker. `cargo build --workspace` (debug+release) and `cargo test --workspace` both pass
cleanly on the real server. `shared_backend()`'s double-checked-locking fix is a genuine,
correct fix for the `BackendAlreadyInitialized` race, not a workaround. The `model=laya` 400
fix is confirmed. G13/G15/G17/G18 all hold at the new architecture's granularity. Loop 14's
disclosed `cargo fmt --all` side-effect on Loop 13's test file
(`crates/laya-native/tests/sequence_and_reference.rs`) is confirmed formatting-only via a
byte-level whitespace-stripped comparison — no content lost, file still git-tracked, all 4
of its tests still pass. No regressions found anywhere in scope. The only gaps are
pre-existing-class (test coverage, doc placement) or a scoped, justified skip (live Laya
outage, to protect Loop 13's concurrent work) — none block acceptance of this increment.

## Unresolved Questions

- Should the topology-probe raw data be promoted from a `pool.rs` doc comment into a
  `docs/benchmarks/` file for consistency with how other loops (11, 12, 15) recorded their
  benchmark data? (cosmetic, not blocking)
- Should a future loop re-run the live Laya-outage test once Loop 13 is no longer
  concurrently depending on the shared `laya serve` process, to close that residual gap with
  fresh evidence rather than relying on unchanged-code inference?
