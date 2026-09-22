---
phase: 0
name: Spike Verification
status: pending
depends_on: []
---

## Context Links
- `../plan.md` § Unresolved / Must Verify in Phase 0, § Architecture (workspace layout), §
  Decisions Locked #9 (Laya)
- `research/researcher-01-llama-cpp-2-engine.md` §1, §4, Unresolved 1-3
- `research/researcher-03-model-acquisition.md` §1, §2, Unresolved 1-4
- Original source to check: https://github.com/workszop/openjev (model config, worker.js)
- Laya: https://huggingface.co/mys/laya-GGUF (model card, usage snippet), github.com/monatis/ggmlc
  (`ggmlc-run` CLI source)

## Overview
- Priority: P0, blocking. No implementation code beyond a build-only workspace skeleton.
- Status: pending.
- Close 8 unresolved facts from research with primary-source evidence before any pipeline/engine
  code is written. This is a spike — output is a decisions record + a buildable empty workspace
  skeleton (6 crates: 4 lib + 2 bin, plus root project files), not features.

## Key Insights
- R1/R3 both explicitly flagged conflicting or unverified API/naming facts — guessing here
  propagates wrong assumptions through Phases 1-3. Iron rule: no code against unverified APIs. Same
  rule applies to `ggmlc-run`'s scoring CLI syntax (plan.md Unresolved #6) — do not guess the
  input/output format.
