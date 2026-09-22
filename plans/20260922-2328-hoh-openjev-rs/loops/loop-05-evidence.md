---
loop: 5
candidate: A_5
status: passed
builder: Developer
assessor: QATester
verified_behaviors:
  - "IDE E0063 diagnostic (missing field `laya` at apps/cli/src/main.rs:110, apps/server/src/app.rs:260, flagged in task context) is a FALSE ALARM -- same pattern as Loop 4's E0560. Real `cargo build --workspace` on the Linux server (source ~/.cargo/env; /tmp/openjev): exit 0, only the 2 pre-existing warnings (timing unused Instant, engine deprecated Special). `cargo build --workspace --release`: exit 0, same warnings only. `cargo test --workspace`: 3 passed (pipeline::readout), 0 failed, identical to E_2/E_3/E_4 baseline. Both `CliOutput` (apps/cli/src/main.rs:41-49) and `BenchReport` (apps/server/src/app.rs:32-40) structs read directly: both declare `laya: Option<models::LayaScoreResult>` and both constructors (main.rs:127-135, app.rs:285-293) populate it -- no missing-field condition exists in the real source. Confirmed stale rust-analyzer cache, not a real compile error."
  - "`laya` binary is real, not faked: `/tmp/ggmlc/build/examples/laya/laya` is an ELF64 executable (5.5MB, not stripped). `laya help` lists real subcommands (help/decide/serve/daemon/bench/info/list-presets/detect-lang) with real usage text. `laya info <laya_english_ud_q4_k_m.gguf>` loads the actual GGUF and prints real metadata (vocab=50368, max_len=512, template=`[CLS] {qtype} question: ... [SEP] {state} [SEP]`, temperature tables) -- not a stub."
  - "`laya serve` running as a persistent background process on 127.0.0.1... actually bound 0.0.0.0:8090 (see Gap below), verified via `ss -ltnp` (PID 67328/laya after restart) and `curl -s http://127.0.0.1:8090/health` -> 200 from the server itself, matching D_5 VR1."
  - "Real `/v1/decide` response, curled directly via SSH (not reused from Developer's doc comment): `POST http://127.0.0.1:8090/v1/decide` with the France/A-London/B-Paris prompt -> `{\"answers\":{\"answer\":{\"type\":\"choice\",\"action\":{...},\"confidence\":0.0095,\"choice\":\"B\",\"probabilities\":{\"A\":0.4426,\"B\":0.5574}}},...}`. Independently read `crates/models/src/laya.rs`: `AnswerEntry` struct (lines 81-86) declares `choice: Option<String>`, `probabilities: Option<HashMap<String,f32>>`, `confidence: Option<f32>` -- exact field-name match against the real response, not guessed. `score()` (lines 93-149) correctly extracts `answers.get(\"answer\")`, `.probabilities`, `.choice`, `.confidence` -- parsing logic matches the real live shape."
  - "External CLI+server integration confirmed live (external curl from Windows machine, `skip_laya` omitted/false): `POST http://103.146.166.46/bench` for qwen3-0.6b -> HTTP 200, `laya` populated (`{\"probs\":{\"A\":0.4426,\"B\":0.5574},\"best_option\":\"B\",\"confidence\":0.0095}`), `laya_inference_ms=8158` (real non-zero HTTP-round-trip number, much slower than the LLM's own constrained_readout_ms=93 as expected for a first-request cold graph-alloc pass, still far less than a full generation). `laya_model_load_ms=0` for every call, matching the documented design (model loads once at `laya serve` startup)."
  - "`apps/cli` independently re-run via SSH (laya enabled, not skipped): `openjev-cli --model qwen3-0.6b ...` -> exit 0, `laya` field populated (`probs A=0.4426/B=0.5574, best_option=B, confidence=0.0095`) -- EXACT match to the external server's `/bench` response for the identical prompt, confirming CLI/server DTO parity for the new laya field, same standard as prior loops' model parity checks."
  - "`skip_laya=true` genuinely opts out (VR4): external `POST /bench` with `\"skip_laya\":true` -> HTTP 200, `\"laya\":null`, `laya_model_load_ms=0`, `laya_inference_ms=0` -- exactly the documented pre-Loop-5 default shape."
  - "Outage scenario (VR5) independently reproduced, not reused from any prior report: killed the real `laya serve` PID (65836) via SSH, confirmed `ss -ltnp` no longer shows :8090 listening. External `POST /bench` (laya not skipped) during the outage -> HTTP 200 (NOT 500), readout+generate both succeeded correctly (best_option=B, valid=true), `laya:null`, `laya_inference_ms=9` (fast local connection-refused, not a hang/timeout stall). Server log (`grep 'laya unavailable' logs/server.log`) shows the real degrade message: `laya unavailable, continuing without it: laya error: laya serve unreachable: error sending request for url (http://127.0.0.1:8090/v1/decide)`."
  - "Auto-recovery without restarting `openjev-server` (VR5, second half): restarted `laya serve` via the same `setsid nohup ... & disown` pattern, waited for its own `/health`->200, then called external `/bench` again -- `laya` field populated again correctly, with the SAME `openjev-server` PID (66339/66340, started 01:13) unchanged throughout the whole outage+recovery cycle (`ps aux` re-checked before and after) -- confirms self-heal with zero operator/process intervention on the server binary itself."
  - "G15 fix re-verified via a SELF-CONSTRUCTED fault injection (not reusing E_4's evidence): read `apps/server/src/app.rs:114` directly -- `panic_message(&*payload)` (deref applied, the fix). Backed up app.rs (md5sum `aa0c69873b0e29835deec05a66577c4a`), injected a 3-line sentinel panic (`if req.prompt == \"__QA_G15_FAULT_INJECT__\" { panic!(\"QA G15 re-verification: deliberate literal-str panic\") }`) at the top of `run_bench`, rebuilt release (exit 0), restarted server. `POST /bench` with the sentinel prompt -> HTTP 500 `{\"error\":\"engine worker thread dropped the response\"}` (clean, not a hang). `grep -A1 panicked logs/server.log` shows the REAL message: `engine worker thread panicked, respawning with an empty model cache: QA G15 re-verification: deliberate literal-str panic` -- NOT `<non-string panic payload>`. This directly disproves the old G15 bug and confirms the fix works for a real `&str` literal panic payload, the exact case G15 was about."
  - "G13 respawn-supervisor still functional, re-proven via the same fault injection above (not a separate test, same panic event doubles as regression coverage): immediately after the panic, `/health` -> 200 (unaffected), next `/bench` call -> HTTP 200, `model_load_ms=889` (>0, proving fresh reload of an empty cache), correct answer. No regression in G13's behavior from Loop 4."
  - "Fault-injection cleanup verified, not just claimed: `cp` backup restored, `md5sum apps/server/src/app.rs` -> `aa0c69873b0e29835deec05a66577c4a`, exact match to the pre-edit hash. `grep -c QA_G15 apps/server/src/app.rs` -> 0. Rebuilt release clean (exit 0). Server restarted on the clean binary (new PID 67870). Final `grep -rn 'QA_G15\\|QA_FAULT' apps/ crates/` -> no matches (grep exit 1). Backup file (`/tmp/app.rs.qa-backup`) deleted."
  - "Preservation -- all 3 LLM models re-verified correct AFTER the fault-injection revert+rebuild cycle (not before): external `/bench` (skip_laya=true) for qwen3-0.6b, minicpm5-2b, qwen3-4b all -> HTTP 200, all answer `best_option=B` (Paris) with high confidence (0.9999/0.9987/0.99998), `valid=true`. Matches E_4's baseline correctness, no regression from the Laya wiring or the fault-injection cycle."
  - "Preservation -- `cargo test --workspace` re-run on the final clean reverted state: 3 passed, 0 failed (identical to the pre-fault-injection run above)."
  - "Preservation -- `reset_context` call-site count and semantic positions unchanged: `grep -n reset_context apps/server/src/app.rs` shows 3 call sites (line 232 inside the fresh-load/warmup branch, line 248 at the per-request boundary before tokenize -- applies to both cache-hit and fresh-load paths, line 259 between readout and generate -- `MUST reset between the two independent pipeline runs` comment present); `apps/cli/src/main.rs` unchanged at lines 84, 100. Line numbers shifted vs E_4 (232/248/259 vs E_4's 237/248) purely because Laya code/comments were inserted above -- the same 2 semantic invariants (top-of-request, between-readout-generate) still hold, matching D_5's preservation constraint text exactly."
  - "Preservation -- server ends alive, clean, externally reachable: final external `curl http://103.146.166.46/health` -> 200. Final SSH-side `curl 127.0.0.1:80/health` -> 200 and `curl 127.0.0.1:8090/health` -> 200. `ps aux` shows exactly one `openjev-server` process (PID 67870, clean post-revert binary) and one `laya serve` process (PID 67328) at session end."
  - "Docs (Task 8) partially and substantively updated: README.md §Laya Single-Pass Encoder and new §Laya Server Process (lines 60-80) correctly describe the real `laya serve` + blocking-HTTP-client architecture (not a shell-out), the two-process model, real CPU latency numbers (cold `laya decide` CLI ~39.6s per README, not independently re-timed by me this loop -- see Unresolved Questions), and restart/status-check instructions. `docs/project-overview-pdr.md:98` and `docs/codebase-summary.md:47` also correctly describe the HTTP-client approach. `docs/codebase-summary.md`'s top summary paragraph (line 3) correctly says all crates are 'implemented and running real inference (not stubs)' -- but see G16 regression note below: the Directory Tree ASCII diagram in the SAME file still incorrectly labels models/engine/pipeline `[STUB]`, unfixed since Loop 4."
