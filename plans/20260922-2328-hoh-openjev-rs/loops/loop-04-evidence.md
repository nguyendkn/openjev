---
loop: 4
candidate: A_4
status: passed
builder: Developer
assessor: QATester
verified_behaviors:
  - "Build (debug), real cargo on the Linux server: `cargo build --workspace` -> exit 0, only pre-existing warnings (`timing` unused `Instant` import, `engine` deprecated `Special`). This DISPROVES the IDE-reported `E0560 no such field` at crates/engine/src/lib.rs:119 flagged in the task context as a suspected rust-analyzer cache artifact -- a real `cargo build` on the actual Linux server compiles clean, no E0560 or any struct-field error. Confirmed false alarm."
  - "Build (release): `cargo build --workspace --release` -> exit 0, same warnings only."
  - "Test: `cargo test --workspace` -> 3/3 pass (pipeline::readout tests: empty_indices_returns_empty, picks_higher_logit, uniform_when_all_neg_inf), 0 failures -- identical to E_2/E_3 baseline, no regression."
  - "shared_backend() fix (BackendAlreadyInitialized) confirmed by direct source read, crates/engine/src/lib.rs: `static BACKEND: OnceLock<LlamaBackend>` (line 16), `fn shared_backend()` (lines 21-30) lazily inits once via `BACKEND.get_or_init`. `struct Engine` (lines 78-82) fields are exactly `{ ctx, model, n_vocab }` -- NO `backend` field -- matching `Ok(Self { ctx, model, n_vocab })` at construction (lines 114-118). `Engine::load` calls `shared_backend()?` (not `LlamaBackend::init()` per-Engine) -- this is the real fix for the multi-model-cache BackendAlreadyInitialized bug the doc comment describes. Not just self-report: independently re-tested empirically below (all 3 models loaded in one process without error)."
  - "All 3 registered LLM models verified via external `POST /bench` (curl from this Windows machine, not SSH), fresh in THIS QA session: qwen3-0.6b -> HTTP 200, readout.best_option=Paris (prob 0.99999845), generate.parsed.answer=Paris, valid=true. minicpm5-2b -> HTTP 200, readout.best_option=Paris (0.99362224), generate.parsed.answer=Paris, valid=true. qwen3-4b -> HTTP 200, readout.best_option=Paris (0.9999827), generate.parsed.answer=Paris, valid=true. All 7 Timings fields present in each response."
  - "Lazy-load+cache genuinely exercised (not just claimed): `grep 'loaded meta data'` on `logs/server.log` shows each of the 3 GGUF files (Qwen3-0.6B-Q8_0.gguf, MiniCPM5-2B-Q4_K_M.gguf, Qwen3-4B-Q4_K_M.gguf) loaded exactly ONCE in the server's lifetime; my 3 `/bench` calls above and the earlier main-agent verification all hit warm cache (model_load_ms=0), consistent with one real load per model, no reload thrashing, no BackendAlreadyInitialized error anywhere in the log."
  - "apps/cli independently re-verified for BOTH new models (not just re-trusting the self-report), on the Linux server: `openjev-cli --model minicpm5-2b ...` -> valid JSON, probs/best_option/tokens_generated=62 EXACTLY matches the server's `/bench` response for the same prompt. `openjev-cli --model qwen3-4b ...` -> valid JSON, probs/tokens_generated=149 EXACTLY matches server's response. Both exit 0, no crash. Confirms D_4 VR1/VR2 and CLI/server DTO parity holds for the 2 new models, same as it already did for qwen3-0.6b."
  - "Chat-template-differs-per-model concern (D_4 Task 1) independently checked, not assumed: `grep tokenizer.chat_template logs/server.log` shows MiniCPM5's GGUF (line ~1092) carries its OWN embedded chat_template metadata (starts `{{- bos_token }}...`), distinct in content from Qwen3's (starts `{%- if tools %}...`). `Engine::apply_chat_template` (crates/engine/src/lib.rs) calls `self.model.chat_template(None)` generically per-model rather than hardcoding Qwen3's template, and both models produced valid, correct JSON output -- confirms the generic path, not a Qwen3-only assumption, is what's actually running."
  - "Registry key names confirmed by direct grep, not trusted from any report: crates/models/src/download.rs:47,54,61,68 -- `id: \"qwen3-0.6b\"`, `id: \"qwen3-4b\"`, `id: \"minicpm5-2b\"`, `id: \"laya\"`. Exactly matches what was used in every /bench and CLI call above and in the prior context's curl tests."
  - "G13 fix (respawn-supervisor) independently proven via a SELF-CONSTRUCTED fault injection, not reused evidence: temporarily added `if req.prompt == \"__QA_FAULT_INJECT_PANIC__\" { panic!(...) }` at the top of `run_bench` (apps/server/src/app.rs), rebuilt release, restarted server. POST /bench with that sentinel prompt -> HTTP 500 `{\"error\":\"engine worker thread dropped the response\"}` in 0.085s (clean error, not a hang). Immediately re-curled /health -> HTTP 200 in 0.09s (async listener/main thread unaffected by the worker-thread panic). Immediately re-curled /bench with a normal qwen3-0.6b prompt -> HTTP 200, correct Paris answer, `model_load_ms=879` (>0, proving the model cache was genuinely wiped and reloaded by the respawn, exactly matching the documented 'fresh empty HashMap on every worker_loop re-entry' design, not just no-op recovery). Server log confirmed: `thread '<unnamed>' panicked at apps/server/src/app.rs:202:9` + `engine worker thread panicked, respawning with an empty model cache: ...`. Self-healing proven empirically, no operator/process restart needed for recovery."
  - "Fault-injection code fully reverted after testing: `diff` against a pre-edit backup showed exactly the 3 injected lines and nothing else; `md5sum` of apps/server/src/app.rs after revert matches the pre-edit hash (ecc84964578971e2afd6dd715cc9702d); rebuilt release clean (exit 0); server restarted on the clean binary; `grep -c QA_FAULT_INJECT apps/server/src/app.rs` -> 0; temp test log/scratch files (`logs/server_fault_test.log`, `/tmp/panictest*`) deleted; final sanity `/bench` (qwen3-0.6b) after clean restart -> HTTP 200, correct."
  - "Preservation: reset_context() ordering unchanged -- app.rs:237 (top of run_bench's per-request path, after model-load/cache branch, before tokenize) and app.rs:248 (between readout and generate) both present and correctly ordered, same as E_3's verified positions. apps/cli/src/main.rs reset_context calls unchanged at lines 81, 97."
  - "Preservation: 6-crate workspace unchanged (`Cargo.toml` members = apps/cli, apps/server, crates/engine, crates/models, crates/pipeline, crates/timing). `Timings` struct still exactly 7 fields + `Serialize`/`Clone`/`Copy`/`Debug`, `Default` all-zero. All 4 model cache dirs present (`~/.cache/huggingface/hub/models--{Qwen--Qwen3-0.6B-GGUF,Qwen--Qwen3-4B-GGUF,openbmb--MiniCPM5-2B-GGUF,mys--laya-GGUF}`). `ggmlc-run` binary present, executable, unchanged at `/tmp/ggmlc/build/runtime/ggmlc-run`."
  - "Preservation: no new unsafe outside what was already documented -- `grep -rn unsafe apps/server/src crates/engine/src` -> only the 2 pre-existing doc-comment mentions + the 1 pre-existing `transmute` in engine/lib.rs:105 (Engine's self-referential-struct pattern, present since Loop 2/G11). No new unsafe in apps/server."
  - "G3/G4/G5 stay closed: crates/models/src/download.rs registry data (hf-hub usage, revisions, licenses) unchanged from E_2/E_3's verified state; crates/engine's logits/KV-cache API usage (`ctx.get_logits_ith`, `ctx.clear_kv_cache`) unchanged; the only engine.rs change this loop (shared_backend/OnceLock) is additive and doesn't touch the previously-verified hf-hub/logits/licensing surfaces."
  - "Server left alive and clean at end of QA session: `curl /health` final check -> HTTP 200. Process is the clean rebuild (post-revert), started 00:55, `ps` confirms single process, `grep -c QA_FAULT_INJECT` on live source -> 0."
  - "G9 (docs) substantively addressed: docs/codebase-summary.md and docs/project-overview-pdr.md both re-read in full. Per-Crate State tables now correctly say IMPLEMENTED (not STUB) for engine/models/pipeline/apps/server, with accurate Loop-4-specific detail (shared_backend fix, G13 panic supervision, 3-model verification, correct registry ids incl. `minicpm5-2b` not `minicpm-2b`). project-overview-pdr.md's Open Risks table items 1-5 correctly marked RESOLVED with real evidence citations, item 6-8 (Laya) correctly left open. Known Issues section correctly lists G9/G10/G12/G14/Laya with accurate status. This is real content accuracy, not a rubber-stamp relabel."
