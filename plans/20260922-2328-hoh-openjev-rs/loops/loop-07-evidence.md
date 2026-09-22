---
loop: 7
candidate: A_7
status: passed
builder: Developer
assessor: QATester
verified_behaviors:
  - "cargo build --workspace clean on the FINAL, currently-running config: forced recompile (touch'd app.rs/main.rs/lib.rs) with RUSTFLAGS='-C target-cpu=native' cargo build --workspace -- exit 0 (debug, 1m41s, rebuilt llama-cpp-sys-2's C library from scratch), exit 0 (release, 4-5s using the matching cached native C lib). Only the 2 pre-existing warnings (timing unused import, engine deprecated Special enum) -- grep-counted (4 warning: lines total = 2 warnings x 2 builds), matches E_5/E_6 baseline exactly, no new warnings from Loop 7's n_batch plumbing. cargo test --workspace: 3 passed/0 failed (pipeline::readout), same RUSTFLAGS."
  - "openmp genuinely active, independently re-verified at the same 3 levels the Developer claimed, not trusted: (1) llama-cpp-2-0.1.156's own Cargo.toml source read directly from ~/.cargo/registry -- default=[..,\"openmp\",..], openmp=[\"llama-cpp-sys-2/openmp\"] -- confirmed the crate really does default this on, apps/server never sets default-features=false. (2) target/release/build/llama-cpp-sys-2-645b0bc7f842ac6a/out/build/ggml/src/CMakeFiles/ggml-cpu.dir/flags.make read directly: C_FLAGS/CXX_FLAGS both literally contain '-march=native -fopenmp' on the real gcc/g++ command line, not just declared in CMakeCache. (3) ldd target/release/openjev-server shows libgomp.so.1 genuinely linked."
  - "CPU-native build genuinely active on the CURRENTLY RUNNING binary, not just claimed: 4 llama-cpp-sys-2 build dirs exist under target/release/build; the 2 with a completed CMakeCache.txt are 645b0bc7... (GGML_NATIVE:BOOL=ON, mtime 2026-09-23 02:19:41) and b9451eb1... (GGML_NATIVE:BOOL=OFF, mtime 2026-09-22 23:57:58, the pre-native-test baseline build). The running server binary (target/release/openjev-server) was linked at 02:57:16 -- AFTER the native CMakeCache was produced, consistent with the native lib being what's actually linked in. ps -o pid,args on the live PID confirms './target/release/openjev-server --port 80 --threads 28' with no --batch flag (default 512, matching the doc's claim). llama-cpp-sys-2-0.1.156/build.rs source read directly: reads CARGO_ENCODED_RUSTFLAGS, target_cpu==\"native\" -> config.define(\"GGML_NATIVE\",\"ON\"), else explicit OFF -- confirms RUSTFLAGS=-C target-cpu=native is the real, sourced mechanism, not guessed."
  - "n_batch plumbing genuinely wired end-to-end (grep-verified, not assumed): main.rs --batch Option<u32> flag -> unwrap_or_else(EngineConfig::default().n_batch) -> AppState::new(n_threads, n_batch) -> supervisor_loop(rx, n_threads, n_batch) -> worker_loop(&mut rx, n_threads, n_batch) -> run_bench(engines, req, n_threads, n_batch) -> EngineConfig { n_threads, n_batch, .. } -- same shape as Loop 6's n_threads plumbing, not a stub."
  - "docs/benchmarks/server-tuning-results.md Loop 7 tables cross-checked against raw JSON on the server, independently parsed with python3/json (not eye-balled): native-summary.json, batch256-summary.json, batch1024-summary.json -- generation_ms mean/median/max and constrained_readout_ms mean for all 3 models x 3 runs match the doc's tables EXACTLY (e.g. native minicpm5-2b gen 8544.2/9595.5/13144 readout 178.9; batch1024 qwen3-0.6b gen 3965.3/3269/6799 readout 105.3). total_requests=30/error_count=0 confirmed for all 3 files."
  - "Full regression via external curl (not SSH-side), all 3 models, all 3 methods, on the FINAL clean rebuilt+restarted binary: qwen3-0.6b (readout=Paris, generate.valid=true/{\"answer\":\"Paris\"}, laya=Paris/conf 0.4945), minicpm5-2b (same, all correct), qwen3-4b (same, all correct). External /health -> 200."
  - "G13 re-tested a 4th consecutive loop (Loops 4,5,6,7), THIS TIME independently by QA (separate from the Developer's own Loop 7 G13 test in the doc): backed up app.rs (md5 0546062fef4d761c9a75e2c996af029e -- exact match to the Developer's own pre-injection hash, confirming I tested the real current source, not stale). Injected a QA-authored sentinel (__QA7_INDEPENDENT_FAULT_INJECT__, distinct string from the Developer's own test) into the post-n_batch-signature run_bench, rebuilt release (exit 0, 4.18s), restarted --threads 28. External curl trigger -> HTTP 500 {\"error\":\"engine worker thread dropped the response\"} (clean, not a hang). /health immediately after -> 200. Next /bench -> HTTP 200, model_load_ms=1013 (fresh reload proven), correct answer (readout=Paris, generate valid=true). Server log shows the REAL panic message ('QA Loop7 independent G13 re-verification panic') and the respawn confirmation ('engine worker thread panicked, respawning with an empty model cache') -- G15's fix (real panic message, not '<non-string panic payload>') also still holds. Cleanup: reverted app.rs, md5 exact match to pre-injection (0546062f...), grep -c QA7_INDEPENDENT -> 0, rebuilt clean (exit 0), restarted on final config (PID 96471), health -> 200, bench -> correct."
  - "G18 (iptables-persistent systemd unit) independently re-verified real, not trusted from the doc: systemctl status openjev-laya-firewall.service -> active (exited), all 3 ExecStart lines status=0/SUCCESS; systemctl is-enabled -> 'enabled' (not just claimed). Unit file read directly (/etc/systemd/system/openjev-laya-firewall.service): oneshot, ConditionPathExists=!/run/openjev-laya-firewall.applied, WantedBy=multi-user.target, re-applies the exact same 2 iptables rules Loop 6 added by hand. QA's OWN independent simulation (separate from the Developer's Loop 7 test): manually removed both live iptables rules + the /run marker (iptables -D x2, rm marker) to reproduce an empty post-reboot INPUT chain -- confirmed iptables -L INPUT showed 0 rules. Plain 'systemctl start' was a no-op (systemd still considered the oneshot 'active (exited)' from its prior run -- a real nuance: this is expected systemd oneshot semantics, not a G18 defect, since after an ACTUAL reboot the unit starts from 'inactive' and would run normally at multi-user.target). 'systemctl restart' forced re-execution -- all 3 ExecStart lines exited 0/SUCCESS again, iptables -L INPUT -n -v immediately after showed the exact same 2 rules back (ACCEPT tcp dpt:8090 in=lo, DROP tcp dpt:8090), marker file recreated. Post-simulation: internal curl 127.0.0.1:8090/health -> 200 (Laya calls unaffected), external curl 103.146.166.46:8090/health -> timeout (curl exit 28) -- G17's mitigation intact after the G18 test. G18 closed this loop."
  - "Final state confirmed alive and correctly configured, independently: openjev-server PID 96471 (./target/release/openjev-server --port 80 --threads 28, native+openmp build, n_batch default 512) and laya serve PID 86269 both running. Internal (SSH) health: :80 -> 200, :8090 -> 200. External (from my own machine, not SSH): :80/health -> 200, :80/bench correct for all 3 models across all 3 methods; :8090 -> connection timeout (curl exit 28), G17 still enforced."
