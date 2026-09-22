---
loop: 1
status: pending
preservation_constraints: []
---

## Objective
Phase 0 spike + provisioned toolchain (local + remote), workspace skeleton builds.

Resolve the 8 unresolved unknowns from `plans/20260922-2146-openjev-rust-implementation/
plan.md` § Unresolved (model repo IDs, `hf-hub` API, `llama-cpp-2` logits API, `ggmlc-run`
CLI/build), scaffold the 6-crate Cargo workspace, and get both the Windows dev machine and
the remote Linux server (`root@103.146.166.46`) to a state where `cargo build --workspace`
succeeds. This is the gate every later loop depends on — nothing pipeline/API-shaped should
be built on unverified assumptions.

## Tasks
1. **Resolve Unresolved #1-#5** (model repo IDs, `hf-hub` API, `llama-cpp-2` API, licensing):
   read `openjev`/SemIf's own GitHub source for exact HF repo IDs; `cargo add hf-hub
   --dry-run` + local docs for the real API; read `llama-cpp-2` source
   (`llama_context.rs`/`model.rs`) for `get_logits`/KV-cache-reset/chat-template signatures.
   Record findings + citations directly in this loop's evidence.
2. **Resolve Unresolved #6-#8** (`ggmlc-run`): read `ggmlc-run --help` (build it first),
   confirm typed-decision CLI syntax, note CPU latency order-of-magnitude.
3. **Provision `root@103.146.166.46`**: install `rustc`/`cargo` (rustup), `cmake` (≥3.18),
   `build-essential` (gcc/g++), `git`, `pkg-config`. Confirm versions.
4. **Scaffold workspace** (both machines, same source tree, synced via `rsync`/`scp` to the
   server): `Cargo.toml` (workspace root, 6 members), `apps/cli`, `apps/server`,
   `crates/{engine,models,pipeline,timing}` — stub `lib.rs`/`main.rs` per crate (may be
   near-empty at this point, just enough to compile), `Cargo.toml` per member with the
   path-dependency graph from `plan.md` § Architecture. Add `.gitignore`,
   `.env.local.example`, `rust-toolchain.toml`, `Makefile` skeleton, `README.md` skeleton.
5. **Verify build both places**: `cargo build --workspace` exits 0 on Windows dev machine
   AND on the remote server (via SSH).

## Preservation
None — first loop, `A_0` = empty workspace.

## Validation Requirements
- Given the 6-crate workspace skeleton, When `cargo build --workspace` runs on the Windows
  dev machine, Then it exits 0.
- Given the same skeleton rsynced to `root@103.146.166.46`, When `cargo build --workspace`
  runs there over SSH, Then it exits 0.
- Given `ggmlc-run` cloned+built on the remote server, When invoked with `--help`, Then it
  prints usage without crashing, and the typed-decision subcommand/flags are identified and
  recorded.
- Given all 8 Unresolved items, When this loop ends, Then each has either a primary-source
  citation or an explicit documented fallback (never an unstated guess).

## Out-of-scope
- Actual `engine`/`models`/`pipeline` logic (real GGUF load, inference) — Loop 2+.
- HTTP server, CLI arg parsing beyond stubs — Loop 2+.
- Model downloads (Qwen3/MiniCPM/Laya GGUF files) — happens when a loop actually needs to
  run inference, not in this skeleton-only loop, unless needed to test `ggmlc-run --help`
  wiring (info-only invocation, no full model run required here).
- Performance tuning — far later loop.
