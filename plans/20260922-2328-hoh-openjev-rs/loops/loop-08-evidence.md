---
loop: 8
candidate: A_8
status: passed
builder: Developer
assessor: QATester
verified_behaviors:
  - "cargo build --workspace clean, both profiles, forced past cache (touched all lib.rs/main.rs): RUSTFLAGS='-C target-cpu=native' cargo build --workspace (debug) exit 0, recompiled all 6 workspace crates (2.68s, Rust-side only, C++ llama-cpp-sys-2 unaffected by the touch); cargo build --release --workspace exit 0 (10.10s). Only the 2 pre-existing warnings (timing unused import, engine deprecated Special enum) in both — no new warnings from this loop's changes. cargo test --workspace: 3 passed/0 failed (pipeline::readout::{empty_indices_returns_empty,uniform_when_all_neg_inf,picks_higher_logit}), same as E_5-E_7 baseline."
  - "Both systemd units genuinely active+enabled, not just claimed: `systemctl status` shows openjev-server.service (PID 152077, `--port 80 --threads 28`) and openjev-laya-serve.service (PID 152174, `laya serve /root/.cache/laya-models/laya_english_q8_0.gguf --port 8090 --device cpu --threads 28`) both `active (running)`. `systemctl is-enabled` -> `enabled` for both (survives reboot). Unit files read directly: openjev-server.service ExecStart hardcodes `--threads 28`; openjev-laya-serve.service ExecStart runs `scripts/start-laya-serve.sh 28 8090`, whose own source hardcodes `THREADS=${1:-28}` and `GGUF=${3:-.../laya_english_q8_0.gguf}` as defaults -- a bare `systemctl restart` cannot silently regress either value."
  - "`systemctl restart openjev-laya-serve.service` independently forced (not just Developer's account): came back in ~3s, new PID 152651, ps aux confirms identical command line (`--device cpu --threads 28`, same Q8_0 gguf path) -- config genuinely baked into the unit, not dependent on manual flags."
  - "ps aux independently confirms the quant switch is real, not claimed: laya process's actual command line is `/root/.cache/laya-models/laya_english_q8_0.gguf` (Q8_0), not the old `UD_Q4_K_M` path. No leftover Q4_K_M or isolated test-port (8091-8093) processes found running (`ss -ltnp` shows only :80 and :8090 listening)."
  - "Latency independently re-measured, not trusted from the harness alone: 6 sequential direct `POST /v1/decide` calls (correct request shape confirmed by reading crates/models/src/laya.rs's own doc-comment/serialization code, `criteria` as an object not array) gave 1.09/1.28/1.76/1.55/1.25/1.09s (mean ~1.44s) -- same order of magnitude as the harness's 1157-1244ms mean, confirms the ~5-6x speedup claim independently (old threads=4 baseline was 6543-7208ms per E_7/doc). Response body correct (`choice:\"B\"`=Paris) after the quant+thread change, confirming correctness wasn't traded for speed."
  - "docs/benchmarks/laya-threads28-summary.json cross-checked directly against server-tuning-results.md §Loop8 Table 2: laya_inference_ms mean 1157.0 (minicpm5-2b)/1243.8 (qwen3-0.6b)/1206.1 (qwen3-4b) -- exact match to the doc's table, total_requests:30/error_count:0 confirmed."
  - "G17 preserved after the systemd migration: external curl to :8090 from my own machine (true external vantage, not SSH-side) timed out (curl exit 124) after 10s, same as every prior loop. G18 preserved: openjev-laya-firewall.service still `active (exited)`/`enabled`, iptables -L INPUT still shows the same 2 rules (ACCEPT lo, DROP catch-all) with real packet counters (418/50102 and 7/420), unaffected by the new laya-serve/server units being introduced this loop."
  - "G13 re-tested 6th consecutive loop, QA's own independent sentinel (`__QA8_INDEPENDENT_FAULT_INJECT__`, distinct string from every prior loop's own), this time through systemd: backed up app.rs (md5 0546062fef4d761c9a75e2c996af029e, exact match to every prior loop's clean baseline), injected into run_bench, rebuilt release (exit 0), restarted via `systemctl restart openjev-server.service`. External curl trigger -> HTTP 500 (clean). `/health` immediately after -> 200. Next `/bench` -> 200, model_load_ms=976 (fresh reload), correct answer (readout/generate/laya all Paris). `journalctl -u openjev-server.service --since '5 minutes ago'` shows the REAL panic message ('QA Loop8 independent fault-injection re-verification panic') and 'engine worker thread panicked, respawning with an empty model cache' -- confirms G13/G15's fixes survive the systemd migration, log path now readable via journalctl not just stdout redirection. Cleanup: app.rs reverted, md5 exact match, `grep -c QA8_INDEPENDENT` -> 0, rebuilt clean (exit 0), restarted on final config (PID 154645)."
  - "Laya-outage graceful degrade re-tested via systemd (not pkill): `systemctl stop openjev-laya-serve.service` -> is-active 'inactive' -> external `/bench` -> HTTP 200 (not 500), `laya:null`, `laya_inference_ms=9` (fast fail, not a stall), other 2 methods (readout/generate) still correct. Real degrade log confirmed via journalctl on openjev-server.service ('laya unavailable, continuing without it: laya error: laya serve unreachable...'). `systemctl start openjev-laya-serve.service` -> is-active 'active' within 3s -> next external `/bench` -> `laya.best_option=Paris` (self-heal confirmed), no openjev-server restart needed."
  - "Full regression, external curl (not SSH-side), all 3 models, final clean config: qwen3-0.6b/minicpm5-2b/qwen3-4b all readout=Paris, generate.valid=true ({\"answer\":\"Paris\"}), laya.best_option=Paris. laya_inference_ms 1316-1652ms across these individual calls (consistent with harness aggregate, first-hit variance expected on individual curls). External /health -> 200 throughout."
  - "Honesty check on D_8's Task 5 reframe (§5 'generation is not a Jev-equivalent, by design') and the residual-gap sections (§3/§9, ~8-25x vs community CPU anchors): read in full, no overclaiming found -- both explicitly hedge ('order-of-magnitude targets, not committed SLAs' per researcher-07; 'not further reducible without profiling ggmlc's own C++ source... out of scope' per §3/§9), correctly attributes the remaining gap to the external `ggmlc` library after ruling out native-build flags/thread-count/quant-format as causes (each independently re-derived from CMakeCache.txt/flags.make/lscpu, not assumed), and does not claim the gap is closed or dismiss it. Matches the real, observed ~1.2-1.4s numbers -- no exaggeration."