unresolved_gaps:
  - "Minor doc-precision nuance (new, cosmetic, non-blocking): server-tuning-results.md §2 states 'non-native build had GGML_AVX512:BOOL=OFF' in a sentence adjacent to the native-build claim, which could be read as implying the native build flips GGML_AVX512 to ON. Independently checked: the native build's own CMakeCache.txt ALSO shows GGML_AVX512:BOOL=OFF (this is a separate, manually-set CMake toggle from GGML_NATIVE's own -march=native auto-detection -- llama.cpp's build doesn't set GGML_AVX512=ON just because GGML_NATIVE=ON). Does not affect the substantive, independently-confirmed claim (real -march=native on the actual gcc/g++ command line, real 10-25% constrained_readout_ms improvement) -- just a wording precision gap in the doc, worth a 1-line clarification if a future loop touches this file, non-blocking."
  - "G16 (carried, unchanged). docs/codebase-summary.md tree-vs-prose self-contradiction, cosmetic, not re-checked this loop (out of D_7 scope)."
  - "G10 (carried, unchanged). No unit tests for generate.rs."
  - "G12 (carried, unchanged, larger surface). apps/server still zero in-repo unit tests; now also covers this loop's n_batch plumbing -- only my ad hoc, reverted fault-injection test covered the panic path this session."
  - "G14 (carried, unchanged). Port 80 deviation, previously accepted."
  - "G2/G11 (carried, unchanged, out of scope)."
  - "A finer-grained n_threads sweep (20/24/26) and multi-run averaging (to separate real native/batch effects from single-run harness noise) remain untested -- explicitly out of D_7's scope ('not an open-ended search'), carried as a documented, non-blocking limitation of the tuning methodology."