unresolved_gaps:
  - "G17 (new, security-relevant). `laya serve` is bound to `0.0.0.0:8090`, NOT `127.0.0.1:8090` as D_5's task text specified and as README.md:64 explicitly (and incorrectly) claims ('bound to `127.0.0.1:8090`... not externally reachable by design'). Independently confirmed via `ss -ltnp`: `LISTEN 0 10 0.0.0.0:8090 0.0.0.0:* users:((\"laya\",pid=...))`, reproduced both before and after a `laya serve` restart during this QA session (not a one-off fluke). Root cause confirmed via `laya help`'s own SERVE section: the binary has NO `--host`/`--bind` flag at all (`--port`, `--device`, `--threads`, `--cuda-graph`, `--max-batch`, `--models-dir`, `--family` only) -- there is no way to force loopback-only binding from the CLI; this is a binary limitation, not a developer misconfiguration. Currently NOT externally reachable in practice: external curl from a Windows machine outside the server timed out after 8s (`Connection timed out`), while port 80 (openjev-server) answered normally from the same external vantage point -- so SOME external filter (a cloud-provider security group, per G14's precedent) is blocking 8090 today. However: (1) no host-level firewall exists on the box at all (`iptables`/`ufw` both `command not found`), so this protection is 100% external to the repo/server and undocumented/unverifiable from inside this project; (2) `laya help`'s own SERVE text states 'Auth is off unless LAYA_API_KEY or TYPESAFE_API_KEY is set' -- Laya's own docs flag this as expected to be paired with either loopback binding or auth, neither of which this deployment has; (3) if the cloud security group is ever widened (a plausible future change, e.g. for a different service, or reset during a redeploy) `/v1/decide` becomes a fully unauthenticated, unrestricted internet-facing inference endpoint with no rate-limiting -- a real DoS/resource-abuse and potential model-probing exposure, not a hypothetical one. This is a genuine deviation from the intended design (D_5's task text explicitly says 'bound to `127.0.0.1:<internal-port>` (does NOT need external reachability... never the public internet)'), and README.md's claim of 'not externally reachable by design' is factually wrong (it is 'not currently reachable by an external, unverified, out-of-band firewall rule this loop cannot inspect or guarantee') -- a meaningfully weaker and riskier claim than what's documented. Recommend: (a) fix README's wording to describe the ACTUAL protection mechanism (external firewall, not app-level binding) so a future reader doesn't over-trust it: (b) investigate whether a reverse-proxy/local-only wrapper (e.g. `socat`/`nginx` binding 127.0.0.1 and proxying, or setting `LAYA_API_KEY` even for localhost-only defense-in-depth) is feasible without modifying the `laya` binary itself; (c) at minimum, verify with whoever controls the cloud security group that 8090 is deliberately closed and document that dependency explicitly rather than silently relying on it. Non-blocking for D_5's functional VRs (all of which pass), but a real infrastructure-security finding that should not sit un-escalated."
  - "G16 (carried, NOT fixed this loop -- verified by direct re-read). `docs/codebase-summary.md`'s Directory Tree ASCII diagram (now lines ~20-22) still labels `models/`, `engine/`, `pipeline/` as `[STUB]`, while the file's own top summary paragraph (line 3, updated THIS loop for the Laya work) correctly says 'all implemented and running real inference (not stubs)'. Self-contradiction persists, now arguably more visible since the top paragraph was freshly touched this loop without the nearby tree being fixed. Cosmetic, not a functional gap."
  - "G10 (carried, unchanged). crates/pipeline/generate.rs still has no committed unit tests. Not this loop's scope."
  - "G12 (carried, unchanged). apps/server/src/{app,main}.rs still has zero committed unit tests -- the new `run_laya`/laya-degradation branch in `run_bench` (app.rs:270-283, this loop's new logic) also has no in-repo regression protection; only my ad hoc, now-reverted fault-injection and outage-simulation tests covered it this session, and per the read-only QA constraint those test artifacts don't persist in-repo."
  - "G14 (carried, unchanged). Port 80 deviation, previously accepted -- unaffected this loop."
  - "G2/G11 (carried, unchanged, out of this loop's scope per D_5's Out-of-scope section)."