unresolved_gaps:
  - "G15 (new). `panic_message()` (apps/server/src/app.rs:108, `panic_message(&payload)`) has a real bug, NOT merely cosmetic: it passes `&payload` where `payload: Box<dyn Any + Send>` instead of `&*payload`, and `downcast_ref::<&str>()`/`downcast_ref::<String>()` on the resulting `&Box<dyn Any+Send>`-shaped reference ALWAYS returns None regardless of the actual panic payload's real type -- independently reproduced in 3 isolated minimal Rust programs on the same server/toolchain (rustc 1.98.1): `panic_message(&*e)` correctly extracts a plain-literal panic message every time; the otherwise-identical `panic_message(&e)` (no deref) ALWAYS falls through to the `<non-string panic payload>` fallback, even for a trivial `panic!(\"literal\")`. This was empirically confirmed against the real server too: my fault-injection panic (`panic!(\"QA fault injection: deliberate panic to test G13 supervisor respawn\")`, a plain `&'static str` literal, confirmed as such by the default panic hook's own stderr line) was logged by `supervisor_loop`'s own `eprintln!` as `engine worker thread panicked, respawning with an empty model cache: <non-string panic payload>` -- the diagnostic message is silently wrong on every panic, not just this one. Impact: does NOT affect the respawn/recovery mechanism itself (verified separately above -- that works correctly), but means every future production panic's logged cause will be useless for debugging. Fix: change `panic_message(&payload)` to `panic_message(&*payload)` (or `payload.as_ref()`) at app.rs:108. Assessed independently, NOT accepting the Developer's 'cosmetic' framing at face value -- this is a real, reproducible bug in an observability path, though non-blocking for G13's core acceptance criteria."
  - "G16 (new, minor). docs/codebase-summary.md self-contradicts: the Directory Tree ASCII diagram (lines 16-18) still labels `models/`, `engine/`, `pipeline/` as `[STUB]`, while the Per-Crate State section immediately below (lines 42+, same file) correctly marks all three IMPLEMENTED. Residual leftover from the G9 doc-refresh -- the detailed prose was updated but the summary ASCII tree at the top was missed. Purely cosmetic/consistency issue, not a functional gap, but leaves the file self-contradictory to a reader who only skims the tree."
  - "G10 (carried, unchanged). crates/pipeline/generate.rs still has no committed unit tests. Not this loop's scope."
  - "G12 (carried, unchanged). apps/server/src/{app,main}.rs still has zero committed unit tests -- the new supervisor_loop/panic_message/worker_loop logic (all new this loop) also has no in-repo regression protection; only my ad hoc black-box fault-injection test covers it, and that test's own artifacts were reverted/deleted per the read-only QA constraint, so no regression protection persists in-repo for G13's own fix."
  - "G14 (carried, unchanged). Port 80 deviation, previously accepted -- unaffected this loop."