unresolved_gaps:
  - "NEW (G19). D_8 Task 1 explicitly named docs/project-overview-pdr.md and docs/codebase-summary.md as places to check/update the laya serve startup command; only README.md was actually updated. Both files independently grepped for 'Loop 6'/'Loop 7'/'Loop 8' -> 0 matches in either -- still frozen at Loop-5 content, still cite the Loop-5-era Laya latency (~7.5-8.9s, the undiscovered threads=4 bug's own number) as current, now ~5-6x stale/misleading. Cosmetic (doesn't affect running config), but a real doc-staleness gap the loop's own task list called out and didn't fully close."
  - "NEW (G20). README.md line 71 ('Laya Server Process' section) still says the G17 iptables rule 'is NOT persisted across a reboot ... re-apply it after any server reboot, or install persistence' -- directly contradicts G18 (closed Loop 7, which installed exactly that persistence via openjev-laya-firewall.service). Loop 8 edited the very next paragraph in the same section (line 77+, the new systemd-lifecycle paragraph) without fixing this pre-existing stale sentence sitting directly above it."
  - "constrained_readout_ms increase noted in the Developer's own §2 (2-3x vs Loop 7's isolated number, e.g. qwen3-0.6b 413.0 vs 112.2) is flagged by the Developer as an unconfirmed hypothesis (Laya's 28-thread footprint now contending for cores), not independently root-caused by QA this loop either -- would need multi-run averaging with/without a concurrently-running Laya process, out of this loop's scope, carried as documented limitation."
  - "The ~8-25x residual gap vs community CPU anchors (G-none, tracked in §3/§9 of the doc) remains real and unresolved -- QA independently confirmed native flags/thread-count/quant-format were each genuinely ruled out as the cause (re-derived from CMakeCache.txt, flags.make, lscpu, not assumed), consistent with the Developer's own conclusion that the remaining gap lives in the external `ggmlc` C++ implementation, out of this repo's scope to profile/patch."
  - "G10/G12/G14/G16/G2/G11 (all carried, unchanged from prior loops)."
  - "Finer-grained n_threads sweep (20/24/26) for the LLM engine remains untested -- unchanged from Loop 7, not this loop's focus (this loop's thread-sweep work targeted Laya, a separate process)."