regressions: []
schema_valid: true
---

## 1. Verified Behaviors

### E0063 IDE diagnostic resolved -- confirmed FALSE ALARM (same pattern as Loop 4's E0560)
Real `cargo build --workspace` on the Linux server: **exit 0**, only the 2 pre-existing warnings.
`cargo build --workspace --release`: **exit 0**, same. `cargo test --workspace`: **3 passed, 0
failed**. Directly read both `CliOutput` (apps/cli/src/main.rs:41-49) and `BenchReport`
(apps/server/src/app.rs:32-40): both declare `laya: Option<models::LayaScoreResult>`, and both
constructors populate it (main.rs:127-135, app.rs:285-293). No missing-field condition exists in
the real source at the flagged lines or anywhere else -- confirmed stale rust-analyzer cache.

### `laya` binary and `laya serve` process, verified real not faked
- `laya` binary: ELF64 executable, 5.5MB, not stripped, at `/tmp/ggmlc/build/examples/laya/laya`.
- `laya help`: real usage text, 8 subcommands (help/decide/serve/daemon/bench/info/list-presets/detect-lang).
- `laya info <gguf>`: loads the real model, prints real metadata (vocab=50368, max_len=512,
  the exact `[CLS] {qtype} question:...` template, temperature-by-option-count tables).