regressions: []
schema_valid: true
---

## 1. Verified Behaviors

### Build/test on the real Linux server (resolves the IDE E0560 suspicion)
- `cargo build --workspace` (debug) -> **exit 0**. Only pre-existing warnings (`timing`'s unused `Instant` import, `engine`'s deprecated `Special` enum use) -- same as every prior loop's baseline. **No `E0560` or any struct-field error.** The IDE diagnostic flagged in the task context is confirmed a stale rust-analyzer cache artifact, not a real compile error: `struct Engine { ctx, model, n_vocab }` (crates/engine/src/lib.rs:78-82) and its construction `Ok(Self { ctx, model, n_vocab })` (lines 114-118) match exactly, and the real compiler agrees.
- `cargo build --workspace --release` -> exit 0, same warnings only.
- `cargo test --workspace` -> `3 passed; 0 failed` (all in `pipeline::readout`), identical to E_2/E_3's baseline. No regression.

### shared_backend()/BackendAlreadyInitialized fix (white-box + empirical)
Read `crates/engine/src/lib.rs` directly. `static BACKEND: OnceLock<LlamaBackend> = OnceLock::new()` (line 16); `fn shared_backend()` (lines 21-30) returns the cached instance or inits once via `LlamaBackend::init()` + `BACKEND.get_or_init`. `Engine::load` (line ~85) calls `shared_backend()?` instead of owning its own `LlamaBackend`. `struct Engine`'s fields are exactly `{ ctx: LlamaContext<'static>, model: Box<LlamaModel>, n_vocab: i32 }` -- **no `backend` field**, consistent with the doc comment's stated reason (the old owned-`backend`-per-`Engine` design broke a second model's `Engine::load` while a first model's `Engine` was still cached, reproducing `BackendAlreadyInitialized`). Empirically re-confirmed below: all 3 LLM models loaded and served correctly in the SAME server process, no such error anywhere in `logs/server.log`.

### All 3 LLM models, external curl (fresh in this QA session, not reused screenshots)
```
qwen3-0.6b   -> HTTP 200, best_option=Paris (0.99999845), generate=Paris, valid=true
minicpm5-2b  -> HTTP 200, best_option=Paris (0.99362224),  generate=Paris, valid=true
qwen3-4b     -> HTTP 200, best_option=Paris (0.9999827),   generate=Paris, valid=true
```
All 7 `Timings` fields present in each. `grep 'loaded meta data' logs/server.log` shows each of the 3 GGUF files loaded exactly once -- genuine lazy-load-and-cache, not reload-per-request.

### apps/cli, independently re-run for the 2 new models
- `openjev-cli --model minicpm5-2b ...` -> exit 0, valid JSON, `probs`/`best_option=Paris`/`tokens_generated=62` -- **exact match** to the server's `/bench` response for the identical prompt.
- `openjev-cli --model qwen3-4b ...` -> exit 0, valid JSON, `tokens_generated=149` -- **exact match** to the server's response.
- Confirms D_4's VR1/VR2 and CLI/server DTO parity for both new models.

### Chat-template-per-model concern (D_4 Task 1), checked not assumed
`grep tokenizer.chat_template logs/server.log`: MiniCPM5's GGUF carries its own distinct embedded template (`{{- bos_token }}...`), different from Qwen3's (`{%- if tools %}...`). `Engine::apply_chat_template` calls `self.model.chat_template(None)` generically per model (not a hardcoded Qwen3 template), and both models produced valid correct output -- the generic path is what's actually exercised, not a lucky Qwen3-only coincidence.

### Registry key names, grep-confirmed
`crates/models/src/download.rs:47,54,61,68` -- `id: "qwen3-0.6b"`, `"qwen3-4b"`, `"minicpm5-2b"`, `"laya"`. Matches every call made above.

### G13 respawn-supervisor, self-constructed fault injection (not reused evidence)
Temporarily added `if req.prompt == "__QA_FAULT_INJECT_PANIC__" { panic!("QA fault injection: deliberate panic to test G13 supervisor respawn"); }` at the top of `run_bench` (apps/server/src/app.rs), rebuilt release (`cargo build --workspace --release` -> exit 0), restarted the server with the fault-injected binary.

1. `POST /bench {"prompt":"__QA_FAULT_INJECT_PANIC__",...}` -> **HTTP 500**, `{"error":"engine worker thread dropped the response"}`, **0.085s** (clean error, not a hang).
2. Immediately `GET /health` -> **HTTP 200**, 0.09s (the async main thread/listener was never affected -- panic was confined to the dedicated worker thread as designed).
3. Immediately `POST /bench` with a normal `qwen3-0.6b` prompt -> **HTTP 200**, correct Paris answer, **`model_load_ms=879` (>0)** -- proves the model cache was genuinely wiped and reloaded by the respawn (matches the documented "fresh empty `HashMap` on every `worker_loop` re-entry" design), not a no-op.
4. Server log confirms: `thread '<unnamed>' (...) panicked at apps/server/src/app.rs:202:9` + `engine worker thread panicked, respawning with an empty model cache: ...`.

**Self-healing proven empirically**, no operator/process restart needed for recovery -- this directly satisfies D_4's VR4 ("prove this empirically, don't just reason about it").

### Fault-injection cleanup, verified not just claimed
`diff` against a pre-edit backup showed exactly the 3 injected lines; `md5sum` of `apps/server/src/app.rs` after revert = `ecc84964578971e2afd6dd715cc9702d`, matching the pre-edit hash. Rebuilt release clean (exit 0). Server restarted on the clean binary. `grep -c QA_FAULT_INJECT apps/server/src/app.rs` -> 0. Scratch files (`logs/server_fault_test.log`, all `/tmp/panictest*` on both this machine and the server) deleted. Final sanity `/bench` (qwen3-0.6b) on the clean restarted server -> HTTP 200, correct.

### Preservation
- `reset_context()` ordering unchanged: app.rs:237 (top of `run_bench`, after cache-branch, before tokenize) and app.rs:248 (between readout/generate) both present, correctly ordered -- same positions E_3 verified. `apps/cli/src/main.rs` calls unchanged at lines 81, 97.
- 6-crate workspace unchanged. `Timings` still 7 fields + `Serialize`/`Clone`/`Copy`/`Debug` + all-zero `Default`. All 4 model cache dirs present. `ggmlc-run` binary present, executable, unchanged.
- No new `unsafe`: `grep -rn unsafe apps/server/src crates/engine/src` -> only the 2 pre-existing doc-comment mentions + the 1 pre-existing `transmute` (engine/lib.rs:105, since Loop 2/G11). Nothing new in `apps/server`.
- G3/G4/G5 stay closed: hf-hub usage, logits/KV-cache API usage, and licensing data in `crates/models`/`crates/engine` unchanged from their previously-verified state; this loop's only `engine.rs` change (shared backend) is additive and doesn't touch those surfaces.
- Server left alive and clean: final `curl /health` -> HTTP 200. Process is the post-revert clean rebuild (started 00:55). `grep -c QA_FAULT_INJECT` on live source -> 0.

### G9 (docs), substantively addressed
Re-read both `docs/codebase-summary.md` and `docs/project-overview-pdr.md` in full. Per-Crate State tables now correctly say IMPLEMENTED (not STUB) with accurate Loop-4 detail (shared_backend fix, G13 supervision, 3-model verification, correct `minicpm5-2b` id). `project-overview-pdr.md`'s Open Risks items 1-5 correctly RESOLVED with real evidence citations; 6-8 (Laya) correctly left open. This is real content accuracy, not a relabel -- **but see G16 below for one residual inconsistency found**.

## 2. Unresolved Gaps

- **G15 (new).** `panic_message()` (app.rs:108, `panic_message(&payload)`) is a **real bug, not cosmetic**: passing `&payload` (where `payload: Box<dyn Any + Send>`) instead of `&*payload` means `downcast_ref::<&str>()`/`downcast_ref::<String>()` ALWAYS return `None` regardless of the actual payload type. Independently reproduced in 3 isolated minimal Rust programs on the same server/toolchain: `panic_message(&*e)` correctly extracts a plain-literal message every time; the otherwise-identical `panic_message(&e)` always falls through to `"<non-string panic payload>"`, even for `panic!("literal")`. Confirmed against the real server too: my fault-injection panic (a plain `&'static str` literal, confirmed by the default panic hook's own stderr output) was logged by `supervisor_loop` as `"...respawning with an empty model cache: <non-string panic payload>"` -- wrong every time, not just this once. Does NOT affect the respawn/recovery mechanism's correctness (verified working above), but silently breaks production debuggability for every future panic. **Fix: `panic_message(&payload)` -> `panic_message(&*payload)` at app.rs:108.**
- **G16 (new, minor).** `docs/codebase-summary.md` self-contradicts: Directory Tree ASCII diagram (lines 16-18) still shows `models/`/`engine/`/`pipeline/` as `[STUB]`, while the Per-Crate State section right below (same file) correctly says IMPLEMENTED for all three. Leftover from the G9 refresh -- prose updated, summary tree missed. Cosmetic only.
- **G10 (carried, unchanged).** `crates/pipeline/generate.rs` still has no committed unit tests. Not this loop's scope.
- **G12 (carried, unchanged, now larger surface).** `apps/server` still has zero committed unit tests. This loop ADDED new logic (`supervisor_loop`, `panic_message`, `worker_loop` split) that also has no in-repo regression protection -- only my ad hoc, now-reverted fault-injection test covered it. G15 (the panic_message bug) is exactly the kind of defect committed unit tests would have caught.
- **G14 (carried, unchanged).** Port 80 deviation, previously accepted.