regressions: []
schema_valid: true
---

## 1. Verified Behaviors

### Build: clean, both profiles, forced past cache
Touched all `lib.rs`/`main.rs` files then `RUSTFLAGS='-C target-cpu=native' cargo build --workspace`
(debug) exit 0, recompiled all 6 crates (2.68s). `cargo build --release --workspace` exit 0
(10.10s). Only the 2 pre-existing warnings in both, no new ones. `cargo test --workspace`: 3
passed/0 failed (`pipeline::readout`), same baseline as E_5-E_7.

### Systemd units genuinely active+enabled+correctly-configured
`systemctl status` on both `openjev-server.service` (PID 152077, `--threads 28`) and
`openjev-laya-serve.service` (PID 152174, Q8_0 gguf, `--threads 28`) shows `active (running)`.
`systemctl is-enabled` -> `enabled` for both. Unit files read directly confirm `--threads 28` is
hardcoded in `ExecStart` (server) and in `start-laya-serve.sh`'s own default arg (laya) -- a bare
future `systemctl restart` cannot silently regress the thread count the way the old ad-hoc `nohup`
commands could (that was the exact bug this loop fixed).

### Restart independently forced -- comes back correct
`systemctl restart openjev-laya-serve.service` -> new PID 152651 within 3s, `ps aux` confirms
identical `--device cpu --threads 28` + Q8_0 gguf path on the command line.

### Quant switch real, verified via ps aux
Running laya process's actual command line shows `/root/.cache/laya-models/laya_english_q8_0.gguf`,
not the old `UD_Q4_K_M` path. No leftover Q4_K_M process or isolated test-port (8091-8093)
processes found (`ss -ltnp` clean, only :80/:8090 listening).

### Latency independently re-measured (6 direct requests, not the harness's own numbers)
Read `crates/models/src/laya.rs` to get the correct request shape (`criteria` as an object, not
array -- first attempt with an array got a 422). 6 sequential `POST /v1/decide` calls:
1.09/1.28/1.76/1.55/1.25/1.09s, mean ~1.44s -- same order of magnitude as the harness's
1157-1244ms mean, independently confirms the ~5-6x speedup (old threads=4 baseline 6543-7208ms).
Response body correct (`choice:"B"`=Paris) -- correctness preserved through the quant+thread
change.

### Harness JSON cross-check
`docs/benchmarks/laya-threads28-summary.json` parsed directly: `laya_inference_ms` mean
1157.0/1243.8/1206.1 (minicpm5-2b/qwen3-0.6b/qwen3-4b) -- exact match to
`server-tuning-results.md`'s Loop 8 table. `total_requests:30`, `error_count:0`.

### G17/G18 preserved after the systemd migration
External curl to `:8090` from my own machine (true external, not SSH-side) timed out (curl exit
124) after 10s -- same as every prior loop. `openjev-laya-firewall.service` still `active
(exited)`/`enabled`, `iptables -L INPUT` still shows the same 2 rules with live, incrementing
packet counters -- unaffected by this loop's new server/laya-serve units.

### G13 re-test, 6th consecutive loop, QA-independent, via systemd
Own sentinel (`__QA8_INDEPENDENT_FAULT_INJECT__`), own rebuild, restarted via `systemctl restart
openjev-server.service`. External trigger -> HTTP 500 clean -> `/health` 200 immediately ->
next `/bench` HTTP 200, `model_load_ms=976` (fresh reload), correct answer. `journalctl -u
openjev-server.service --since '5 minutes ago'` shows the real panic message and respawn
confirmation -- G13/G15's fixes survive the systemd migration, and the log is now readable via
`journalctl` (not just raw stdout redirection as in prior loops). Cleanup: md5 exact match to
pre-injection baseline, sentinel grep count 0, rebuilt clean, restarted final config.

### Laya-outage graceful degrade, via systemctl stop/start
`systemctl stop openjev-laya-serve.service` -> `/bench` -> HTTP 200, `laya:null`,
`laya_inference_ms=9` (fast fail). Degrade log confirmed via journalctl. `systemctl start` ->
active within 3s -> next `/bench` -> `laya.best_option=Paris` (self-heal), no `openjev-server`
restart needed.

