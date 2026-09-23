---
loop: 11b
status: pending
preservation_constraints:
  - "openjev-server and laya serve (systemd) BOTH stay live and unaffected"
  - "All 3 LLM models + existing Laya still correct via apps/cli and apps/server"
  - "cargo build --workspace (debug+release) and cargo test --workspace still pass"
  - "DO NOT touch crates/laya-native/** this loop — Loop 11 is actively editing graph.rs/gguf.rs there in parallel; a conflicting edit would corrupt its work"
---

## Objective
Build a `logger.rs` fine-grained perf-logging infrastructure (1-minute rotating chunks); wire
it into the parts of the codebase NOT currently being edited by Loop 11.

User's explicit ask: log everything possible, chunked into 1-minute files, so the main agent
(coordinator) can periodically read them, find which functions/logic have high latency, and
keep directing optimization "down to the microsecond" — with special emphasis on the
ggmlc-replacement engine work (`crates/laya-native`), which is exactly what Loop 11 is
currently editing. To avoid a file conflict, THIS loop builds the logger infrastructure itself
plus wires it into everything ELSE (engine, pipeline, apps/server's hot paths); wiring it into
`laya-native` specifically is Loop 12's first task, once Loop 11 has merged.

## Tasks
1. **Design + implement `crates/timing/src/logger.rs`**: a lightweight, low-overhead perf
   logger. Requirements:
   - A `PerfSpan` RAII guard (`PerfSpan::start("module::function_or_block_name")` → on `Drop`,
     records `{timestamp_micros, span_name, duration_micros, thread_id}` plus optional
     key-value metadata (`span.set("model", "qwen3-0.6b")` etc.) into a channel.
   - A background writer thread (started once, e.g. via `OnceLock`/lazy init) that drains the
     channel and appends JSONL records to `logs/perf/perf-<YYYYMMDD-HHmm>.jsonl`, rotating to a
     NEW file every 1 minute (wall-clock minute boundary, not "1 minute since first write") —
     never blocking the hot path: the `PerfSpan::drop` only does a cheap channel `send`, all
     file I/O happens on the background thread.
   - Must be safe to call from many threads concurrently (the actor/worker-thread pattern in
     `apps/server`, the various pipeline/engine call sites) — use an MPSC channel, not a shared
     mutex around file I/O.
   - Keep the public API to instrument a call site trivially, e.g. `let _s =
     perf_span!("engine::decode_prompt");` (a small macro wrapping `PerfSpan::start` with
     `module_path!()`/`function name` boilerplate) — must be cheap enough to sprinkle liberally
     without measurably slowing down the paths it instruments (benchmark the overhead itself —
     see Validation Requirements).
2. **Instrument the CURRENTLY-STABLE parts liberally** (everywhere it's safe to edit right now):
   `crates/engine` (model load, tokenize, decode_prompt, decode_next, sample_greedy,
   reset_context, apply_chat_template), `crates/pipeline` (`run_readout`, `run_generate`,
   `constrained_softmax`, `strip_think`, `parse_and_validate`), `crates/models` (
   `ensure_downloaded`, the existing `laya.rs` HTTP-client path — NOT `crates/laya-native`),
   `apps/server`'s worker loop and `run_bench`. Every meaningfully-sized block of logic should
   get a span — the user's explicit ask is "no un-optimized logic without visibility," so err
   on the side of instrumenting more, not less.
3. **Add a small log-analysis helper** (a script is fine — `scripts/analyze-perf-logs.sh` or
   `.py`, whichever is simpler) that reads a window of `logs/perf/*.jsonl` files and prints a
   sorted summary: span name → count, mean/p50/p95/max duration — this is what the main agent
   will actually use each cycle to find hot spots, so make the output genuinely scannable, not
   just a raw dump.
4. **Measure and report the logger's own overhead**: with instrumentation on vs. off (behind a
   cheap runtime check, e.g. an env var `PERF_LOG=1`, so it can be disabled with near-zero cost
   when not wanted), run the existing benchmark harness (`scripts/bench-harness.sh`) and confirm
   total latency doesn't meaningfully regress. If it does, redesign before calling this done —
   a profiler that distorts the very latencies it's supposed to measure is worse than useless.
5. **Deploy + verify on the server**: rebuild, restart the systemd services, confirm
   `logs/perf/` starts filling with real 1-minute-chunked JSONL files under real traffic
   (run the benchmark harness once to generate data), and that the analysis script produces a
   sensible report from that real data.

## Preservation
See frontmatter — `crates/laya-native/**` is OFF LIMITS this loop.

## Validation Requirements
- Given the logger enabled, When the benchmark harness runs, Then `logs/perf/` contains
  correctly 1-minute-chunked JSONL files with real span data from `engine`/`pipeline`/`models`/
  `apps/server`.
- Given the logger's overhead measurement, When compared enabled vs. disabled, Then the
  reported latency delta is small and explicitly stated (not assumed zero).
- Given the analysis script run against real generated data, Then it produces a readable
  span-name → latency-stats summary, not a raw log dump.
- Given production services after this loop's changes, When `curl`'d externally, Then they
  still respond correctly (no regression from adding instrumentation).

## Out-of-scope
- Instrumenting `crates/laya-native` — Loop 12's first task, after Loop 11 merges.
- Any automated "auto-optimize based on logs" logic — this loop builds the OBSERVABILITY tool;
  the main agent (human-in-the-loop coordinator) reads it and decides what to optimize next,
  per the user's own framing ("để main agent điều phối").
- Long-term log retention/rotation policy beyond "one file per minute" — no cleanup/archival
  logic needed yet.