regressions: []
schema_valid: true
---

## 1. Verified Behaviors

### Build: final config clean, forced past cache
`RUSTFLAGS='-C target-cpu=native' cargo build --workspace` (debug) exit 0 after touching
app.rs/main.rs/lib.rs (1m41s, real recompile of llama-cpp-sys-2's C library, not cache). Release
exit 0 (4-5s, cached matching native C lib). Only the 2 pre-existing warnings (grep-counted: 4
`warning:` lines across both builds = 2 warnings x 2 builds), no new warnings from the n_batch
plumbing. `cargo test --workspace`: 3 passed/0 failed (pipeline::readout).

### openmp genuinely active -- re-verified independently, not trusted from the doc
Read `llama-cpp-2-0.1.156/Cargo.toml` directly from the registry: `default = [...,"openmp",...]`.
Read the actual `flags.make` for `ggml-cpu` in the native build dir: `-march=native -fopenmp`
literally on the real `gcc`/`g++` command line. `ldd target/release/openjev-server` shows
`libgomp.so.1` genuinely linked.

### Native build genuinely running, correlated by build-dir timestamp not just claimed
4 `llama-cpp-sys-2` build dirs exist; only 2 have a completed `CMakeCache.txt`: the native one
(`GGML_NATIVE:BOOL=ON`, mtime 02:19:41) and the pre-native baseline (`GGML_NATIVE:BOOL=OFF`, mtime
Sep 22 23:57:58). The running server binary was linked at 02:57:16 -- after the native cache was
produced. `build.rs` source read directly confirms `CARGO_ENCODED_RUSTFLAGS`/`target_cpu=="native"`
is the real mechanism (not guessed): `config.define("GGML_NATIVE","ON")` when true, explicit
`"OFF"` otherwise.

### n_batch plumbing, grep-verified end-to-end
`main.rs --batch` -> `AppState::new(n_threads, n_batch)` -> `supervisor_loop` -> `worker_loop` ->
`run_bench` -> `EngineConfig { n_threads, n_batch, .. }` -- same shape as Loop 6's `n_threads`
wiring, not a stub.

### Raw JSON cross-check
Independently parsed `native-summary.json`, `batch256-summary.json`, `batch1024-summary.json`
with python3/json. `generation_ms` mean/median/max and `constrained_readout_ms` mean for all 3
models x 3 runs match `server-tuning-results.md`'s tables EXACTLY. `total_requests:30`,
`error_count:0` for all 3.

### Full regression, external curl, all 3 models x 3 methods, final clean binary
qwen3-0.6b/minicpm5-2b/qwen3-4b all: `readout.best_option=Paris`, `generate.valid=true`
(`{"answer":"Paris"}`), `laya.best_option=Paris`. External `/health` -> 200.

### G13 re-test, 4th consecutive loop, QA-independent this time
Own sentinel (`__QA7_INDEPENDENT_FAULT_INJECT__`, distinct from the Developer's own Loop 7 test),
own rebuild, own trigger. HTTP 500 clean -> `/health` 200 immediately -> next `/bench` HTTP 200,
`model_load_ms=1013` (fresh reload), correct answer. Real panic message + respawn confirmation
in the log. Cleanup: md5 exact match to pre-injection, sentinel grep count 0, rebuilt clean,
restarted, health/bench both correct on final state.

### G18, independently simulated by QA (separate from the Developer's own test)
`systemctl status`/`is-enabled` confirm real, not claimed. Removed live rules + `/run` marker to
reproduce an empty post-reboot state; `systemctl start` was a no-op (expected oneshot
`active(exited)` semantics, not a defect -- an actual reboot starts the unit from `inactive`);
`systemctl restart` forced re-execution, all 3 `ExecStart` lines `0/SUCCESS`, rules and marker
both reapplied. G17 intact after: internal 8090 -> 200, external 8090 -> timeout.

## 2. Unresolved Gaps

- **New, cosmetic, non-blocking.** `server-tuning-results.md` §2's phrasing around `GGML_AVX512`
  could mislead -- the native build's own CMakeCache ALSO shows `GGML_AVX512:BOOL=OFF` (a separate
  manual CMake toggle from `GGML_NATIVE`'s `-march=native` auto-detection). Does not affect the
  substantive, independently-confirmed claim (real `-march=native` on the compile line, real
  10-25% readout speedup) -- wording precision only.
- **G16/G10/G12/G14/G2/G11** (all carried, unchanged from Loop 6, see frontmatter).
- Finer-grained `n_threads` sweep (20/24/26) and multi-run averaging remain untested --
  explicitly out of D_7 scope.

## 3. Regressions

None. G13/G15 fixes re-proven working (4th consecutive loop, this time via a QA-independent
fault injection separate from the Developer's own). Laya-outage degrade not separately
re-tested by QA this loop (Developer's own doc covers it in §6, numbers plausible and consistent
with Loop 4-6's pattern; not independently re-run given G13 already consumed the fault-injection
budget and time, see Unresolved Questions). All 3 models still correct via external curl. `cargo
test --workspace` clean. G17 mitigation intact after the G18 test.

## 4. Runtime Check

- SSH-side (`root@103.146.166.46`, `/tmp/openjev`): `cargo build --workspace` exit 0 (debug +
  release, `RUSTFLAGS='-C target-cpu=native'`, forced recompile via `touch`), `cargo test
  --workspace` 3/3 pass. `flags.make`/`CMakeCache.txt`/`ldd`/`build.rs` source all independently
  confirm openmp + native-CPU build genuinely active on the linked binary, not just declared.
  `systemctl status`/`is-enabled openjev-laya-firewall.service` confirm real, enabled, and a
  QA-independent rule-removal + restart simulation proves the unit correctly reapplies both
  `iptables` rules from an empty state.
- External (my own machine, not SSH): `/health` on port 80 -> 200. `/bench` for all 3 models, all
  3 methods (readout/generate/laya) -> 200, all correct, on the FINAL clean rebuilt binary. Port
  8090 external connect -> timeout (curl exit 28), confirmed both before and after the G18
  simulation.
- G13 sentinel (QA's own, independent from the Developer's) -> HTTP 500 clean, `/health` stays
  200, self-heals with `model_load_ms>0`, real panic message + respawn log line -- re-proven
  fresh, 4th consecutive loop.
- `docs/benchmarks/server-tuning-results.md`'s Loop 7 tables (native/batch256/batch1024, all 3
  models) cross-checked against raw `*-summary.json` files independently parsed -- exact match.
- `schema_valid: true` -- E_7 produced against a workspace that built and tested clean (debug +
  release, forced past cache, exact same `RUSTFLAGS` as the running binary), served real external
  HTTP traffic across all 3 LLM models x 3 methods, survived a fresh QA-authored
  panic-recovery re-test, and had its two headline technical claims (openmp active, native build
  active) independently re-derived from primary source (`Cargo.toml`, `build.rs`, `flags.make`,
  `ldd`, build-dir timestamps) rather than trusted from the report.

## 5. Assessment

**Accept: yes.** All of D_7's Validation Requirements independently verified:
- Each tested variable (native build, batch=256, batch=1024) has real recorded numbers in
  `server-tuning-results.md`, cross-checked verbatim against raw JSON, with a stated final config
  (`n_threads=28`, native build, `n_batch=512` default).
- External `/health` -> 200 and full `/bench` (all 3 methods) -> 200 and correct, on the final
  chosen config, for all 3 models (not just Qwen3-0.6B).
- G13 fault-injection re-tested (this time QA-independent, not reused from the Developer's own
  test) -- self-heals exactly as proven in Loops 4-6, no regression from any of this loop's
  rebuilds.

Both of this loop's headline technical claims -- "openmp genuinely active" and "native build
genuinely active on the running binary" -- were independently re-derived from primary source
(crate manifests, `build.rs`, real compiler `flags.make`, `ldd`, build-directory timestamps
correlated against the live binary's link time), not trusted from the Developer's self-report.
One cosmetic doc-wording nuance found (GGML_AVX512 framing), non-blocking. G18 independently
re-simulated (own rule-removal + restart, separate from the Developer's own test) -- genuinely
closes: the unit is enabled, its `ExecStart` actions are correct and idempotent, and the one
nuance found (`start` is a no-op on an already-`active(exited)` oneshot, `restart` needed to
force re-exec in a live simulation) is expected systemd semantics that does not apply to a real
boot -- not a functional gap.

## Definition of Done Checklist (run.md § Definition of Done, cross-loop)

1. **`openjev-server` running on 103.146.166.46, reachable by IP.** MET. Since Loop 3, still true
   now (external curl to port 80 succeeds this loop).
2. **`curl http://103.146.166.46:<port>/health` -> 200.** MET. Re-confirmed this loop, multiple
   times, including after the G13 and G18 fault-injection/simulation tests.
3. **`POST /bench` -> valid `BenchReport` JSON, all 3 methods, at least Qwen3-0.6B.** MET. Exceeded
   -- re-confirmed for all 3 models this loop via external curl, all 3 methods each.
4. **All 3 LLM models + Laya benchmarked successfully on the real server.** MET. Established Loop
   4 (3 LLMs) + Loop 5 (Laya wired), re-confirmed this loop's full regression pass (all 3 models x
   3 methods including `laya`, correct, external curl).
5. **>=1 real performance-tuning iteration recorded (before/after `Timings`).** MET, substantially
   exceeded. Loop 6: `n_threads` in {16,28,32}, 90 real requests, winner=28. Loop 7 (this loop):
   native-CPU build vs. non-native (30 requests), `n_batch` in {256,512,1024} (60 requests) --
   all with real before/after numbers, independently cross-checked against raw JSON both loops.
6. **No auth on `/bench`/`/health` this round.** MET, trivially -- every curl call this loop
   (regression, G13, health checks) succeeded with zero auth headers, in-scope per spec.

**All 6 items MET**, each independently re-verified across at least 2 loops (not just self-report
from the loop that first satisfied it). No item regressed this loop. This supports treating the
HoH loop as ready to conclude after this evidence, per the Runtime's own Loop 6 diminishing-
returns framing in `run.md`.

## Unresolved Questions

- Laya-outage graceful-degrade was NOT independently re-run by QA this loop (only the
  Developer's own §6 account exists for Loop 7's specific native+n_batch-plumbed binary); QA's
  fault-injection budget this loop went to G13 (the more architecturally significant SPOF) and
  G18 (the explicit "insurance"/persistence-verification ask). Laya-outage has been independently
  re-verified by QA in Loops 4, 5, and 6 with unchanged behavior each time, and the code path
  (`run_bench`'s Laya-call error handling) was not touched by Loop 7's changes (only `n_batch`
  plumbing and build flags) -- low risk, but flagging as not independently re-run this specific
  loop, for completeness.
- None of the above block accepting this loop or concluding the HoH run; both are non-blocking
  observations for whoever synthesizes the final wrap-up report.

Server confirmed alive at write time: `openjev-server` PID 96471 (`--port 80 --threads 28`) and
`laya serve` PID 86269, both external-health-200, before this evidence file was finalized.