- `laya serve` process confirmed running via `ps aux` and `ss -ltnp`; `/health` -> 200 via SSH.

### Real `/v1/decide` response matches `crates/models/src/laya.rs` parsing exactly
```
curl -X POST http://127.0.0.1:8090/v1/decide ... (France/London/Paris prompt)
-> {"answers":{"answer":{"choice":"B","confidence":0.0095,
    "probabilities":{"A":0.4426,"B":0.5574}, ...}}, ...}
```
`AnswerEntry` struct (laya.rs:81-86) fields `choice`/`probabilities`/`confidence` match exactly.
`score()` (laya.rs:93-149) extracts `answers.answer.{choice,probabilities,confidence}` correctly.

### Full CLI + server integration, external curl (VR2/VR3)
- External `/bench` (laya enabled): HTTP 200, `laya:{"probs":{"A":0.4426,"B":0.5574},"best_option":"B","confidence":0.0095}`, `laya_inference_ms=8158` (real, non-zero, HTTP-round-trip-scale), `laya_model_load_ms=0`.
- `openjev-cli` (laya enabled, via SSH): exit 0, `laya` field EXACT match to server's response for the identical prompt -- CLI/server DTO parity confirmed for the new field.
- `skip_laya=true` (VR4): HTTP 200, `laya:null`, both laya timing fields `0` -- exact pre-Loop-5 default shape.

