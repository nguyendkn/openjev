---
loop: 4
status: pending
preservation_constraints:
  - "apps/server stays live and curl-reachable on 103.146.166.46:80 throughout (restart is OK if needed for the new build, but end state must be reachable again)"
  - "Qwen3-0.6B readout+generate still correct via both apps/cli and apps/server"
  - "reset_context still called at the top of every server request AND between readout/generate"
  - "cargo build --workspace (debug+release) and cargo test --workspace still pass on Linux server"
  - "6-crate structure, Timings 7 fields, 4 cached models, ggmlc-run binary intact"
  - "G3/G4/G5 stay closed"
---

## Objective
MiniCPM-2B + Qwen-4B real inference wired (both apps); fix G13 worker-thread SPOF.

DoD requires all 3 LLM models benchmarked, not just Qwen3-0.6B. Extend the proven
`engine`/`models`/`pipeline` path (already correct for Qwen3-0.6B) to the other 2 registered
LLM entries — this should be mostly registry wiring + verification, not new architecture,
since Loop 2 already built the generic path. Also close G13 (`apps/server`'s single worker
thread has no panic supervision — if it panics on a bad request, every subsequent request
hangs forever waiting on a dead channel) before the tuning loop (Loop 6+) hammers this
component repeatedly.

## Tasks
1. **Verify MiniCPM-2B and Qwen-4B load and run correctly** via `apps/cli` on the Linux
   server (both pipelines, same obvious-answer sanity prompt style as Qwen3-0.6B). These
   models are LARGER (1.45GB, 2.33GB) — expect longer `model_load_ms`/`generation_ms`, that's
   expected and fine, not a bug. If either model's chat template or tokenization differs
   meaningfully from Qwen3 (e.g. MiniCPM may not be ChatML), verify `apply_chat_template`/the
   fallback path handles it — don't assume Qwen3's exact template works unchanged for every
   model without checking.
2. **Verify both models via `apps/server`** (`POST /bench` with `"model":"minicpm-2b"` and
   `"model":"qwen3-4b"` — use whatever exact registry key names `crates/models` already
   defines, check first rather than guessing) — confirm lazy-load-and-cache works per-model
   (first request pays load cost, second is fast).
3. **Fix G13 (worker thread panic supervision)**: the single dedicated thread in
   `apps/server` that owns `HashMap<String, Engine>` must not silently die on a panic and
   leave all future requests hanging. Options (pick the simplest correct one): wrap the
   per-request work in `std::panic::catch_unwind` inside the worker loop and reply with an
   error to the `oneshot` sender instead of letting the thread die; OR detect thread death
   (e.g. `JoinHandle` + a supervisor that respawns the worker thread, accepting that a
   respawned worker has an empty model cache) and make `AppState`'s sender-side aware enough
   to surface a clear 500 instead of hanging forever on a dead channel. Prefer
   `catch_unwind`-per-job if it's compatible with `Engine`'s internals (verify no
   `UnwindSafe` issues — `Engine` uses `unsafe`/raw pointers, panic-safety needs a real check,
   not an assumption) — if `catch_unwind` around `Engine` calls is unsound given the unsafe
   lifetime pattern, use the respawn-supervisor approach instead and document why.
4. **Quick pass on G9** (stale docs): update `docs/codebase-summary.md` and
   `docs/project-overview-pdr.md` to reflect current reality (engine/models/pipeline/server
   are implemented, not stubs; 3 LLM models working; Unresolved #1-5 resolution status) — keep
   this light, don't let it eat the loop's time budget; if it doesn't fit, leave as an
   explicit gap rather than a half-edit.

## Preservation
See frontmatter.

## Validation Requirements
- Given `openjev-cli --model minicpm-2b --prompt "..." --options A,B --format json` run on
  the server, When executed, Then valid JSON with both readout+generate populated, no crash.
- Given the same for `qwen3-4b`, Then likewise valid.
- Given `POST /bench` with each of the 3 model names via `apps/server`, When run (external
  curl, not SSH), Then each returns 200 + valid `BenchReport`.
- Given a request that triggers a real panic inside the worker thread (construct one
  deliberately to prove the fix — e.g. a malformed prompt that hits a known edge case, or a
  temporary code-level fault injection reverted after the test), When it happens, Then the
  server does NOT hang forever on subsequent requests — either the panicking request gets a
  clean error response and the worker keeps serving, or the worker respawns and later
  requests succeed within a bounded time. Prove this empirically, don't just reason about it.

## Out-of-scope
- Laya — Loop 5+.
- Performance tuning — later.
- G10 (generate.rs tests), G12 (apps/server tests), G14 (port 80) — tracked, not this loop's
  focus unless trivially foldable.