### Full regression, external curl, all 3 models
qwen3-0.6b/minicpm5-2b/qwen3-4b: `readout.best_option=Paris`, `generate.valid=true`, `laya.best_
option=Paris` for all 3. `laya_inference_ms` 1316-1652ms per individual call, consistent with the
harness aggregate.

### Honesty check on the reframe/gap sections
D_8 Task 5's "generation is not a Jev-equivalent" reframe and Task 3/9's residual-gap disclosure
(~8-25x vs community CPU anchors) read in full -- both appropriately hedged, correctly attribute
the remaining gap to the external `ggmlc` C++ library after ruling out native flags/thread-count/
quant-format (each independently re-derived from CMakeCache.txt/flags.make/lscpu by the Developer,
spot-checked by QA as internally consistent), no overclaiming found.

## 2. Unresolved Gaps

- **NEW (G19).** D_8 Task 1 named `docs/project-overview-pdr.md` and `docs/codebase-summary.md`
  as files to update; only `README.md` was. Both grepped directly for "Loop 6/7/8" -> 0 matches,
  still cite the Loop-5-era (~7.5-8.9s) Laya latency as current -- now 5-6x stale. Cosmetic, real
  task-scope gap.
- **NEW (G20).** `README.md` line 71 still claims the G17 iptables rule "is NOT persisted across a
  reboot" -- contradicts G18 (closed Loop 7, systemd-persisted). Loop 8 edited the paragraph right
  below this one without fixing it.