### Outage scenario, independently reproduced (VR5)
1. Killed real `laya serve` PID via SSH; `ss -ltnp` confirmed :8090 no longer listening.
2. External `/bench` (laya not skipped) during outage -> **HTTP 200** (not 500), readout+generate both correct, `laya:null`, `laya_inference_ms=9` (fast fail, not a hang).
3. Server log: `laya unavailable, continuing without it: laya error: laya serve unreachable: error sending request for url (http://127.0.0.1:8090/v1/decide)` -- real degrade message.
4. Restarted `laya serve`; external `/bench` again -> `laya` populated correctly again.
5. `openjev-server`'s PID (66339/66340, started 01:13) was IDENTICAL before, during, and after the whole outage+recovery cycle -- confirms auto-recovery with zero server-process intervention.

### G15 fix, re-verified via a NEW self-constructed fault injection (not reused from E_4)
Read `app.rs:114`: `panic_message(&*payload)` (deref applied -- the fix). Backed up app.rs
(md5sum recorded), injected a 3-line sentinel panic, rebuilt release (exit 0), restarted.
`POST /bench` with the sentinel -> HTTP 500 clean error (not a hang). Log shows the **real**
message: `...respawning with an empty model cache: QA G15 re-verification: deliberate
literal-str panic` -- **not** `<non-string panic payload>`. This is a plain `&str` literal
panic, the exact payload shape G15's original bug always misclassified. Fix confirmed working.

Same event doubles as a G13 regression check: `/health` -> 200 immediately after the panic
(unaffected), next `/bench` -> HTTP 200, `model_load_ms=889` (>0, fresh reload proven), correct
answer. G13 still functions correctly, no regression from this loop's Laya changes.

Cleanup verified, not just claimed: `md5sum` after revert = `aa0c69873b0e29835deec05a66577c4a`,
exact match to the pre-edit hash. `grep -c QA_G15` -> 0. Rebuilt clean (exit 0), restarted
(new PID 67870). Final `grep -rn 'QA_G15|QA_FAULT'` across `apps/`+`crates/` -> no matches.
Backup file deleted.

### Preservation, re-checked AFTER the fault-injection revert+rebuild cycle
- All 3 LLM models: external `/bench` (skip_laya=true) for qwen3-0.6b/minicpm5-2b/qwen3-4b all HTTP 200, all `best_option=B` (Paris), `valid=true`. No regression.
- `cargo test --workspace` on the final clean state: 3 passed, 0 failed.
- `reset_context`: 3 call sites in app.rs (line 232 warmup branch, 248 per-request boundary, 259 between readout/generate -- same 2 semantic invariants D_5 requires, line numbers shifted only because Laya code was inserted above), 2 unchanged in apps/cli/main.rs (84, 100).
- Server ends alive/clean: external `/health` -> 200, SSH-side both `/health` (80 and 8090) -> 200. Exactly one `openjev-server` and one `laya serve` process running.

