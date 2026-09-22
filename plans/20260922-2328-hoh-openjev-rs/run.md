---
slug: hoh-openjev-rs
spec: plans/20260922-2328-hoh-openjev-rs/spec.md
started: 2026-09-22T23:28
budget: 8
lane: normal
model: sonnet-5
---

## Deployment Target

`ssh root@103.146.166.46` — Ubuntu 24.04.2 LTS, 32 vCPU, 62GB RAM (60GB free), 4GB swap.
Confirmed 2026-09-22 23:29: SSH reachable (key-based, no password prompt), NO toolchain
installed yet (no rustc/cargo/cmake/gcc/g++/git). Provisioning is part of Loop 1.

## Definition of Done (user directive, supersedes default 3-loop budget if unmet)

1. `openjev-server` running on 103.146.166.46, reachable by IP.
2. `curl http://103.146.166.46:<port>/health` → 200.
3. `curl -X POST http://103.146.166.46:<port>/bench ...` → valid `BenchReport` JSON, all 3
   methods (readout/generate/laya) for at least Qwen3-0.6B.
4. All 3 LLM models + Laya benchmarked successfully on the real server.
5. ≥1 real performance-tuning iteration recorded (before/after `Timings`), converging toward
   fastest measured config on the 32-vCPU hardware — loop continues past budget=8 if gaps
   remain, per explicit user instruction ("không dừng lại cho tới khi thành công").
6. No auth on `/bench`/`/health` this round (in-scope, not a gap).

## Pre-warmed state (parallel to Loop 1, done ahead of schedule)

All 4 GGUF models pre-downloaded to `root@103.146.166.46`'s standard HF Hub cache
(`~/.cache/huggingface/hub/`, via `hf download` CLI — same layout the Rust `hf-hub` crate
reads, so Loop 2+ should cache-hit instantly):
- `Qwen/Qwen3-0.6B-GGUF` → `Qwen3-0.6B-Q8_0.gguf` (639MB)
- `openbmb/MiniCPM5-2B-GGUF` → `MiniCPM5-2B-Q4_K_M.gguf` (1.45GB) — **confirms Unresolved #1's
  fallback repo genuinely exists** (strong evidence, not just a documented fallback anymore)
- `Qwen/Qwen3-4B-GGUF` → `Qwen3-4B-Q4_K_M.gguf` (2.33GB)
- `mys/laya-GGUF` → `laya_english_ud_q4_k_m.gguf` (419MB)

Total 4.8GB, server disk 265GB free after.

## Runtime decision: Windows dev-build parity descoped (not a gate)

G1 (Windows `cargo build --workspace` fails — missing libclang for `llama-cpp-sys-2` bindgen)
is logged but **not blocking**. The user's Definition of Done is entirely about the Linux
server (103.146.166.46) — that's the authoritative build/run target for every loop from here
on. Windows is dev-convenience only. Decision (mechanical, Runtime call): from Loop 2 onward,
`Runtime.check(A_t)` runs against the **Linux server** as the primary/required check; a
Windows build attempt is opportunistic/best-effort, never blocking. Will revisit G1 with a
cheap fix (install LLVM, set `LIBCLANG_PATH`) if a loop has spare capacity, not as a gate.

## Reference material for later loops

`plans/20260922-2146-openjev-rust-implementation/research/researcher-05-benchmark-usecases.md`
— 14 real Jev/OpenJev/Laya use-case categories (email routing, jailbreak detection, invoice
categorization, agent tool-routing, etc.), a reusable proxy eval-dataset list (InjecAgent, BEIR
SciFact, SNIPS, Banking77, MetaTool, SkillRetBench, BFCL v3 — from a third-party ~22.5k-call
Jev benchmark), and **10 concrete benchmark scenarios (prompt/options/category/rationale)**
spanning easy→hard/adversarial. **Use these to replace the trivial "capital of France" example**
once a loop builds the default test-scenario set for `apps/cli`/`apps/server` demos and
Phase 7's tuning loop — not yet wired into any loop's Tasks, flagged here so it isn't lost.

## Reference material: Jev API contract (for a future drop-in-compatibility endpoint)

`plans/20260922-2146-openjev-rust-implementation/research/researcher-06-jev-api-contract.md`
— real official Jev contract found (medium-high confidence, AI-summarized fetches not raw
bytes, caveat noted in the report):
- `POST https://api.typesafe.ai/v1/systemone`, `Authorization: Bearer <key>`.
- Request: `{state, model, questions: {name: {type: noul|choice|score, instructions,
  criteria}}}`.
