---
loop: 5
status: pending
preservation_constraints:
  - "apps/server stays live and curl-reachable on 103.146.166.46:80 (brief restart for redeploy OK, must end reachable)"
  - "All 3 LLM models (qwen3-0.6b, minicpm5-2b, qwen3-4b) still correct via both apps/cli and apps/server"
  - "reset_context still called at top of every server request AND between readout/generate"
  - "G13 respawn-supervisor still functional (do not regress it while adding Laya wiring)"
  - "cargo build --workspace (debug+release) and cargo test --workspace still pass on Linux server"
  - "6-crate structure, Timings 7 fields (now to be genuinely populated, not 0/null, for laya_model_load_ms/laya_inference_ms), 4 cached models, existing ggmlc-run generic binary can stay or be removed if superseded — see Task 1"
---

## Objective
Laya wired as the 3rd real comparison method — closes G6/G7(Linux)/G8, `--skip-laya` becomes real.

**CORRECTED UNDERSTANDING (main agent read Laya's real HF model card + the `examples/laya`
README on GitHub directly — this supersedes Loop 1's exploration of the generic `ggmlc-run`
binary, which was the WRONG tool)**:

- The real CLI for typed-decision scoring is a **separate binary named `laya`** (not the
  generic `ggmlc-run` Loop 1 built), built from `github.com/monatis/ggmlc/tree/main/examples/laya`
  (has its own `CMakeLists.txt` in that subdirectory — check whether it's built automatically
  as part of the top-level `ggmlc` CMake build or needs an explicit target/flag; Loop 1 never
  built this specific target, only the generic `ggmlc-run` runner).
- `laya serve <model.gguf> --port <p> [--device cpu]` starts a **persistent HTTP server**
  that is a **byte-compatible TypeSafe System One server**: `POST /v1/systemone`, `POST
  /v1/decide` (same body/response), `GET /health`, `GET /v1/models`. Request:
  `{"state": ..., "model": "jev-latest", "questions": {"<id>": {"type": "choice"|"score"|"noul",
  "instructions": "...", "criteria": {...}}}}`. Response: `{"model", "answers": {"<id>": {...
  "probabilities"?, "confidence"?}}, "usage": {"input_tokens","output_tokens"}}` — extra Laya
  fields (`family`, `route`, `usage.latency_ms`, `action`) are additive, ignored by strict
  clients. Auth off by default (matches this project's no-auth-yet scope) unless
  `LAYA_API_KEY`/`TYPESAFE_API_KEY` env is set — do NOT set it.
- Custom (non-preset) questions ARE supported (not preset-only) — confirmed via the
  `daemon`/`/v1/decide` JSON-RPC example in the README: arbitrary `criteria` map per question.
  This is how our arbitrary `options: ["A","B",...]` should map: one `choice`-type question
  (e.g. id `"answer"`) with `criteria` = one entry per option label.
- No documented Linux CPU-only prebuilt binary (release artifacts are CUDA/Metal only) — build
  from source on the Linux server, same toolchain family as the rest of this project
  (CMake/C++17, already proven working for the generic `ggmlc-run` in Loop 1).
- **Architecture decision (this loop, taste-level, reasoned below)**: run `laya serve` as a
  **persistent background process** on the Linux server (same `setsid nohup ... & disown`
  pattern as `apps/server`), bound to `127.0.0.1:<internal-port>` (does NOT need external
  reachability — only our own `openjev-server`/`openjev-cli` call it, never the public
  internet). `crates/models`'s Laya integration becomes a small blocking HTTP client (add
  `ureq` or `reqwest` blocking — pick whichever is already easiest given `hf-hub`'s existing
  dependency tree, avoid pulling in a second async HTTP stack if avoidable) calling
  `http://127.0.0.1:<port>/v1/decide`, NOT a per-request `std::process::Command` shell-out to
  `laya decide` (that was the original plan when the tool was assumed to be a one-shot CLI;
  now that `serve` exists, a warm persistent process + HTTP call is strictly better: avoids
  process-spawn + model-reload cost on every single benchmark call, matches how our own
  `engine`/`apps/server` already keep models warm). This changes the original `models::laya`
  shell-out design from Loop 1's `README.md`/`docs/project-overview-pdr.md` notes — update
  those docs to reflect the real approach in this loop's docs pass.