### Docs (Task 8)
README.md, project-overview-pdr.md, codebase-summary.md all correctly updated to describe the
real `laya serve` + blocking-HTTP-client architecture (not a shell-out), matching what I verified
live. However README.md's specific claim about the bind address is factually wrong -- see G17.

## 2. Unresolved Gaps

- **G17 (new, security-relevant).** `laya serve` binds `0.0.0.0:8090`, not `127.0.0.1:8090` as
  intended/documented. `ss -ltnp` confirms this both before and after a restart during this
  session (not a fluke). Root cause: `laya help`'s SERVE section has no `--host`/`--bind` flag at
  all -- a binary limitation, not a config mistake. Currently not externally reachable (external
  curl from outside the server timed out, unlike port 80 which answered normally), but that
  protection is an UNVERIFIED external firewall/security-group rule this project cannot inspect
  or guarantee -- there is no host-level firewall at all (`iptables`/`ufw` both missing), so it is
  100% reliant on infrastructure outside this repo. Laya's own `serve` help text says auth is off
  by default; if the external filter is ever loosened, `/v1/decide` becomes a fully
  unauthenticated internet-facing inference endpoint (DoS/resource-abuse/model-probing exposure).
  README.md:64's claim ("bound to `127.0.0.1:8090`... not externally reachable by design") is
  factually incorrect and should be corrected to describe the real (external, unverifiable)
  protection mechanism instead of claiming an app-level guarantee that doesn't exist. Non-blocking
  for D_5's functional VRs, but a real infra-security finding -- see Unresolved Questions for
  recommended next steps.
- **G16 (carried, confirmed NOT fixed this loop).** `docs/codebase-summary.md`'s directory tree
  still says `[STUB]` for models/engine/pipeline despite the file's own top paragraph (freshly
  edited this loop) correctly saying "implemented... not stubs". Cosmetic only.
- **G10 (carried, unchanged).** No unit tests for `generate.rs`.
- **G12 (carried, unchanged, larger surface).** `apps/server` still zero unit tests; this loop's
  new `run_laya`/degradation branch (app.rs:270-283) also uncovered in-repo.
- **G14 (carried, unchanged).** Port 80 deviation, previously accepted.
- **G2/G11 (carried, unchanged).** Out of this loop's scope.

## 3. Regressions

None. All Loop 1-4 preservation constraints re-verified and hold: build/test clean, all 3 models
correct via both surfaces, `reset_context` ordering (semantically unchanged), G13 respawn still
works (re-proven via this loop's own fault injection), 6-crate structure, `Timings` 7 fields (now
genuinely populated for `laya_inference_ms`, `laya_model_load_ms` stays intentionally 0), 4 cached
models, server ends alive/reachable on port 80.

## 4. Runtime Check

- SSH-side (`root@103.146.166.46`, `/tmp/openjev`): `cargo build --workspace` exit 0 (debug and
  release), `cargo test --workspace` 3/3 pass (both before AND after the G15 fault-injection
  cycle). `laya` binary confirmed real via `help`/`info`. `laya serve` confirmed running,
  `/health` 200, `/v1/decide` real response matching `models::laya`'s parser.