## 3. Regressions

None. All Loop 1-3 preservation constraints (workspace structure, `Timings` shape, cached models, `ggmlc-run`, build/test, `reset_context` ordering in both `apps/cli` and `apps/server`, G3/G4/G5 closure, qwen3-0.6b correctness) re-verified and hold.

## 4. Runtime Check

- External curl (from this Windows machine, NOT SSH): `/health` -> 200 (multiple times, including post-fault-injection and final). `/bench` for all 3 models -> 200, correct, valid `BenchReport`. Fault-injection panic request -> 500 clean error, not a hang.
- SSH-side (`root@103.146.166.46`, `/tmp/openjev`):
  - `cargo build --workspace` (debug) -> exit 0, pre-existing warnings only, **no E0560**.
  - `cargo build --workspace --release` -> exit 0 (both before and after the fault-injection revert cycle).
  - `cargo test --workspace` -> 3/3 pass, 0 failures.
  - `openjev-cli` re-run for `minicpm5-2b` and `qwen3-4b` -> exact match to server responses.
  - `grep`-confirmed: registry ids, `reset_context` call sites, `unsafe` usage, `shared_backend`/`Engine` struct shape, chat-template metadata per model.
  - Fault injection: added, rebuilt, tested (panic -> 500 clean, `/health` unaffected, next request self-healed with reload), fully reverted (md5sum-verified), rebuilt clean, server restarted, final health check 200.