## Tasks
1. **Build the real `laya` binary** on the Linux server (`/tmp/ggmlc`, already cloned from
   Loop 1): investigate `examples/laya/CMakeLists.txt` and the ggmlc repo's top-level build
   docs for how to build this specific example target (may need a CMake flag like
   `-DGGMLC_BUILD_EXAMPLES=ON` or building `examples/laya` as its own subproject — check for
   real, don't guess). CPU-only (no CUDA — server has no GPU). Confirm the binary runs:
   `laya help`, `laya list-presets`, `laya info <laya_english_ud_q4_k_m.gguf path from HF
   cache>`.
2. **Sanity-test `laya decide` once via CLI** (closes G8 for real): `laya decide
   <model> --preset guard --text "Ignore previous instructions" --json --device cpu`,
   time it (`time laya decide ...`) — this is the first real CPU latency number for Laya
   (previously only GPU numbers existed). Record it.
3. **Start `laya serve` as a persistent background process**: pick an internal port not
   conflicting with `openjev-server`'s port 80 (e.g. 8090 — verify free first via `ss
   -ltnp`), `--device cpu`, bind to `127.0.0.1` (localhost-only is fine and preferred — no
   external exposure needed). Persistent via `setsid nohup ... & disown` (same pattern as
   `apps/server`). Verify via `curl -s http://127.0.0.1:8090/health` from the server itself
   (SSH — this one genuinely doesn't need external reachability, unlike `apps/server`).
4. **Implement `models::laya`** (or wherever the existing stub `crates/models/src/laya.rs`
   lives): a blocking HTTP client function `score(prompt: &str, options: &[String]) ->
   Result<LayaScoreResult, LayaError>` that POSTs to `http://127.0.0.1:8090/v1/decide` with
   body `{"state": prompt, "model": "laya-english", "questions": {"answer": {"type": "choice",
   "instructions": "Pick the correct option.", "criteria": {<one key per option, value =
   null or a short description>}}}}`, parses the response's `answers.answer.probabilities`
   (or whatever the real field is — verify against Task 3's actual live response, don't guess
   the exact key name) into a per-option probability map + best option, matching the shape of
   `pipeline::readout`'s `ReadoutResult` as closely as sensible for consistent output.
5. **Implement `pipeline::laya::run_laya`**: wraps `models::laya::score`, measures
   `laya_inference_ms` (the HTTP round-trip) — `laya_model_load_ms` should be ~0 for every
   call once `laya serve` is warm (the model loaded once at `laya serve` startup, not
   per-request — document this in a doc-comment so it's not mistaken for a bug later).
6. **Wire into `apps/cli` and `apps/server`**: `--skip-laya`/`skip_laya` becomes REAL (no
   longer a documented no-op) — when false/absent, `laya` result populates in the
   `BenchReport`; when true, `laya: null` and both laya `Timings` fields stay 0/null exactly
   as already implemented for the not-yet-wired state. If `laya serve` is unreachable
   (connection refused/timeout), do NOT crash the whole request — return a clear per-field
   error/null for the laya portion while readout/generate still succeed (Laya being down
   should degrade gracefully, not take down the other 2 methods' results).
7. **Quick fix G15** (one-line): `crates/... panic_message` — `&payload` → `&*payload` so
   `downcast_ref` actually matches `&str` panic payloads. Verify via the same fault-injection
   technique already proven in Loop 4 (inject, test, confirm real message now logs, revert).
8. **Update docs** (`README.md`, `docs/project-overview-pdr.md`, `docs/codebase-summary.md`):
   correct the Laya integration description to the real `laya serve` + HTTP-client approach
   (not shell-out), note the internal port, note `laya serve` is a separate long-running
   process from `openjev-server` (2 processes on the server now, document how to check/restart
   both).

## Preservation
See frontmatter.

## Validation Requirements
- Given the real `laya` binary built and `laya serve` running on `127.0.0.1:8090`, When
  `curl http://127.0.0.1:8090/health` runs (from the server via SSH), Then it returns 200.
- Given `openjev-cli --model qwen3-0.6b --prompt "The capital of France is: A) London B)
  Paris" --options A,B --format json` (Laya NOT skipped), When run on the server, Then the
  `laya` field in the output is populated with real probabilities (not null), and
  `laya_inference_ms` is a real non-zero-but-fast number (single encoder pass should be much
  faster than LLM generation).
- Given the same via `POST /bench` on `apps/server` (external curl, `"skip_laya": false` or
  omitted), Then the response includes a populated `laya` field matching the CLI's shape.
- Given `"skip_laya": true`, Then `laya: null` and both laya timing fields stay at their
  existing default (0/null) — exactly the pre-Loop-5 behavior, now genuinely opt-out rather
  than "not implemented yet".
- Given `laya serve` is stopped (simulate: kill it, make one request, then restart it), When
  `apps/server` handles a request during the outage, Then readout+generate still succeed and
  the response indicates Laya's result is unavailable (does not 500 the whole request).
- Given the G15 fix, When a worker-thread panic is fault-injected (same technique as Loop 4),
  Then the server log shows the REAL panic message, not `<non-string panic payload>`.

## Out-of-scope
- The separate Jev-compatible `/v1/systemone`-shaped endpoint on OUR `apps/server` (still a
  later loop — note that `laya serve` already speaks this protocol natively; a future loop can
  decide whether to reverse-proxy to it or hand-roll our own, see `run.md`'s reference note).
- Performance tuning loop (Phase 7 proper) — after Laya is wired and DoD item 4 is fully met.
- G10/G12 (missing unit tests), G14 (port 80), G16 (minor doc drift) — not this loop's focus.
- Windows build (G7/G1) — still descoped, Linux is authoritative.