- Response: `{model, answers: {name: {type, <value-field>, probabilities?, confidence?,
  legend?}}, usage: {input_tokens, output_tokens}}`.

Plan: keep `apps/server`'s existing `/bench` endpoint as-is (rich 3-method comparison +
per-phase timing — that's this project's differentiator, doesn't fit Jev's single-answer
shape). ADD a separate Jev-shaped compatibility endpoint (e.g. `POST /v1/systemone`) in a
LATER loop (not Loop 3 — don't block curl-reachability on this), accepting the same
request/response field names so a Jev user can point their existing client at our server with
minimal changes. Route it internally to the constrained-readout method (closest to Jev's
fast/calibrated positioning) by default. Not yet scheduled to a specific loop number — flagged
here so it isn't lost; likely Loop 5-6 after all 3 methods + 4 models work on the primary
`/bench` endpoint.

## Corrected understanding: Laya's real tool is `laya`, not `ggmlc-run` (Loop 5)

Main agent read Laya's real HF model card + `examples/laya/README.md` on GitHub directly
(2026-09-23) — supersedes Loop 1's exploration of the generic `ggmlc-run` binary. The real
tool is a separate `laya` binary (build target `examples/laya` in the `ggmlc` repo). Critically:
`laya serve <model> --port <p>` is **already a byte-compatible TypeSafe System One HTTP
server** (`POST /v1/systemone`, `/v1/decide`, `GET /health`) — confirms researcher-06's Jev
contract findings from an independent source. Design: run `laya serve` as a persistent
localhost-only background process on the server, `crates/models::laya` becomes an HTTP client
calling it (not a per-request CLI shell-out) — warm model, no per-call reload cost. See
`loop-05-dev-doc.md` for full detail. This also means a FUTURE Jev-compatible endpoint on our
own `apps/server` could just reverse-proxy to the already-running `laya serve` process instead
of reimplementing the protocol — worth considering when that later loop happens.

## Loops

| Loop | Status | D_t | A_t | E_t | gaps | regressions |
|------|--------|-----|-----|-----|------|-------------|
| 1 | delivered | loop-01-dev-doc.md | (workspace + remote server, commit e098942) | loop-01-evidence.md | 8 | 0 |
| 2 | delivered | loop-02-dev-doc.md | (workspace + remote server, real engine/models/pipeline, Qwen3-0.6B e2e via CLI) | loop-02-evidence.md | 3 new (G9/G10/G11), G3/G4/G5 closed | 0 |
| 3 | delivered | loop-03-dev-doc.md | (apps/server live on 103.146.166.46:80, curl-verified externally, commit ba0155b) | loop-03-evidence.md | 3 new (G12/G13/G14), 0 closed | 0 |
| 4 | delivered | loop-04-dev-doc.md | (all 3 LLM models live via CLI+HTTP, shared_backend fix, G13 respawn-supervisor proven) | loop-04-evidence.md | 2 new (G15/G16), G9/G13 closed | 0 |
| 5 | delivered | loop-05-dev-doc.md | (Laya wired via laya serve HTTP client, real 3-way BenchReport) | loop-05-evidence.md | G17 new, G6/G7-Linux/G8/G15 closed | 0 |

**DoD status after Loop 5**: items 1-4 ALL MET — server live, health 200, bench-via-curl valid,
all 3 LLM models + Laya benchmarked successfully via external curl (independently re-verified
by Runtime.check). Item 6 (no-auth) trivially true. **Only item 5 remains: the continuous
performance-tuning loop.** New gap G17 (Laya server binds 0.0.0.0 without auth, README's
`127.0.0.1`-only claim is false) — security-relevant but non-blocking per explicit no-auth-
this-round scope; cheap mitigation (local firewall rule restricting :8090 to localhost) worth
folding into Loop 6 if trivial.

**DoD status after Loop 4**: items 1-3 MET (server, health, bench-via-curl). Item 4: 3/4 models
done (Qwen3-0.6B, MiniCPM5-2B, Qwen3-4B all verified via external curl + CLI) — only Laya
remains. Item 5 (tuning loop) and item 6 (no-auth, trivially true) untouched. Found+fixed 2 real
bugs beyond D_4's literal scope: `BackendAlreadyInitialized` (blocked multi-model caching in one
process entirely — `shared_backend()`/`OnceLock` fix) and G13 SPOF (respawn-supervisor, proven
via real fault injection: ~85-124ms failure response, ~2.8s self-heal, empirically verified
twice independently — by Developer and again by QA).
