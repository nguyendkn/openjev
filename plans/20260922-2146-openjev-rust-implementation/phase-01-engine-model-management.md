---
phase: 1
name: Engine + Model Management (timing, models, engine crates)
status: pending
depends_on: [0]
---

## Context Links
- `../plan.md` § Architecture, § Decisions Locked (incl. #9, Laya)
- `research/researcher-01-llama-cpp-2-engine.md` §1, §3
- `research/researcher-03-model-acquisition.md` §1-4
- `phase-00-spike-verification.md` (must be closed first — supplies pinned model table, confirmed
  crate APIs, confirmed `ggmlc-run` CLI syntax, and the scaffolded `crates/timing`, `crates/models`,
  `crates/engine` manifests)

## Overview
- Priority: P0, blocking Phases 2 and 3.
- Status: pending.
- Fill in the `timing`, `models`, and `engine` library crates (scaffolded empty by Phase 0):
  `timing` — the `Timings` struct; `models` — static registry of 4 models (3 LLMs + Laya-en) +
  `hf-hub` download/cache/checksum **plus the `ggmlc-run` shell-out wrapper for Laya scoring** (no
  separate `laya` crate — merged per direct user request); `engine` — GGUF load + context
  management via `llama-cpp-2` for the 3 LLM models.

## Key Insights
- `LlamaContext`/model handle are `!Send`/`!Sync` (R2 §2, confirmed via
  github.com/utilityai/llama-cpp-rs issue #483) — `engine` must expose an API that a single owner
  (Mutex or dedicated thread) can call from blocking contexts; do not attempt to share raw across
  threads. The Laya side of `models` has no equivalent concern — it shells out to a subprocess per
  call, no persistent native handle to share.
- Prompt decode pattern confirmed in R1 §1: build a `LlamaBatch`, `add(token, pos, seq_ids,
  logits: bool)` per token, request logits only at the last position, `ctx.decode(&mut batch)`.
  `engine`'s decode-prompt function should expose "decode full prompt, return logits at last
  position" as its core primitive — the `readout`/`generate` pipeline submodules (Phase 2/3) build
  on it.
- Pin `(repo_id, revision_sha, filename, expected_sha256)` per model (R3 §3) — revision must be a
  commit SHA from Phase 0, not `main`/a tag, since HF refs can move. Applies identically to the
  Laya-en entry.
- Checksum verification is the caller's responsibility (`hf-hub` doesn't do it per R3 §2) — fetch
  each file's sha256 from HF's model API once during Phase 0/1, hardcode into the registry, verify
  post-download with the `sha2` crate, fail closed on mismatch. Same `ensure_downloaded` function
  serves all 4 registry entries.
- **Quant defaults** (plan.md § Validation Summary + Decisions Locked #9): Qwen3-0.6B entry uses
  **Q8_0** (confirmed from the original site); MiniCPM-2B and Qwen-4B entries default to **Q4_K_M**
  (CPU-speed-first); Laya-en defaults to **UD_Q4_K_M**. Registry fields stay user-configurable;
  these are just the shipped defaults per model.
- v1 is CPU-only (plan.md Decisions Locked #7): `crates/engine/Cargo.toml` must not enable
  `cuda`/`vulkan`/`rocm`/`opencl` features for `llama-cpp-2`/`llama-cpp-sys-2`. `n_threads` and
  batch size must be configurable on `Engine`/`LlamaContextParams`, not hardcoded — default
  `n_threads` tuned near the available core count (e.g. `std::thread::available_parallelism()`
  minus a small OS headroom, overridable by caller), not a conservative low default; exact tuned
  value is confirmed empirically on the real Linux target hardware in Phase 7, not guessed here.
  All 4 models (0.6B/2B/4B/Laya-en) are loaded through their respective runtime's load path — no
  special-casing any as lower priority; RAM/disk are not constraints on the target hardware (62GB
  RAM / 276GB disk free).
- Crate dependency direction (plan.md § Architecture): `timing` has zero intra-workspace deps;
  `models` has zero intra-workspace deps (it's the crate everything else depends ON, including its
  own Laya subprocess logic — Laya's download path already lives in `models` regardless of where
  the scoring wrapper sits, so folding the wrapper in too avoids a near-empty sibling crate that
  would only exist to re-expose `models`'s own download function); `engine` depends on BOTH
  `models` (path) and `timing` (path). This phase implements `timing`/`models`/`engine` together
  since they're tightly sequenced and all gate Phase 2/3 identically.

## Requirements
### Functional
- `crates/timing`: `Timings` struct — plain data container, zero deps on `models`/`engine`. Fields
  cover both the LLM pipelines (model_load, warmup, tokenize, constrained_readout, generation) and
  Laya (laya_model_load, laya_inference) — Laya has no separate "warmup"/"tokenize" phase distinct
  from its single inference call, so it gets exactly 2 fields, both `Option<Duration>` (populated
  only when Laya actually runs — `--skip-laya` leaves them `None`).
- `crates/models`: registry of 4 entries (Qwen3-0.6B/Q8_0, MiniCPM "2B"/Q4_K_M, Qwen "4B"/Q4_K_M,
  Laya-en/UD_Q4_K_M) with repo id, pinned revision SHA, filename, expected sha256, license — values
  sourced from Phase 0's spike notes, not re-guessed. `mys/laya-multilingual-GGUF` is documented as
  a deferred future entry in a code comment, not implemented (YAGNI).
- `crates/models`: download function — given a registry entry, use `hf-hub` (confirmed API from
  Phase 0) to fetch the GGUF file to local cache if not already present; verify sha256 after
  download; error (not panic) on checksum mismatch or network failure. Same function for all 4
  entries.
- `crates/models`: Laya scoring — a `LayaRunner` type that resolves+downloads the Laya-en
  `ModelSpec` (via this same crate's `ensure_downloaded`), verifies the `ggmlc-run` binary is
  discoverable (on PATH or a configured path), and exposes a `score(prompt, options) ->
  Result<Vec<(String, f32)>, LayaError>` primitive that shells out to `ggmlc-run` with the syntax
  Phase 0 confirmed, parsing its output into label/score pairs. Lives in its own submodule
  (`crates/models/src/laya.rs`) within this crate, not a separate crate.
- `crates/engine`: `Engine` type — load a GGUF path into `LlamaModel` + `LlamaContext` via
  `llama-cpp-2`; expose a "decode prompt, return logits at last position" primitive, a "decode one
  more token, return logits" primitive, a **greedy-sample-next-token** primitive, and a
  **reset/clear KV-cache** primitive (all consumed by Phase 2/3's `pipeline` crate).
- `crates/engine`: Warmup — run a fixed sanity prompt ("Paris is the capital of...") once after
  load, per original methodology (`docs/research/openjev-rust-research.md` §2) — timed separately
  as its own phase.

### Non-functional
- No hardcoded absolute paths; cache dir resolved via `hf-hub`'s confirmed default (or explicit
  override) from Phase 0. `ggmlc-run`'s binary location is resolved via `PATH` by default,
  overridable via an explicit config field (mirrors `EngineConfig`'s override pattern).
- `Engine` type is `!Send`/`!Sync`-aware: don't wrap it in a bare `Arc` and hand it across threads
  in this crate — that's Phase 4/5's job (Mutex pattern in `apps/server`), this crate just needs to
  compile and work single-threaded correctly. `LayaRunner` has no such constraint (stateless
  subprocess-per-call), so it can be shared via a plain `Arc` without a `Mutex` if ever needed —
  not required for v1's usage pattern regardless.
- `EngineConfig` (or equivalent params struct) carries `n_threads: Option<usize>` and
  `batch_size: Option<usize>`, both defaulting sensibly (see Key Insights) but always
  caller-overridable end-to-end (CLI flag → HTTP request field → this struct) so Phase 7 can sweep
  both without code changes. `laya` has no equivalent thread/batch knob (single subprocess call, no
  persistent context) — nothing to expose there.
- Every fallible step in `crates/models/src/laya.rs` (binary not found, spawn failure, non-zero
  exit, unparseable output) returns a structured `Err`, never panics — `ggmlc-run`'s exact failure
  modes are unknown until Phase 0 verifies them, so error handling must be defensive, not
  optimistic.

## Architecture
```
crates/timing/src/lib.rs
  #[derive(Serialize)] pub struct Timings { pub model_load: Duration, pub warmup: Duration,
    pub tokenize: Duration, pub constrained_readout: Duration, pub generation: Duration,
    pub laya_model_load: Option<Duration>, pub laya_inference: Option<Duration> }
  -- plain data struct, zero deps — apps/cli and apps/server assemble/print it, but the `pipeline`
     crate needs the TYPE to exist to accept `&mut Timings`, hence its own lowest-level crate.

crates/models/src/lib.rs
  -- registry: struct ModelSpec { name, repo_id, revision, filename, quant, sha256, license }
  -- fn all() -> [ModelSpec; 4]   // Qwen3-0.6B, MiniCPM-2B, Qwen-4B, Laya-en
  -- // mys/laya-multilingual-GGUF: documented as a deferred future entry (code comment), not implemented
  -- pub mod download;
  -- pub mod laya;
crates/models/src/download.rs
  -- fn ensure_downloaded(spec: &ModelSpec) -> Result<PathBuf, ModelError>
  -- sha256 verification via `sha2` crate — runtime-agnostic, serves engine AND laya::LayaRunner
crates/models/src/laya.rs
  -- struct LayaConfig { ggmlc_run_path: Option<PathBuf> }   // default: resolve "ggmlc-run" via PATH
  -- struct LayaRunner { model_path: PathBuf, config: LayaConfig }
  -- fn load_from_spec(spec: &ModelSpec, config: LayaConfig) -> Result<LayaRunner, LayaError>
  --   // download::ensure_downloaded(spec) + verify ggmlc-run binary is invocable (e.g. `ggmlc-run --version`/--help)
  -- fn score(&self, prompt: &str, options: &[String]) -> Result<Vec<(String, f32)>, LayaError>
  --   // std::process::Command::new(ggmlc_run_path).arg(<Phase-0-confirmed subcommand/flags>).arg(&self.model_path)...
  --   // .output(); parse stdout per the confirmed format; Err on non-zero exit or unparseable output
crates/models/src/error.rs
  -- ModelError enum (thiserror): download/checksum failures
  -- LayaError enum (thiserror): BinaryNotFound, SpawnFailed, NonZeroExit { code, stderr }, ParseFailed

crates/engine/src/lib.rs
  -- struct EngineConfig { n_threads: Option<usize>, batch_size: Option<usize> }
  -- struct Engine { backend: LlamaBackend, model: LlamaModel, ctx: LlamaContext }
  -- fn load(model_path: &Path, config: EngineConfig) -> Result<Engine, EngineError>
  --   // n_threads default: available_parallelism() with small OS headroom; batch_size default: llama-cpp-2's own default (512 per R1 examples) unless overridden
  -- fn load_from_spec(spec: &models::ModelSpec, config: EngineConfig) -> Result<Engine, EngineError>
  --   // convenience: calls models::ensure_downloaded(spec) then load() — this is WHY engine depends on models
  -- fn decode_prompt(&mut self, tokens: &[LlamaToken]) -> Result<&[f32], EngineError>  // logits at last pos
  -- fn decode_next(&mut self, token: LlamaToken, pos: i32) -> Result<&[f32], EngineError>
  -- fn sample_greedy(&mut self, pos: i32) -> Result<LlamaToken, EngineError>  // wraps LlamaSampler::chain_simple([dist, greedy()]).sample(&ctx, pos) per R1 §2 — used by Phase 3's generation loop
  -- fn reset_context(&mut self) -> Result<(), EngineError>  // KV-cache clear via Phase 0's confirmed API; called between independent pipeline runs so readout/generation never share stale KV state
  -- fn warmup(&mut self) -> Result<(), EngineError>
  -- fn tokenize(&self, text: &str) -> Result<Vec<LlamaToken>, EngineError>
crates/engine/src/error.rs
  -- EngineError enum (thiserror), wraps llama-cpp-2's DecodeError/LlamaModelLoadError/StringToTokenError
```
`sample_greedy` and `reset_context` live in `engine` (not `pipeline`) because `LlamaContext` is a
private field of `Engine` — no other crate can reach the raw context to sample or reset it
directly. This closes two gaps: Phase 3's generation loop needs a sampling primitive it doesn't
otherwise have access to, and both pipelines need a clean KV-cache state before each independent
measured run (the original methodology times each phase in isolation; stale KV cache from a prior
run would silently skew a later one). `load_from_spec` is why `crates/engine/Cargo.toml` depends
on `crates/models` (path dep) — it's a real call, not just a type reference. `models::laya`'s
`LayaRunner` is intentionally a thin, stateless-per-call wrapper (no persistent native handle,
unlike `Engine`) — `score()` is safe to call repeatedly or even concurrently since each call is an
independent subprocess spawn; it lives inside `models` (not a separate crate) since it depends on
nothing this crate doesn't already have (`download`'s own `ensure_downloaded`), per the direct
user simplification request.

## Related Code Files
- CREATE `crates/timing/src/lib.rs` — `Timings` struct, `Serialize` derive (fills in Phase 0's
  empty stub).
- CREATE `crates/models/src/lib.rs` — `ModelSpec` struct + static table of 4 entries (Q8_0 for
  0.6B, Q4_K_M for 2B/4B, UD_Q4_K_M for Laya-en); declares `pub mod download;` + `pub mod laya;`.
- CREATE `crates/models/src/download.rs` — hf-hub download + sha256 verify.
- CREATE `crates/models/src/laya.rs` — `LayaRunner`/`LayaConfig`/`load_from_spec`/`score` (the
  `ggmlc-run` shell-out wrapper).
- CREATE `crates/models/src/error.rs` — `ModelError` + `LayaError` enums (`thiserror`), covering
  download/checksum failures and Laya-specific binary-not-found/spawn/exit/parse failures.
- CREATE `crates/engine/src/lib.rs` — `Engine`/`EngineConfig` load/`load_from_spec`/tokenize/
  decode/`sample_greedy`/`reset_context`/warmup.
- CREATE `crates/engine/src/error.rs` — `EngineError` enum (`thiserror`) wrapping `llama-cpp-2`'s
  `DecodeError`/`LlamaModelLoadError`/`StringToTokenError`.
- MODIFY `crates/models/Cargo.toml` — add `hf-hub` (pinned version from Phase 0), `sha2`,
  `thiserror`, `serde` (no `llama-cpp-2` dep — this crate never touches it, even for Laya).
- MODIFY `crates/engine/Cargo.toml` — add `llama-cpp-2` (pinned version, NO `cuda`/`vulkan`/
  `rocm`/`opencl` feature enabled — CPU-only baseline, plan.md Decisions Locked #7), `thiserror`,
  confirm `models` and `timing` path deps are present (scaffolded by Phase 0, verify not missing).

## Implementation Steps
1. Implement `Timings` struct in `crates/timing/src/lib.rs` — plain fields (5 required + 2
   `Option` Laya fields), `#[derive(Serialize)]`, no behavior beyond being a data container
   (population happens in Phase 2/3/4/5).
2. Write `ModelSpec` struct + populate the 4-entry table in `crates/models/src/lib.rs` using Phase
   0's spike notes (exact repo ids/revisions/filenames/sha256/license, Q8_0 for 0.6B, Q4_K_M for
   2B/4B, UD_Q4_K_M for Laya-en) — do not invent values here. Add a code comment documenting
   `mys/laya-multilingual-GGUF` as a deliberately-deferred future entry. Declare `pub mod download;`
   and `pub mod laya;`.
3. Implement `ensure_downloaded` in `crates/models/src/download.rs` using the confirmed `hf-hub`
   API (builder or `Api`/`ApiBuilder`, per Phase 0 finding) — download to cache, compute sha256 of
   the downloaded file via `sha2`, compare to `ModelSpec.sha256`, return `Err` on mismatch (do not
   silently proceed). This one function serves all 4 registry entries.
4. Implement `LayaRunner::load_from_spec` in `crates/models/src/laya.rs`: call
   `download::ensure_downloaded(&laya_en_spec)`, then verify the `ggmlc-run` binary is invocable
   (e.g. run it with a harmless flag like `--version`/`--help` and check exit status) — return
   `Err(LayaError::BinaryNotFound)` early if not, rather than failing later on the real scoring
   call.
5. Implement `LayaRunner::score(prompt, options)`: build the exact `std::process::Command`
   invocation using Phase 0's confirmed `ggmlc-run` subcommand/flags/input format, pass the model
   path + prompt/options as argv arguments (never via a shell string — no injection surface),
   capture stdout/stderr, parse the confirmed output format into `Vec<(String, f32)>`; non-zero
   exit or unparseable output returns a structured `Err`, never a panic.
6. Wire `ModelError`/`LayaError` enums (`thiserror`) in `crates/models/src/error.rs` covering:
   download failure, checksum mismatch, binary-not-found, spawn failure, non-zero exit, parse
   failure.
7. Implement `Engine::load` in `crates/engine/src/lib.rs` per R1 §1's confirmed pattern:
   `LlamaBackend::init()`, `LlamaModel::load_from_file`, `model.new_context(&backend, ctx_params)`.
8. Implement `Engine::load_from_spec(spec: &models::ModelSpec, config: EngineConfig)` — calls
   `models::ensure_downloaded(spec)` then `Self::load(path, config)`; this is the primary
   entrypoint both `apps/cli` and `apps/server` use for the 3 LLM models.
9. Implement `Engine::tokenize` wrapping `model.str_to_token(text, AddBos::Always)`.
10. Implement `Engine::decode_prompt`: build `LlamaBatch`, add all prompt tokens with `logits:
    true` only on the last one, `ctx.decode(&mut batch)`, return logits slice via the confirmed
    `get_logits`/`get_logits_ith` signature from Phase 0.
11. Implement `Engine::decode_next` for single-token incremental decode (used by Phase 3's
    generation loop) — same batch pattern, one token, position = running counter.
12. Implement `Engine::sample_greedy` per R1 §2's confirmed pattern (`LlamaSampler::chain_simple`
    with `dist`+`greedy()`, `.sample(&ctx, pos)`) — this is the primitive Phase 3's generation loop
    calls each iteration; owning it here (not in `pipeline`) avoids exposing the private
    `LlamaContext` field outside `engine`.
13. Implement `Engine::reset_context` using Phase 0's confirmed KV-cache reset call (e.g.
    `kv_cache_clear`/`kv_cache_seq_rm`) — to be called between independent pipeline runs (Phase
    4/5's orchestration calls this between `run_readout` and `run_generate`) so neither pipeline's
    timed measurement is contaminated by the other's leftover KV-cache state.
14. Implement `Engine::warmup` running the fixed sanity prompt through `decode_prompt` once,
    discarding output — just proves the loaded model responds.
15. Wire `EngineError` enum (`thiserror`) covering: model load failure, tokenize failure, decode
    failure, sample failure, reset failure — every fallible step returns `Result`, nothing unwraps
    in library code.

## Todo List
- [ ] `Timings` struct in `crates/timing/src/lib.rs`, `Serialize` derive, incl. 2 Laya `Option`
      fields
- [ ] `ModelSpec` table populated from Phase 0 data (4 entries, correct quant per model,
      laya-multilingual documented-but-deferred)
- [ ] `ensure_downloaded` implemented + checksum-verified, serves all 4 entries
- [ ] `LayaRunner::load_from_spec`/`score` implemented in `crates/models/src/laya.rs`, binary-
      availability check + argv-based `std::process::Command` invocation, structured errors (no
      panics)
- [ ] `EngineConfig` with `n_threads`/`batch_size`, sensible defaults, fully overridable
- [ ] `Engine::load`/`load_from_spec`/`tokenize`/`decode_prompt`/`decode_next`/`sample_greedy`/
      `reset_context`/`warmup` implemented, CPU-only (no GPU feature flags in
      `crates/engine/Cargo.toml`)
- [ ] `ModelError`/`LayaError`/`EngineError` cover every fallible step, no `unwrap()`/`expect()`
      in non-test code
- [ ] Manual smoke test: `engine` loads Qwen3-0.6B, tokenizes, decodes, gets non-empty logits
      slice; `models::laya` scores a trivial 2-option question via `ggmlc-run`, gets non-empty
      scores
- [ ] `cargo build --workspace` still succeeds after filling in `timing`/`models`/`engine` (path
      deps resolve)

## Success Criteria
- `cargo build --workspace` succeeds with `timing`/`models`/`engine` filled in, CPU-only (no GPU
  feature flags).
- Manual run (or a `#[cfg(feature = "integration")]` test, see Phase 6) loads Qwen3-0.6B end-to-end:
  download (or cache-hit) → checksum pass → load → tokenize → decode_prompt → non-empty `&[f32]`
  logits returned, length equal to model vocab size.
- Manual run (or integration test) scores Laya-en end-to-end via `models::laya`: download →
  checksum pass → binary check → `ggmlc-run` invocation → non-empty label/score pairs returned.
- Checksum mismatch path returns `Err`, does not load a corrupted/wrong file into either runtime.
- `EngineConfig.n_threads`/`batch_size` overrides are observably applied (e.g. passed through to
  `LlamaContextParams` correctly) — exact optimal values are NOT determined here, only that the
  knob works end-to-end; Phase 7 does the real tuning on the target Linux hardware.

## Test Strategy & Quality Gate
Lane: normal.
- Spec: Goal = reliably load any of the 3 registered LLM GGUFs (return raw logits for a given
  prompt) and reliably score Laya-en via `ggmlc-run` (return label/score pairs). AC: (1) Given a
  valid `ModelSpec` and empty cache, When `ensure_downloaded` runs, Then file exists locally and
  sha256 matches. (2) Given a corrupted cache file (wrong bytes), When `ensure_downloaded` runs,
  Then it returns `Err` (does not silently accept). (3) Given a loaded `Engine` and a prompt, When
  `decode_prompt` runs, Then it returns a logits slice of length == vocab size. (4) Given a loaded
  `LayaRunner` and a prompt+options, When `score` runs, Then it returns non-empty label/score pairs
  or a structured `Err` (never panics), and a missing `ggmlc-run` binary returns
  `Err(BinaryNotFound)` specifically, not a generic spawn error. I/O contract: `ModelSpec ->
  Result<PathBuf, ModelError>`; `(&mut Engine, &str) -> Result<&[f32], EngineError>`; `(&LayaRunner,
  &str, &[String]) -> Result<Vec<(String, f32)>, LayaError>`. Out-of-scope: constrained softmax
  (Phase 2), generation loop control (Phase 3), the `pipeline::laya` orchestration wrapper (Phase
  2).
- Pyramid ~70/20/10 adjusted for this module (heavier integration weight since real value is in
  native-library/subprocess interaction): unit tests for `ModelSpec` table shape + checksum-compare
  logic + `LayaError` variant mapping (mockable, no real download/subprocess) target ~50%;
  integration tests behind `feature = "integration"` that actually download/load the smallest LLM
  (Qwen3-0.6B) and Laya-en, and assert real decode/score output ~40%; the named e2e scenario
  (~10%) IS that same feature-gated integration test's full round trip for EACH runtime — "given an
  empty cache and a valid ModelSpec, ensure_downloaded → checksum-verify → Engine::load_from_spec →
  tokenize → decode_prompt returns a vocab-sized logits slice" (engine) and "given an empty cache
  and the Laya-en ModelSpec, ensure_downloaded → checksum-verify → LayaRunner::load_from_spec →
  score returns non-empty label/score pairs" (models::laya) — these crates' primary user journeys
  end-to-end. Phase 4's CLI later exercises both paths again through its own e2e scenario; that is
  additional coverage, not a substitute for naming these crates' own.
- Coverage target: ≥90% line / ≥75% branch on `crates/models/**` (incl. `laya.rs`),
  `crates/engine/**`, and `crates/timing/**` (trivial struct, but still counted) new code, measured
  on the non-integration-gated unit tests (checksum comparator, `ModelSpec` table construction,
  `LayaError` mapping) — integration-gated tests count toward pyramid but coverage tool run is the
  fast/default job per R4 §1/§4.
- Evidence commands: `cargo test -p timing -p models -p engine` (default, no integration feature —
  fast unit tests only), `cargo test -p models -p engine --features integration --
  --test-threads=1` (real model download+load+score, run manually/CI-scheduled per R4 §4), `cargo
  llvm-cov` or `cargo tarpaulin` for diff coverage (pick one at implementation time, whichever is
  already idiomatic for the toolchain used).

## Risk Assessment
- Risk: `hf-hub` API from Phase 0 turns out incomplete for revision-pinned downloads (e.g. no
  direct "download this exact file at this exact revision" call) → mitigation: fall back to
  constructing the raw HF resolve URL (`https://huggingface.co/{repo}/resolve/{revision}/{file}`)
  and downloading via `reqwest` directly, still checksum-verified. Document if this fallback is
  used.
- Risk: 4B model too large/slow for frequent local dev iteration → not a blocker (v1 requirement is
  fixed), but note in Phase 6 that integration tests default to the 0.6B model for CI speed,
  4B/2B/Laya reserved for manual/scheduled runs (R4 §4).
- Risk: `ggmlc-run`'s actual output format (from Phase 0) is harder to parse reliably than expected
  (e.g. mixes log lines with result data on stdout) → if this surfaces, prefer the most
  conservative parse (last well-formed line/JSON blob) and document the assumption; flag to user if
  the format is genuinely ambiguous rather than guessing silently.
- Risk: workspace path-dependency graph (`engine`→models+timing) misconfigured in
  `crates/engine/Cargo.toml` (e.g. missing `timing` dep) → caught immediately by `cargo build
  --workspace` failing; Phase 0's skeleton already declares the graph, this phase should not need
  to change it beyond adding external deps.

## Security Considerations
- Checksum verification is the integrity control — fail closed on mismatch, never load unverified
  weights (applies to all 4 registry entries).
- No credentials needed for public HF repos; if any repo requires a token, document explicitly (not
  expected per Phase 0 findings) and never hardcode a token in source (`.env.local` pattern from
  Phase 0 covers this if ever needed).
- `crates/models/src/laya.rs`'s subprocess invocation passes arguments as an argv array to
  `std::process::Command`, never through a shell string — no shell-injection surface even though
  `prompt`/`options` are user-supplied text.

## Next Steps
- Phase 2 (the `pipeline` crate's `readout`/`laya` submodules) depends on both `Engine::
  decode_prompt`/`decode_next`/`tokenize`/`sample_greedy`/`reset_context` AND
  `models::laya::LayaRunner::score` being correct and stable. Phase 3 (`pipeline::generate`)
  depends only on the `engine` side. Do not change any of these signatures without updating the
  downstream phases that consume them.