- External (Windows machine, not SSH): `/health` on port 80 -> 200 (multiple times, including
  final). `/bench` for all 3 models -> 200, correct. Laya-enabled `/bench` -> populated `laya`
  field matching CLI output exactly. `skip_laya:true` -> `laya:null`. Laya-outage `/bench` -> 200,
  graceful degrade. G15 sentinel panic -> 500 clean, not a hang. Port 8090 external connect -> 8s
  timeout (not externally reachable in practice today, see G17 for why this isn't a guarantee).
- `schema_valid: true` -- E_5 produced against a workspace that built, tested, served real
  external HTTP traffic across all 3 LLM models + the newly-wired Laya method, survived a real
  self-constructed panic-recovery re-test and a real Laya-outage simulation, and ended in a
  clean, reachable, fault-injection-free state on the required Linux server target.

## 5. Assessment

**Accept: yes.** All of D_5's Validation Requirements are independently verified, not trusted
from the Developer's self-report:
- `laya` binary built and `laya serve` running, `/health` -> 200 (VR1).
- `openjev-cli` with Laya enabled produces a populated, real `laya` field with real non-zero
  `laya_inference_ms` (VR2).
- External `/bench` matches the CLI's `laya` shape exactly (VR3).
- `skip_laya:true` -> `laya:null`, timings stay 0 (VR4).
- Laya outage -> readout/generate still succeed, HTTP 200 not 500, auto-recovers without an
  `openjev-server` restart (VR5).
- G15 fix -> real panic message now logged, re-verified via a fresh, independent fault injection
  (not reusing E_4's evidence), fully reverted and checksum-confirmed clean.
- The suspected IDE E0063 diagnostic is confirmed a stale-cache false alarm, same pattern as
  Loop 4's E0560 -- real `cargo build --workspace` on Linux compiles clean.
- All preservation constraints hold: 3 LLM models still correct, `reset_context` semantics
  unchanged, `cargo test --workspace` passes, 6-crate structure/`Timings` shape/4 cached models
  intact, server ends alive and externally reachable on port 80.

One new gap found through independent verification, assessed as security-relevant and
non-trivial: **G17**, `laya serve`'s actual bind address (`0.0.0.0:8090`) contradicts both D_5's
task intent and README.md's explicit documentation ("bound to 127.0.0.1... by design"), and the
binary provides no way to fix this from the CLI. Currently mitigated only by an external,
unverified firewall rule -- not an in-repo/app-level guarantee. This does not block D_5's
functional acceptance (all VRs pass) but should not be silently accepted the way G14 (port 80)
was, since G14 involves an authenticated-by-design-scope service on a known port, while this
involves an explicitly no-auth-by-default inference endpoint whose exposure was believed (per the
task doc and README) to be structurally impossible and is not. Recommend escalating to the
planner for a same-loop or next-loop decision (fix the doc's claim at minimum; consider a
loopback-only reverse-proxy wrapper or `LAYA_API_KEY` for defense-in-depth). **G16** (docs
self-contradiction) carried forward unfixed, confirmed by direct re-read, still cosmetic-only.

## Unresolved Questions

- G17: is a loopback-only reverse-proxy wrapper (e.g., `socat`/`nginx` binding `127.0.0.1` and
  forwarding to the `laya` process, since the binary itself has no `--host` flag) worth the added
  operational complexity, given the current external-firewall mitigation appears to be working
  today? Or is documenting-and-accepting (matching G14's precedent) sufficient once README's
  factually-wrong "by design" claim is corrected to describe the real (external, unverified)
  mechanism? Planner call -- I lean toward at minimum fixing the doc claim immediately (cheap,
  high-value: stops a future reader from over-trusting a guarantee that doesn't exist) and
  deferring the reverse-proxy decision.
- Should someone with access to the cloud security-group configuration confirm (and ideally
  document in this repo, e.g. `docs/deployment-guide.md`) that port 8090 is deliberately closed,
  rather than leaving this project blind to a rule it cannot inspect from inside the VM?
- Task 2 (cold `laya decide` CLI sanity-timing, ~39.6s per README) was not independently re-timed
  by me this loop -- I verified the equivalent (and more operationally relevant) warm `/v1/decide`
  HTTP path extensively instead, and `laya info`/`laya help` prove the binary itself is real and
  functional. Low-priority gap in my own verification coverage, not a functional concern; flagging
  for completeness in case the exact cold-CLI number matters for a future latency-budget decision.
</content>