- `schema_valid: true` -- E_4 produced against a workspace that built, tested, served real external HTTP traffic, and survived a real, self-constructed panic-recovery test on the required Linux server target, ending in a clean, reachable state.

## 5. Assessment

**Accept: yes.** All of D_4's Validation Requirements are independently verified, not trusted from the Developer's self-report or reused main-agent evidence:
- MiniCPM-2B (registry id `minicpm5-2b`) and Qwen-4B (`qwen3-4b`) both load and run correctly via `apps/cli`, valid JSON, matching server output exactly.
- Both verified via `apps/server` `/bench` with the real registry key names (grep-confirmed, not guessed).
- The `BackendAlreadyInitialized` bug and its `shared_backend()`/`OnceLock` fix are real: confirmed by source (no `backend` field left on `Engine`) and empirically (all 3 models coexist in one process's cache without error).
- G13's respawn-supervisor is real and proven empirically via a self-constructed fault injection (not reusing the Developer's or main agent's evidence): panic -> clean 500, `/health` stays up, next request self-heals with a fresh reload, no hang, no operator restart needed. Fault-injection code fully reverted and verified clean (md5sum match) before ending.
- The suspected IDE `E0560` diagnostic is confirmed a stale-cache false alarm -- the real Linux `cargo build --workspace` compiles clean.
- G9 (docs) is substantively addressed with accurate content, not a rubber-stamp.
- All preservation constraints from D_4's frontmatter hold: server ends reachable, qwen3-0.6b still correct via both surfaces, `reset_context` ordering unchanged, build/test pass, structural invariants (6 crates, `Timings` 7 fields, 4 cached models, `ggmlc-run` binary) intact, G3/G4/G5 stay closed.

Two new gaps found through independent verification (not accepting self-report at face value): **G15**, a real (not cosmetic) bug in `panic_message()` that silently breaks panic-cause logging for every future worker-thread panic -- non-blocking for G13's core acceptance criterion (recovery works) but should be fixed soon given it directly undermines observability of the very fault this loop was built to survive. **G16**, a minor residual self-contradiction in `docs/codebase-summary.md`'s directory tree. Neither blocks Loop 4 acceptance. G10/G12/G14 carried forward unchanged; G12's surface is now larger since this loop added untested supervisor/panic-recovery logic.

## Unresolved Questions

- Should G15 (`panic_message` bug) be a same-day one-line fix folded into Loop 5's start, given it directly undermines the observability of the fault G13 exists to survive? Recommend yes, trivial fix (`&*payload`), but deferring the priority call to the planner.
- Should G12 (apps/server unit tests) be elevated in priority now that Loop 4 added non-trivial new logic (supervisor_loop/panic_message/worker_loop) with zero committed regression coverage, and this loop's own QA testing already found a real bug (G15) that a unit test would have caught for free? Planner call.
- Is the respawn-with-empty-cache design's per-panic reload cost (confirmed here: ~880ms for qwen3-0.6b, likely several seconds for qwen3-4b) acceptable for production, or should a future loop consider a lighter-weight per-model cache eviction instead of wiping the whole `HashMap` on every panic? Not urgent (panics are expected to be rare), flagging for awareness only.