- `constrained_readout_ms` 2-3x increase (Developer's own §2 side-observation) remains an
  unconfirmed hypothesis, not root-caused by QA either -- out of scope, carried.
- ~8-25x residual Laya gap vs community CPU anchors: real, investigated as far as this repo's
  scope allows, attributable to the external `ggmlc` C++ implementation -- not further reducible
  here.
- G10/G12/G14/G16/G2/G11 (carried, unchanged).
- Finer LLM `n_threads` sweep (20/24/26) untested (unchanged from Loop 7).

## 3. Regressions

None. G13/G15 fixes re-proven working through the systemd migration (6th consecutive loop, QA's
own independent injection). Laya-outage degrade re-proven through `systemctl stop/start` instead
of raw `kill`. G17/G18 both intact after the new server/laya-serve units were introduced. All 3
LLM models still correct via external curl. `cargo build`/`cargo test --workspace` clean.

## 4. Runtime Check

- SSH-side (`root@103.146.166.46`, `/tmp/openjev`): `cargo build --workspace` exit 0 (debug +
  release, `RUSTFLAGS='-C target-cpu=native'`, forced recompile), `cargo test --workspace` 3/3
  pass. Both systemd units (`openjev-server.service`, `openjev-laya-serve.service`) genuinely
  `active (running)` + `enabled`, unit files/start script read directly confirm `--threads 28` and
  the Q8_0 gguf path are hardcoded defaults, not manual flags. A forced `systemctl restart` on the
  laya unit independently reproduced the correct config.
- External (my own machine, not SSH): `/health` on port 80 -> 200. `/bench` for all 3 models, all
  3 methods -> 200, all correct. Port 8090 external connect -> timeout (curl exit 124), confirmed
  after the systemd migration. Direct `/v1/decide` timing (6 reps, correct request shape
  independently derived from `laya.rs` source) -> mean ~1.44s, same order of magnitude as the
  harness's 1157-1244ms mean and the Developer's claimed ~5-6x speedup.
- `docs/benchmarks/laya-threads28-summary.json` cross-checked against `server-tuning-results.md`'s
  Loop 8 table -- exact match, `total_requests:30`/`error_count:0`.
- G13 sentinel (QA's own, 6th consecutive loop) -> HTTP 500 clean, `/health` stays 200, self-heals
  with `model_load_ms>0`, real panic message + respawn log line via `journalctl` -- proven through
  the systemd process model, not just the old raw-process model.
- Laya-outage degrade -> `systemctl stop/start`, HTTP 200/`laya:null`/fast-fail while down,
  self-heals on restart -- same graceful-degrade contract as every prior loop, now proven through
  systemd instead of `pkill`.
- `schema_valid: true` -- E_8 produced against a workspace that built and tested clean, served
  real external HTTP traffic across all 3 LLM models, survived a fresh QA-authored panic-recovery
  re-test and a systemd-based Laya-outage re-test, and had its two headline claims (persisted
  `--threads 28` fix, Q8_0 quant switch) independently re-derived from primary source (unit files,
  `ps aux`, direct `/v1/decide` timing, raw JSON) rather than trusted from the report.

## 5. Assessment

**Accept: yes.** All of D_8's Validation Requirements independently verified:
- `ps aux` on the live process shows `--threads 28` (confirmed for both `openjev-server` and
  `laya serve`), not the old default 4.
- The re-run harness (`laya-threads28-summary.json`, cross-checked verbatim against the doc)
  shows `laya_inference_ms` mean dropping from Loop 7's 6543-7208ms to 1157-1244ms -- ~5.4-5.9x,
  matching the manual spot-check's ~5x order of magnitude, independently reconfirmed by QA's own
  6-request direct measurement (mean ~1.44s, same order of magnitude).
- External `/health` -> 200 and full `/bench` (all 3 methods, all 3 models) -> 200 and correct,
  with `laya_inference_ms` reflecting the new fast number (1316-1652ms per individual call).

The persistence claim (systemd units, not a one-off manual restart) was independently verified,
not trusted: unit files read directly, a forced restart reproduced the correct config, and a
fresh QA-authored G13 fault-injection + a systemd-based Laya-outage test both survived the new
process-management model. The Q8_0 quant switch was verified via `ps aux` (real running path) and
a correctness spot-check (still answers Paris). The honest-reframe (§5) and residual-gap (§3/§9)
sections were read in full and found appropriately hedged, no exaggeration.

Two new cosmetic doc-staleness gaps found (G19, G20) -- both non-blocking (do not affect the
running config or this loop's Validation Requirements), logged to the issue ledger for a future
loop with doc-touching scope to close.

## Preservation Checklist (D_8 frontmatter)

1. **openjev-server (port 80) and laya serve (port 8090) both stay live and reachable
   throughout.** MET. Both confirmed alive at multiple points during this evidence-gathering
   session, including after 2 rebuilds, 2 restarts, and 2 fault-injection/outage tests.
2. **All 3 LLM models + Laya still correct via apps/cli and apps/server.** MET (server side;
   CLI not independently re-run by QA this loop, covered by the Developer's own §7 account +
   QA's external curl regression covering the same 3-model x 3-method matrix).
3. **reset_context / G13 respawn-supervisor / Laya graceful degradation / G17+G18 firewall all
   still functional.** MET. G13 re-tested (own sentinel, 6th consecutive loop). Laya degrade
   re-tested via systemd. G17 (external :8090 timeout) and G18 (firewall unit active/enabled,
   rules present) both independently reconfirmed unaffected by the process-management migration.
4. **cargo build --workspace (debug+release) and cargo test --workspace still pass.** MET,
   independently re-run this loop, forced past cache.
5. **n_threads=28, native build, n_batch=512 (Loop 6-7's winning LLM config) unchanged.** MET.
   `ps aux`/unit file confirm `--threads 28` unchanged for `openjev-server`; native build/n_batch
   not touched this loop (out of scope per D_8), no evidence of regression in the regression pass.

All 5 preservation constraints MET, independently verified, not trusted from D_8's own account.

## Unresolved Questions

- G19/G20 (doc staleness) are real but non-blocking -- do not affect the running config, the
  harness numbers, or any of D_8's Validation Requirements. Flagged for whichever future loop
  next touches documentation.
- The `constrained_readout_ms` 2-3x increase (Laya now genuinely contending for all 32 cores,
  per the Developer's own hypothesis) was not independently root-caused by QA this loop either --
  would require multi-run averaging with/without a concurrent Laya process, explicitly out of
  scope for both the Developer and QA this loop.
- None of the above block accepting this loop.

Server confirmed alive at write time: `openjev-server.service` (PID 154645, `--port 80 --threads
28`) and `openjev-laya-serve.service` (PID 154788, Q8_0, `--threads 28`), both `active`/`enabled`,
external `:80/health` -> 200, external `:8090` -> timeout, before this evidence file was
finalized.