- `llama-cpp-2` native build needs CMake + MSVC C++17 + libclang (bindgen) on Windows (R1 §3) —
  must confirm toolchain present BEFORE Phase 1 writes real engine code, or every later phase
  blocks on environment setup instead of logic. `ggmlc` (Laya's runtime) uses the same toolchain
  family (CMake 3.18+, C++17) — not a new risk *category*, but must still be independently built
  and verified (plan.md Unresolved #7), not assumed to work just because llama-cpp-2 did.
- Two build targets exist: the **Windows dev machine** (this spike, Phases 0-6 day-to-day
  build/test) and the **Linux target server** used for real e2e + performance tuning (Phase 7 —
  Intel Xeon Gold 5320, 32 vCPU, 62GB RAM, CPU-only). Linux builds of `llama-cpp-2`/
  `llama-cpp-sys-2`/`ggmlc` are typically simpler (gcc/clang + cmake via `apt`, no MSVC/vcpkg) — not
  separately deep-researched here; only resolved live in Phase 7 if a real build issue surfaces
  there. This phase's build-skeleton verification is Windows-only; do not block Phase 0 on
  provisioning the Linux server.
- v1 is CPU-only (plan.md Decisions Locked #7) — the skeleton build must NOT enable any GPU
  feature (`cuda`/`vulkan`/`rocm`/`opencl`) on `llama-cpp-2`/`llama-cpp-sys-2`; default (no GPU
  feature flags) is the CPU-only baseline per R1 §3. Laya's own published numbers are GPU-only
  (RTX 4050) — CPU latency is genuinely unknown (plan.md Unresolved #8), get an order-of-magnitude
  sanity number here, full tuning is Phase 7's job.
- **Workspace layout is final** (plan.md § Architecture): a Cargo workspace with 4 library crates
  (`timing`, `models`, `engine`, `pipeline`) and 2 binary crates (`apps/cli` → `openjev-cli`,
  `apps/server` → `openjev-server`), plus root-level project files (`.gitignore`,
  `.env.local.example`, `rust-toolchain.toml`, `Makefile`, `README.md`). **No separate `laya`
  crate** (merged per direct user follow-up) — the `ggmlc-run` shell-out wrapper for Laya lives
  inside `models` (`crates/models/src/laya.rs`), alongside the registry/download logic it shares.
  This phase scaffolds all 6 crate manifests + the root files; Phase 1+ fill in real logic.
- **Quant defaults**: per the `/vk-plan-validate` interview (plan.md § Validation Summary) and the
  later Laya addition, MiniCPM-2B/Qwen-4B/Laya-en all default to speed-first quants — **Q4_K_M**
  for the two LLMs, **UD_Q4_K_M** for Laya (its own quant-ladder naming); Qwen3-0.6B stays **Q8_0**
  (confirmed from the original site). Record all 4 alongside resolved repo IDs/revisions.

## Requirements
### Functional
- Resolve exact HF repo id (org/name) + a specific revision (commit SHA, not `main`) for each of:
  Qwen3-0.6B (Q8_0), the "2B" MiniCPM model (Q4_K_M), the "4B" Qwen model (Q4_K_M), Laya-en
  (`mys/laya-GGUF`, UD_Q4_K_M).
- Resolve `hf-hub` crate: exact published version, and whether its API is the
  `HFClient`/`HFClientSync` builder or `Api`/`ApiBuilder`/`Repo::with_revision`.
- Resolve `llama-cpp-2` crate: confirm `LlamaContext::get_logits`/`get_logits_ith` (or nearest
  equivalent) signature, KV-cache reset call, and presence/absence of a chat-template wrapper.
- Resolve `ggmlc-run`'s exact CLI syntax for typed-decision (choice/score/noun-style) scoring: build
  `ggmlc-run` (github.com/monatis/ggmlc) on the Windows dev machine, read `ggmlc-run --help` and/or
  the laya model card's usage snippet directly — record the literal subcommand/flags/input format,
  do not infer from the `info`/`chat`/`run --image` subcommands already known to exist.
- Run one quick CPU sanity timing for Laya (order-of-magnitude only — no published CPU numbers
  exist, only GPU RTX 4050 figures) once the CLI syntax is resolved.
- Scaffold the full Cargo workspace: root `Cargo.toml` (virtual manifest, `resolver = "2"`,
  `members = ["apps/cli", "apps/server", "crates/engine", "crates/models", "crates/pipeline",
  "crates/timing"]`), all 6 crate `Cargo.toml` manifests with correct path dependencies (`engine` →
  `models`+`timing`; `pipeline` → `engine`+`models`+`timing`; both `apps/*` →
  `engine`+`models`+`pipeline`+`timing`), and minimal `src/lib.rs`/`src/main.rs` stubs so `cargo
  build --workspace` succeeds end-to-end, proving the Windows CMake/MSVC/bindgen toolchain works
  (for both `llama-cpp-2` and `ggmlc`) before real logic is written.
- Scaffold root project files: `.gitignore`, `.env.local.example`, `rust-toolchain.toml`,
  `Makefile` (empty/stub targets acceptable at this phase — real target bodies land in Phase
  1/4/5/6/7 as those pieces exist), `README.md` (brief project description + links).

### Non-functional
- Every resolved fact must cite its source (file/line in openjev's GitHub repo, `cargo doc` output,
  `ggmlc-run --help` output, or HF model card URL) — record in a `DECISIONS.md` at plan root or
  inline in this phase file's Todo List comments, so Phase 1+ can cite it instead of re-researching.

## Architecture
No new product architecture here — this phase only validates the assumptions Phase 1-6's
architecture (see `plan.md` § Architecture) depends on: `llama-cpp-2` as the LLM engine, `ggmlc-run`
(shelled out, wrapped inside `models`) as the Laya engine, `hf-hub` as the shared downloader, and
the workspace layout itself (6 crates, correct path-dependency graph).

## Related Code Files
- CREATE `Cargo.toml` (workspace root) — virtual manifest: `[workspace]`, `resolver = "2"`,
  `members = ["apps/cli", "apps/server", "crates/engine", "crates/models", "crates/pipeline",
  "crates/timing"]`, no `[package]`.
- CREATE `crates/timing/Cargo.toml` + `crates/timing/src/lib.rs` — empty lib stub (no deps).
- CREATE `crates/models/Cargo.toml` + `crates/models/src/lib.rs` — empty lib stub (no
  intra-workspace deps; `hf-hub` dep pinned per this phase's spike finding — this crate will also
  house the `ggmlc-run` shell-out wrapper for Laya, filled in by Phase 1, no separate crate).
- CREATE `crates/engine/Cargo.toml` (deps: `models` path, `timing` path, `llama-cpp-2` pinned, no
  GPU features) + `crates/engine/src/lib.rs` — empty lib stub.
- CREATE `crates/pipeline/Cargo.toml` (deps: `engine` path, `models` path, `timing` path) +
  `crates/pipeline/src/lib.rs` — empty lib stub.
- CREATE `apps/cli/Cargo.toml` (package `cli`, `[[bin]] name = "openjev-cli"`, deps: `engine`,
  `models`, `pipeline`, `timing` all path, plus `clap`/`serde`/`serde_json`) +
  `apps/cli/src/main.rs` — trivial `fn main() {}` or `println!` stub.
- CREATE `apps/server/Cargo.toml` (package `server`, `[[bin]] name = "openjev-server"`, deps:
  `engine`, `models`, `pipeline`, `timing` all path, plus `clap`/`axum`/`tokio`/`serde_json`) +
  `apps/server/src/main.rs` — trivial `fn main() {}` or `println!` stub.
- CREATE `.gitignore` — `/target`, `.env.local`, `*.gguf`.
- CREATE `.env.local.example` — `HF_TOKEN=` template (documented as optional: v1's models are
  public/Apache-2.0 per this phase's licensing check, no token strictly required; useful only for
  higher HF API rate limits).
- CREATE `rust-toolchain.toml` — `[toolchain]` `channel = "<exact stable version resolved during
  this phase's spike>"`, pinning the version this workspace was verified to build against.
- CREATE `Makefile` — stub targets (bodies filled in as their pieces exist): `build` (`cargo build
  --workspace`), `test` (`cargo test --workspace`), `bench` (`cargo run -p cli --`), `serve`
  (`cargo run -p server --`), `fmt` (`cargo fmt --all`), `clippy` (`cargo clippy --workspace -- -D
  warnings`), `ci` (`fmt` check + `clippy` + `test`), `server-deploy` (placeholder invoking Phase
  7's runbook steps, including Laya's `ggmlc-run` build — filled in when Phase 7 exists).
- CREATE `README.md` — project name/description, `cargo build --workspace` quickstart, how to run
  each binary (`cargo run -p cli -- --help`, `cargo run -p server -- --help`), a note that Laya
  comparison requires a locally-built `ggmlc-run` binary on PATH, links to
  `docs/research/openjev-rust-research.md` and this plan.
- CREATE `plans/20260922-2146-openjev-rust-implementation/spike-notes.md` (or append to this
  phase file's Todo List) — the 8 resolved facts + citations.

## Implementation Steps
1. Fetch `https://github.com/workszop/openjev` source (raw file fetch or `WebFetch`): locate model
   config (likely `worker.js` or a `models.js`/config file per `docs/research/openjev-rust-research.md`
   §2 file structure) and extract the literal HF repo ids/filenames used for all 3 LLM models.
2. If step 1 is inconclusive for MiniCPM or the 4B Qwen (source uses a generic name without repo
   id, or repo is inaccessible), apply the plan.md fallback: `openbmb/MiniCPM5-2B-GGUF` for the 2B
   model, `Qwen/Qwen3-4B-GGUF` for the 4B model — record this as an inferred (not verified) choice,
   final per plan.md's Unresolved #1/#2 (no re-confirmation pause needed).
3. For all 3 LLM repos: open each HF model card, capture exact filename for the chosen quant (Q8_0
   for Qwen3-0.6B, **Q4_K_M** for MiniCPM-2B and Qwen-4B, per plan.md § Validation Summary), the
   current commit revision (via HF API or "Files and versions" tab commit hash), and the license
   field.
4. Open `https://huggingface.co/mys/laya-GGUF`'s model card: capture the UD_Q4_K_M filename, commit
   revision, license field, and any usage-snippet text showing `ggmlc-run` invocation syntax.
5. `cargo add hf-hub --dry-run` in a scratch dir to see the version cargo would resolve; then read
   that exact version's docs (`docs.rs/hf-hub/<version>` or local `cargo doc --open` after a real
   `cargo add hf-hub`) — confirm builder API shape and default cache path/env var.
6. `cargo add llama-cpp-2 --dry-run`, then `cargo doc --open` locally (after adding for real in the
   skeleton project) — read `LlamaContext` for `get_logits`/`get_logits_ith`, KV-cache reset
   methods, and `LlamaModel` for any chat-template method. If `cargo doc` rendering is unusable,
   fall back to reading crate source directly (`~/.cargo/registry/src/.../llama-cpp-2-<ver>/src/`).
7. Clone `github.com/monatis/ggmlc`, build `ggmlc-run` on the Windows dev machine (CMake + C++17,
   same toolchain already confirmed for `llama-cpp-2` in step 6). Run `ggmlc-run --help` (and any
   subcommand-specific `--help`) to find the typed-decision scoring syntax; cross-check against the
   laya model card's own usage snippet from step 4. Record the literal command shape.
8. Download the Laya-en GGUF (UD_Q4_K_M) and run one scoring invocation with the syntax from step
   7 against a trivial 2-option question — record wall-clock time as the CPU sanity number (not a
   tuned result).
9. Scaffold the full workspace per Related Code Files: root `Cargo.toml`, 6 crate manifests with
   correct path-dependency graph, minimal stubs, root project files. Run `cargo build --workspace`
   on Windows with NO GPU feature flags enabled (CPU-only baseline). Confirm CMake, MSVC Build
   Tools, and libclang are installed and discoverable — if any is missing, document the exact
   install step (this becomes a one-time environment note, not a per-developer blocker).
10. Write the 8 resolved facts (with citations) into the spike notes file.

## Todo List
- [ ] Model repo ids + revisions + filenames (Q8_0 for 0.6B, Q4_K_M for 2B/4B, UD_Q4_K_M for
      Laya-en) + license for all 4 models, with citation
- [ ] `hf-hub` version + confirmed API shape + cache path, with citation
- [ ] `llama-cpp-2` logits API signature, KV-cache reset call, chat-template presence/absence,
      with citation
- [ ] `ggmlc-run` builds on Windows; exact typed-decision scoring CLI syntax confirmed via
      `--help`/model card, with citation
- [ ] Laya CPU sanity timing recorded (order-of-magnitude, not tuned)
- [ ] Workspace root `Cargo.toml` + 6 crate manifests + stubs created, path-dependency graph
      correct (`engine`→models+timing, `pipeline`→engine+models+timing, both apps→all 4)
- [ ] `.gitignore`, `.env.local.example`, `rust-toolchain.toml`, `Makefile`, `README.md` created
- [ ] `cargo build --workspace` succeeds on Windows (paste build output)
- [ ] Spike notes file written with all citations

## Success Criteria
- All 8 unresolved items in `plan.md` have a recorded answer + citation (source URL or file path,
  not "inferred" without a fallback justification).
- `cargo build --workspace` exits 0 on the skeleton project (Windows, CPU-only, no GPU feature
  flags), proving native toolchain works across all 6 crates and the path-dependency graph resolves.
- `ggmlc-run` binary is built and runs at least one successful scoring invocation against Laya-en.
- Phase 1 can start without re-deriving any of these facts.

## Test Strategy & Quality Gate
Lane: normal — `workflows/quality-gate.md` §8 exempts ONLY lane `tiny`; this phase gets no partial
exemption, so it satisfies §6's letter directly rather than claiming a spike-only carve-out that
doesn't exist in the spec:
- Spec (§2's 4 fields): Goal = close 8 named unknowns with primary-source citations and prove the
  Windows toolchain produces a working (CPU-only) workspace build across all 6 crates, including a
  functioning `ggmlc-run` binary. Acceptance Criteria: (1) Given the 8 unresolved items in plan.md,
  When Phase 0 completes, Then each has a recorded citation (source URL or file path) — no item
  left as bare "inferred" without a documented fallback. (2) Given the scaffolded workspace (6
  crate manifests, no GPU feature flags), When `cargo build --workspace` runs, Then it exits 0. (3)
  Given a built `ggmlc-run` binary and a downloaded Laya-en GGUF, When the confirmed scoring
  command runs, Then it produces output (proving the resolved CLI syntax actually works, not just
  that `--help` printed something). I/O contract: N/A (no runtime request/response — this phase
  produces a build artifact and a citations document, not a callable API). Out-of-scope: any
  pipeline/engine logic (Phase 1-3), Linux build behavior (Phase 7).
- **Named e2e scenario** (this phase's primary "user journey" IS the build): "Given a freshly
  scaffolded workspace (6 crate manifests with pinned `llama-cpp-2`/`hf-hub` versions, no GPU
  features, correct path-dependency graph) and minimal `main.rs`/`lib.rs` stubs, When `cargo build
  --workspace` is run on the Windows dev machine, Then it exits 0 and produces both `openjev-cli`
  and `openjev-server` binaries." This is the one required e2e scenario for this phase (§4); the
  `ggmlc-run` scoring invocation (AC #3 above) is a second concrete check on the same "does the
  toolchain actually work" theme, not a separate e2e scenario category.
- Coverage target: **N/A, explicitly** — not silently omitted. No application code exists yet to
  instrument; there is nothing for a line/branch coverage tool to measure against. This differs
  from "unstated" (which §4.D treats as a gap) because the reason is stated and load-bearing (no
  code under test), not an oversight.
- Evidence commands: `cargo build --workspace` (paste full terminal output, exit code 0) as the
  e2e scenario's evidence; `ggmlc-run <resolved-subcommand> <laya-gguf-path> ...` output as the
  Laya-viability evidence; the 8 citations (source URL/file path per item) as the spec's Acceptance
  Criteria evidence.

## Risk Assessment
- Risk: openjev's GitHub source is unreachable or ambiguous for MiniCPM/4B naming → mitigated by
  documented fallback repos in plan.md (already pre-agreed, not a new decision).
- Risk: `llama-cpp-2` genuinely has no `get_logits_ith`-equivalent → would force restructuring
  Phase 2's readout design (decode-then-index-full-vocab instead). Flag immediately if found;
  re-open Phase 2 design before writing that phase's code.
- Risk: Windows toolchain (CMake/MSVC/libclang) missing → resolve here, not mid-Phase-1.
- Risk: `ggmlc-run` has no documented typed-decision scoring mode at all (only `info`/`chat`/
  `run --image` confirmed to exist) → if `--help` and the model card both come up empty, this is a
  genuine blocker for the Laya integration specifically (not the rest of the plan) — flag to the
  user immediately rather than guessing a JSON/text protocol; the rest of the plan (3-model LLM
  comparison) is not blocked by this, only Decisions Locked #9's scope is.
- Risk: workspace path-dependency graph misconfigured (e.g. a crate declaring a dep it doesn't
  need, or missing one it does) → caught immediately by `cargo build --workspace` failing; fix
  before Phase 1 starts, not discovered later when a downstream crate can't compile.

## Security Considerations
- None (no network-facing code, no auth, no user data yet). Model download URLs are read-only
  fetches from HF — no credentials involved for public repos. `.env.local.example` documents the
  optional `HF_TOKEN` convenience var without ever committing a real token (`.gitignore` excludes
  `.env.local`). Shelling out to `ggmlc-run` via `std::process::Command` uses a fixed, non-user-
  controlled argument shape (model path + resolved subcommand/flags) — no shell injection surface
  since arguments are passed as an argv array, never through a shell string.

## Next Steps
- Phase 1 (`engine`/`models`/`timing` crates — `models` includes the Laya `ggmlc-run` wrapper)
  consumes: pinned model table (all 4 entries, with quant choices), confirmed `hf-hub` API,
  confirmed `llama-cpp-2` API surface, confirmed `ggmlc-run` CLI syntax, and the scaffolded crate
  manifests this phase created.
