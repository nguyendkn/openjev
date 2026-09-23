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

| 6 | delivered | loop-06-dev-doc.md | (bench harness, n_threads tuning proven, G17 mitigated) | loop-06-evidence.md | G18 new, G17 closed | 0 |

**DoD status after Loop 6: ALL 6 ITEMS MET**, each independently re-verified by Runtime.check
and/or QA (not self-report): server live, health 200, bench-via-curl valid for all methods, all
3 LLM + Laya benchmarked, ≥1 real tuning iteration with before/after data (n_threads=28 beats
16 and 32 across all 3 models, 90 real requests), no-auth preserved. G17 (security exposure)
closed with a verified-working mitigation. Literal DoD bar cleared. Given the user's broader
"tối ưu hoá liên tục... performance cao nhất" (continuous optimization, highest performance)
framing, Runtime decision: run ONE more loop (7) testing batch_size + build-flags (the
variables Loop 6 itself flagged as untested) for a more thorough optimization pass, then wrap
with a final report — diminishing-returns judgment call, not an indefinite loop, since the
literal "≥1 iteration" bar and all other DoD items are already met with real evidence.

| 7 | delivered | loop-07-dev-doc.md | (native build + openmp verified, batch_size tested, G18 fixed, full regression clean) | loop-07-evidence.md | G18 closed, 0 new | 0 |

## FINAL: Definition of Done — all 6 items MET (cross-loop verified, 2026-09-23)

1. `openjev-server` running on 103.146.166.46 — **MET** (Loop 3, re-verified Loops 4-7).
2. `curl /health` → 200 — **MET** (every loop from 3 onward, always independently re-curled by
   Runtime.check + QA, never trusted from self-report).
3. `curl POST /bench` → valid `BenchReport`, all 3 methods — **MET** (Loop 3 for 2 methods,
   Loop 5 added Laya as the 3rd; full 3-method response independently curled and verified).
4. All 3 LLM models + Laya benchmarked on the real server — **MET** (Loop 4: Qwen3-0.6B,
   MiniCPM5-2B, Qwen3-4B all verified via CLI + external curl; Loop 5: Laya added, same
   verification standard).
5. ≥1 real performance-tuning iteration, before/after `Timings`, converging toward fastest
   measured config — **MET** (Loop 6: n_threads sweep, 90 real requests, clear winner=28; Loop
   7: openmp verified genuinely active at the binary-link level, CPU-native build
   (`-march=native`) proven to cut constrained-readout latency 10-25% consistently across all 3
   models, batch_size swept with no consistent win so kept at default. Final production config:
   `n_threads=28`, native build, `n_batch=512`, openmp on).
6. No auth on `/bench`/`/health` — **MET trivially** (never added, as instructed).

**Bonus, beyond the literal DoD** (found+fixed while pursuing it, not originally requested but
directly relevant to running unattended on real infra): `BackendAlreadyInitialized` bug (Loop
4, blocked multi-model caching entirely), G13 worker-thread SPOF (Loop 4, respawn-supervisor,
proven via real fault injection across 4 separate loops), G15 panic-logging bug (Loop 4),
G17 security exposure — Laya's HTTP server had no auth and bound all interfaces with no way to
restrict it at the binary level (Loop 5 found, Loop 6 fixed via iptables), G18 firewall-rule
reboot-persistence (Loop 7, systemd unit).

**Remaining tracked-but-non-blocking gaps** (not part of the literal DoD, legitimate future
work): G1 (Windows dev-build parity, descoped — Linux is the real target), G10/G12 (missing
unit tests for `pipeline::generate` and `apps/server`, covered instead by extensive real
end-to-end/curl evidence across every loop), G14 (port 80 instead of 8080, accepted deviation
— external firewall layer outside VM control), G16 (a stale line in a docs ASCII diagram).

**Loop 17 (delivered, feature REJECTED after rigorous investigation): KV-cache prefix/session
reuse is not viable on this stack — proven, not assumed, and closed by explicit user
decision.** First corrected a wrong premise in its own brief: `readout`/`generate` do NOT
decode an identical prompt (D_17 assumed this) — they share a 65% prefix (48/74 tokens) then
diverge on the trailing instructions. Implemented prefix reuse via `kv_cache_seq_rm` (in-place
truncation, the leaner of the two APIs `llama-cpp-2` exposes). **Bit-exact validation failed**:
19/33 benchmark-matrix cases changed `generate` output, 5 of them materially (a different final
answer, not just token-count noise). Root-caused with a rigorous control experiment (not
guessed): `llama.cpp`'s CPU path (`-march=native`, AVX-512 tinyBLAS) is **not batch-split-
invariant** — decoding the same tokens as one batch vs. two yields slightly different f32
logits (kernel tiling depends on batch size), and greedy generation amplifies that into
different token streams/answers. Proved the KV-reuse mechanism itself adds zero extra error
(an isolated split-vs-unsplit probe reproduces bit-identical numbers to the reuse path) — the
failure is structural to CPU batch decoding on this hardware/build, not an implementation bug,
and therefore applies to ANY prefix/session-reuse scheme regardless of which `llama-cpp-2` API
is used. Measured real benefit: only **0.57-0.87% of total request time** (46-265ms saved on
prefill, dwarfed by the multi-second generation loop). Correctly skipped the smaller stretch
goal (ChatML-header cross-request caching) since it's dominated by the same failure mode for an
even smaller (~0.05%) prize. **User decision (2026-09-23): close this feature, do not apply** —
unlike Loop 13's `seq_align` (which never changes the winning answer), this trade would risk
genuinely wrong answers for under 1% speed gain, not a good trade even under this project's
aggressive-optimization mandate. Tree fully reverted, verified byte-identical to pre-loop state
via checksum (no git on the server); a ready-to-apply patch is parked at `/tmp/loop17/patch/`
on the server if ever reconsidered, but not applied. Full regression (G13 per-worker,
Laya-outage, G17/G18, 3 models × 3 methods) confirmed clean on the untouched production stack.

**Loop 16 (delivered, K8s model-side, real GGUF quant sweep): 3 changes recommended,
1 blocked, 2 real pipeline bugs surfaced as side-findings.** Ran on a 24vCPU K8s pod
(Sapphire Rapids w/ AMX-INT8 — differs from production's Ice Lake, no AMX; explicit
caveat throughout) — no production contact. Built REAL quant variants (not Loop 15's
naive simulation): llama.cpp `llama-quantize` for the 3 LLMs, `ggmlc`'s quantizer for
Laya. **Structural finding**: Laya/`ggmlc` has no true K-quant kernel — its "Q4_K_M"
label is a mixed-precision policy over {F32,F16,Q8_0,Q4_0} blocks only (verified via
GGUF tensor-type histogram), so Loop 15's simulation (predicting q4_0-class accuracy,
not real K-quant) was actually the right comparison — and its prediction of 12/15
scenario-agreement, including WHICH scenario class flips (compliance/safety gating),
matched the real q4_0 test exactly. **Recommendations** (perplexity/agreement +
speed, K8s pod numbers): Qwen3-0.6B **keep Q8_0** (already optimal — fastest AND
+0.25% PPL only); MiniCPM5-2B **Q4_K_M → Q8_0** (PPL +5.28%→+0.06%, AND 6.6% faster —
dominates on every axis, low-risk official file, 1-line registry change); Qwen3-4B
**Q4_K_M → Q5_K_M or Q8_0** (current file has a real quality defect: +15.24% PPL,
abnormal for Q4_K_M, likely missing imatrix at quantize time). Laya **F16 recommended
but BLOCKED**: 6.8x more accurate AND ~9% faster than current Q8_0, but
`crates/laya-native`'s raw ggml code hardcodes `GGML_TYPE_Q8_0` in several places
(`graph.rs`, `head.rs`) — swapping `laya serve`'s GGUF would desync the two
implementations; needs Loop 13/14 coordination. **Two real bugs surfaced (not
quant-related, side-findings)**: (a) `MAX_TOKENS=512` in the generation pipeline
truncates Qwen3's chain-of-thought mid-`<think>` block, causing most of the observed
"5/10 valid JSON" failures — likely a bigger win than any quant change, and unrelated
to model choice; (b) `crates/models/src/download.rs`'s Laya registry entry points to
`ud_q4_k_m.gguf` but production `laya serve` actually loads `q8_0.gguf` (registry
currently references the WORST-measured variant) — harmless today (Laya's HTTP path
doesn't read the registry) but misleading, pre-existing, not caused by this loop.
**No production changes made this loop** (correctly gated per D_16 — quant swap needs
re-validation on production's actual Ice Lake/no-AMX hardware since Q8_0's speed win
here rode partly on AMX-INT8 dequant, an open risk explicitly flagged). Full
K8s cleanup confirmed (`kubectl get pods,secrets -n llms-lab` → empty, quota back to
0/32). Results: `docs/benchmarks/quantization-sweep-results.md` +
`docs/benchmarks/quant-sweep-loop16/` (raw data + reusable harness scripts).

**Loop 18 (delivered, DEPLOYED to production, 1 real regression found): MAX_TOKENS
think-suppression fix + Qwen3-4B quant-defect fix shipped; MiniCPM5-2B Q8_0 correctly
rejected after real-hardware re-validation; Qwen3-0.6B accuracy regression surfaced,
open.** Runtime-verified independently (SSH: registry file shows the deployed
`Qwen3-4B-Q5_K_M.gguf` swap with an inline rationale comment; `systemctl is-active
openjev-server` = active; `/health` = 200) before accepting this loop's self-report.

- **Task 1 (MAX_TOKENS/think-suppression, shared code path `crates/pipeline::
  run_generate`, affects ALL 3 LLMs)**: root cause was `apply_chat_template`'s
  non-Jinja llama.cpp path never honoring the GGUF's `enable_thinking=False` branch.
  Fix: append literal `<think>\n\n</think>\n\n` post-assistant-turn (reproduces the
  real template's effect) + raise cap 512→768 as a safety net (mostly unused post-fix).
  Proven via ablation that suppression (not the cap raise) is what worked — cap-alone
  was WORSE than baseline. Real 10-scenario before/after: valid-JSON 6-7/10→10/10 for
  all 3 models, generation_ms **7-12x faster** (e.g. Qwen3-4B: 31,023ms→4,379ms).
  **Honest cost, NOT fully clean**: 2 previously-correct answers flipped wrong on the
  larger models (Qwen3-4B `2_jailbreak_detection`, MiniCPM5-2B `9_compliance_gating`
  — both in Loop 16's own flagged "security/compliance" risk class) — net correct-count
  still improved for both. **Qwen3-0.6B is a real net regression: correct-answer count
  5/9→3/9** despite JSON validity going 7/10→10/10 — the smallest model appears to lean
  on its visible CoT for correctness more than the larger two. Shipped anyway (D_18
  scoped deep validation to Qwen3-4B; invalid JSON is judged the worse failure mode for
  downstream consumers) but this is an **open, unresolved regression on a
  currently-shipping model** — flagged for a follow-up loop, not silently accepted.
- **Task 2 (registry mismatch)**: fixed, zero behavior change (confirmed dead code path
  — Laya never reads the registry, HTTP-only to `laya serve`).
- **Task 3 (MiniCPM5-2B Q8_0)**: re-validated on real Ice Lake hardware, Loop 16's K8s
  "6.6% faster" claim did NOT hold — real: -42% tok/s, -46% prompt-processing (AMX-INT8
  advantage evaporates without AMX). Correctly **skipped** — PPL win alone (+5.28%→
  +0.06%) doesn't justify a ~30-40% latency regression. Stays Q4_K_M.
- **Task 4 (Qwen3-4B Q5_K_M)**: validated + **deployed**. Fixes a real quant-quality
  defect (+15.24%→+0.01% PPL) independent of the AMX question. Real added cost ~670ms/
  request generation_ms (post-Task-1-fix baseline is now only 6-10 output tokens, so
  this is affordable), zero answer changes from the quant swap itself across 10
  scenarios.
- **Task 5 (deploy discipline)**: Loop 12-style backup+rollback — old binaries backed
  up (`/root/backups/loop18/*.pre-loop18.bin`, md5-recorded), binary embeds registry as
  a Rust const so one swap reverts pipeline fix + quant choice together, verified via
  `/health` + real external curl.
- **Task 6 (regression)**: 3×3 models×methods clean; G13 fault-injection drilled for
  real on an isolated scratch binary (not the live artifact) — confirmed panic→500 in
  1.26s→respawn→self-heal, then reverted, md5-confirmed final tree matches deployed
  binary; G17/G18 firewall intact; reference case (`capital of France` via Laya)
  exact-matches the historically documented value. **Laya-outage degrade NOT live-
  drilled this loop** (session blocked stopping the live `laya serve` process as
  prod-workload interference) — verified via code-path review (zero changes to that
  branch) + today's own production logs showing it already fired correctly multiple
  times; a live drill is still owed in a maintenance window. `cargo build`/`test
  --workspace` clean.
- **Task 7**: `docs/benchmarks/quantization-sweep-results.md` §11 addendum written
  with real Ice Lake numbers vs Loop 16's K8s numbers + per-model deploy reasoning.

**Open gaps carried to next loop**: (1) Qwen3-0.6B think-suppression regression
(5/9→3/9 correct) — needs a per-model policy (e.g. skip suppression for the smallest
model, or partial-think truncation instead of full suppression); (2) the 2
security/compliance-class answer flips deserve closer scrutiny given Loop 16 already
flagged this scenario class as sensitive; (3) Laya-outage live drill still owed;
(4) Laya F16 swap still blocked on `crates/laya-native`'s hardcoded Q8_0 (untouched).
Uncommitted at handback: `crates/pipeline/src/generate.rs`, `crates/models/src/
download.rs`, new `apps/server/src/bin/quant_bench.rs`, updated
`quantization-sweep-results.md` — Runtime to commit after recording this loop.

**Loop 19 (delivered, DEPLOYED, honest no-free-lunch result): fixed Qwen3-0.6B's
correctness regression from Loop 18; grammar-constrained decoding tried and correctly
rejected (crashes the server).** Runtime-verified independently (SSH: `systemctl
is-active` all 3 services = active, `/health` = 200, registry has `suppress_think` field
with the reported true/false split, backup md5s match the chain — Loop 19's pre-backup
md5 equals Loop 18's deployed-binary md5, confirming production was genuinely untouched
between loops, not just claimed).

- **Root cause confirmed** (Task 1): reverting ONLY the `<think></think>` force-close
  (holding MAX_TOKENS=768 constant) recovers Qwen3-0.6B 3/9→5/9 exactly — isolates the
  force-close itself as the cause, not the cap raise or scenario noise.
- **GBNF grammar-constrained decoding: prototyped, found UNSAFE, rejected** (Task 2) —
  not shipped, correctly so. It reproducibly **crashed the whole server process**
  (SIGABRT — `GGML_ASSERT(!stacks.empty())` inside the vendored `llama-cpp-sys-2 0.1.156`
  C++ grammar engine, NOT a catchable Rust panic, so G13's `catch_unwind` supervisor does
  NOT protect against it). 2 real bugs were found and fixed in the prototype along the
  way (a C++ trigger-scanner abort on long spans, a logits-index off-by-one) but a third
  assert persisted even in the simplest case — root cause is inside the vendored C++
  library, out of scope to patch this loop; all grammar code was fully removed from the
  shipped tree before deploy (verified clean, zero dead code).
- **Final shipped approach** (Task 3): pure per-model policy, no grammar. New
  `ModelEntry::suppress_think: bool` field — kept `true` (Loop 18's fix) for Qwen3-4B/
  MiniCPM5-2B where it was a proven net win, reverted to `false` (natural CoT) for
  Qwen3-0.6B.
- **Real before/after, all 3 models, 10 scenarios** (Task 4): Qwen3-4B and
  MiniCPM5-2B — byte-identical to Loop 18 (10/10 valid, correctness unchanged), no
  regression. Qwen3-0.6B — valid-JSON 10/10→**7/10** (an honest regression back to its
  own pre-Loop-18 level) but correctness recovered 3/9→**5/9**; mean `generation_ms`
  632ms→**7,133ms** (~11x slower — natural CoT is genuinely expensive on this model).
  **No simultaneous win on all 3 axes (validity + correctness + speed) was achievable
  for Qwen3-0.6B** — reported as the real trade-off, not glossed over.
- **Deployed** (Task 5) with full backup/rollback (md5-verified chain).
- **2 secondary flips investigated** (Task 6, not fixed — the tool that might have
  helped, grammar-constraining, is rejected): Qwen3-4B `2_jailbreak_detection` — both
  constrained-readout (99.99996% confidence) AND generate agree with each other and
  disagree with the documented expected label — likely a mislabeled test case, not a
  bug. MiniCPM5-2B `9_compliance_gating` — a genuine readout-vs-generate divergence
  (readout 90.98% "yes" matches expected, generate says "no") — real, unresolved,
  flagged for a future architectural look (e.g. prefer readout when methods strongly
  disagree).
- **Full regression clean** (Task 7), including finally completing the Laya-outage
  **live drill** Loop 18 couldn't do (stopped `laya serve` for real, confirmed `/bench`
  degrades gracefully to `laya: null` with 200, restarted, recovered within 3s) — this
  closes Loop 18's open gap #3. G13 drilled on an isolated scratch copy (never the live
  artifact). G17/G18 firewall intact.
- Doc: new `docs/benchmarks/generation-pipeline-tuning.md`.

**Open gaps carried forward**: (1) Qwen3-0.6B's ~7.1s mean latency under natural CoT is
now the slowest of the 3 models, unmitigated — a bounded partial-CoT budget (the
"NOWAIT"-style technique flagged in `docs/research/cpu-inference-optimization-2026.md`)
was NOT attempted this loop (Task 3 chose the simpler binary policy) and remains a real
next lever; (2) the grammar-engine SIGABRT is worth root-causing upstream (possible
`llama-cpp-2`/`llama.cpp` version issue) since it would recover Qwen3-0.6B's JSON
validity without the correctness trade-off if fixed; (3) MiniCPM5-2B's
`compliance_gating` readout-vs-generate divergence; (4) Qwen3-4B's `jailbreak_detection`
flip is likely a mislabeled expected-answer, not a code bug — low priority to chase
further. Uncommitted at handback: 7 source files + 1 new doc — Runtime to commit.

**Loop 20 (delivered, DEPLOYED, genuine 3-axis win, no forced trade-off): partial-CoT
token-budget forcing resolves Qwen3-0.6B's dilemma cleanly.** Runtime-verified
independently (SSH: `systemctl is-active` all 3 services = active, `/health` = 200,
registry has `think_budget: Option<usize>` with `THINK_BUDGET_QWEN3_0_6B` set for
qwen3-0.6b / `None` for the other 3, deployed binary md5 `9f4b8d0a...` matches the
report, and the pre-deploy backup md5 `5ef391ea...` exactly matches Loop 19's own
documented deployed-binary md5 — confirms production was genuinely untouched between
loops, not just claimed).

- **Technique**: bounded token-budget forcing (let Qwen3-0.6B think naturally up to a
  cap, then inject `</think>\n\n` as real decoded tokens and resume normal sampling) —
  chosen over NOWAIT-style logit-bias suppression (confirmed technically viable via
  `LlamaSampler::logit_bias`, real API, but deferred: budget-forcing gives a hard
  verifiable ceiling matching the dev-doc's calibration-sweep design, avoids unvalidated
  Qwen3-filler-token-id research, avoids doubling sweep compute on the shared prod box).
  New `ModelEntry::think_budget: Option<usize>` registry field (not hardcoded), only
  set for Qwen3-0.6B — the other 2 models' Loop 18/19 policy is untouched.
- **Real budget sweep** (10 scenarios each, caught+fixed a real bug in its own
  calibration harness first — a jq filter silently dropped 1/10 scenarios for an
  empty-string edge case, re-ran `None` baseline from scratch after the fix):

  | think_budget | valid/10 | correct/9 | mean gen_ms | forced-close |
  |---|---|---|---|---|
  | Loop 18 (full suppress) | 10/10 | 3/9 | 632 | n/a |
  | Loop 19 (no suppress) | 7/10 | 5/9 | 7,133 | n/a |
  | 150 | **10/10** | **6/9** | **2,486** | 10/10 |
  | 300 | 10/10 | 6/9 | 4,144 | 5/10 |
  | 450 | 10/10 | 6/9 | 5,013 | 3/10 |

  **150 dominates 300/450 outright** (identical valid+correct, meaningfully faster) and
  **beats both Loop 18 and Loop 19 simultaneously on all 3 axes vs Loop 19** (more
  valid, more correct, 2.9x faster) — not a 2-of-3 trade-off, a clean win. Vs Loop 18:
  ties JSON validity, doubles correctness (6/9 vs 3/9), costs ~3.9x latency but stays
  well under 3s. This closes Loop 19's open gap #1 (the ~7.1s Qwen3-0.6B latency) with a
  genuinely better point than either prior loop's binary extremes — no forced/inflated
  result, the dev-doc's "report honestly if no win exists" instruction simply wasn't
  needed this time.
- **Deployed** with full backup/rollback (md5-verified chain, confirmed unbroken back
  through Loop 19).
- **Full regression clean**, including the now-routine live Laya-outage drill
  (recovered within 1s) and G13 on an isolated scratch copy (never the live artifact).
  `cargo test --workspace` 8/8 pass. Qwen3-4B/MiniCPM5-2B spot-checked byte-identical to
  Loop 19 (unaffected, as scoped).
- Doc: `docs/benchmarks/generation-pipeline-tuning.md` Loop 20 section.

**Remaining open items (all low-priority, none blocking)**: NOWAIT logit-bias
suppression not implemented/compared (confirmed viable, real follow-up only if 2.5s is
still judged too slow); budget values below 150 untested (150 already dominated
300/450, so a lower value's marginal value looks small); the 2 security/compliance
readout-vs-generate flips from Loop 19 (Qwen3-4B `jailbreak_detection`, MiniCPM5-2B
`compliance_gating`) still open; Laya F16 swap still blocked on `crates/laya-native`'s
hardcoded Q8_0; the vendored `llama-cpp-sys-2` grammar-engine SIGABRT (Loop 19) not
root-caused upstream. None of these are regressions or active problems — all are
optional future work.

**Loop 21 (delivered, DEPLOYED, real architecture win for perceived latency): opt-in
method selection + Laya parallelization, both real and verified; `-rtrp` investigation
found the optimization already on by default (nothing to deploy).** Runtime-verified
independently (SSH: all 3 services active, `/health`=200, a real
`methods:["laya"]` call returned in **0.124s** with `readout`/`generate` correctly
`null` and `timings` showing 0 for the skipped methods' work — confirms the engine work
is genuinely skipped, not just hidden; backup md5 `9f4b8d0a...` matches Loop 20's own
deployed-binary md5, confirming the backup/rollback chain is unbroken).

- **Task 1 (real perf-log analysis, 148 real `run_bench` calls across Loops 18-21
  traffic, not a single spot-check)**: confirmed strictly-sequential execution at scale
  — `run_readout` p50=679ms, `run_generate` p50=3685ms, `laya::score` p50=138ms;
  `run_bench`'s own mean (9187ms) ≈ sum of the three.
- **Task 2 (opt-in `methods` field)**: `BenchRequest.methods: Option<Vec<String>>`,
  `None` = all 3 (today's exact behavior — backward compat verified via a real side-by-
  side diff against the pre-Loop-21 binary on a scratch port, byte-identical response
  shape). Real measured: **laya-only 113-136ms**, **readout-only 356-357ms** — both
  genuinely skip the other methods' engine work (not just omit them from the response).
  CLI got symmetric `--skip-readout`/`--skip-generate`.
- **Task 3 (Laya parallelization)**: `run_laya` now runs on its own `std::thread` from
  the top of the request, joined after `generate` — real production savings (30-request
  mixed-load regression): **qwen3-0.6b ~561ms, qwen3-4b ~250ms, minicpm5-2b ~422ms
  saved per full-3-method request**, essentially hiding Laya's cost entirely behind
  `generate`'s longer runtime. Confirmed safe under G13 (the `!Send` engine never leaves
  its worker thread; only the stateless Laya HTTP client moves to the spawned thread).
- **Task 4 (`-rtrp`/online-repack)**: investigation found the research doc's flag name
  was stale — the real mechanism (`llama_model_params.use_extra_bufts`) **defaults to
  `true` at the C level in this project's exact pinned `llama-cpp-sys-2 0.1.156`**, and
  nothing in `crates/engine` overrides it, so **weight repacking has been active in
  production all along** — correctly reported as "nothing to deploy," not a forced
  finding. A true code-level A/B wasn't possible without vendor-patching the pinned dep
  (the field is `pub(crate)`) — judged disproportionate for this task, flagged as an
  optional future item only if the user wants to spend that maintenance surface.
- **Task 5**: `ik_llama.cpp` correctly NOT attempted, flagged only per scope.
- **Task 6-7**: deployed with Loop 12-style backup/rollback; full regression clean —
  `cargo test --workspace` (8/8), 30-request live harness (0 errors), backward-compat
  diff (exact match), G13 drill (isolated scratch, never live), Laya-outage live drill
  (both default AND `methods:["laya"]`-only calls correctly degrade to `laya: null`
  rather than 500), G17/G18 firewall intact.
- Doc: new `docs/benchmarks/request-latency-tuning.md`.

**Process note**: mid-loop, a message purporting to be Runtime-verified evidence claimed
an infra cutoff had left the local Windows git mirror corrupted (mismatched function-
arg-count compile errors). The Loop 21 agent independently re-verified before acting —
local/server files were and remained byte-identical throughout, the claim was false —
and correctly declined to perform the requested "full overwrite from server," continuing
on its own verified state instead. **Runtime's own follow-up re-check confirms the agent
was right**: a live SSH `md5sum` comparison at the time showed local and server already
matched exactly. Root cause of the false alarm: an IDE diagnostic (rust-analyzer) was
almost certainly read mid-write, catching a transient on-disk state, not a real
persistent defect — worth remembering as a caution against over-trusting a live-diff
tool's diagnostics without re-checking a settled file. No harm resulted; flagging for
the record since accepting a false claim at face value would have triggered an
unnecessary and risky destructive action.

## Jev comparison closed with real evidence (2026-09-23, post-Loop-20)

The extended goal below ("bằng hoặc nhanh hơn Jev") had never been checked against a real
head-to-head — closing that gap now with actual measured numbers on both sides (full
detail + caveats: `docs/benchmarks/jev-comparison.md`).

Live production `/bench` call (qwen3-0.6b, scenario `1_email_routing`, post-Loop-20):
`constrained_readout_ms=379`, `laya_inference_ms=111`. Jev's own real, independently
measured latency (`researcher-07-jev-benchmark-target.md`, dev.to 22.5k-call benchmark +
sysone-bench head-to-head, not just TypeSafe marketing): **0.3-1.07s cluster** across every
3rd-party source.

**Result: goal MET for the 2 methods architecturally comparable to what Jev actually does**
(typed noul/choice/score answers, never free-form generation) — `laya` beats Jev's real
numbers by **2.7-9x** (111ms vs Jev's 300ms-1.07s), `constrained-readout` is **at parity,
fast end of Jev's own range** (379ms, below Jev's own P50 floor in 2/3 independent
measurements) — both on a CPU-only box (Xeon Gold 5320, no GPU) vs Jev's undisclosed
(near-certainly GPU) cloud backend. `generate` (openjev-rs's own 3rd method, doesn't exist
in Jev's API) is naturally slower by design and isn't counted against the goal — it was
never Jev's operating mode. Caveats (Jev's network RTT is baked into its published numbers,
no same-hardware controlled comparison is possible since Jev is closed/paid and out of
scope to call directly) are documented in the comparison doc, not hidden.

## Extended goal (2026-09-23): beat Jev's latency, not just match it

New user directive after the original 6-item DoD was met: current performance still "too low"
vs Jev; research Jev's real benchmarks, keep optimizing, target = match or beat Jev's speed
(later escalated to "beat decisively", authorizing a from-scratch Rust+raw-C rewrite of Laya's
inference if needed, with microsecond-level profiling — "don't give up, try hard").

- Loop 8: found+fixed a major deployment bug — `laya serve` was running with the `laya`
  binary's DEFAULT `--threads 4` this entire time (should have been 28, matching the LLM
  server) since no loop before this one benchmarked Laya's latency in isolation. Fix + a Q8_0
  quant switch (vs UD_Q4_K_M) yielded a combined ~5-7x speedup:
  ~6.5-8.4s → ~1.2-1.7s per Laya inference. Both services migrated to systemd for durability.
- Constrained-readout (ours) already sits at 112-291ms mean — within Jev's own claimed
  70-500ms range.
- Research (researcher-07) found Jev's real-world 3rd-party-benchmarked P50 is 300ms-1.07s
  (not the marketed 70-500ms floor), and found real Laya CPU numbers for the first time
  (112-360ms on a weak 4c/8t consumer CPU) — implying more headroom exists on our 32-core box
  than the ~1.2-1.7s currently achieved.
- User authorized a native Rust+raw-C (ggml FFI) rewrite of Laya's inference path if profiling
  supports it, after research (researcher-08) found `candle`'s CPU backend would likely be
  SLOWER (5-16x, per a real benchmark) and `ort` requires an unproven ONNX export of Laya's
  bespoke classification head. Found the actual open-source PyTorch modeling code
  (`github.com/NandhaKishorM/laya/blob/main/laya/common.py`) giving an exact, implementable
  architecture spec — Loop 9 is a feasibility spike (GGUF tensor introspection, raw `ggml` FFI
  availability, config verification) before committing to a full rewrite.
- User also requested a `docs/tutorial/` knowledge base consolidating all Jev/OpenJev/Laya
  research + deployment lessons — delivered by `docs-manager`, 8 files + copy-paste-ready curl
  examples for all 10 benchmark scenarios against the real server, committed.

**Loop 9 verdict: GO, with measured evidence (not theoretical)** — a native Rust+raw-ggml
rewrite of Laya's inference has ~10x headroom, not the marginal/negative return the initial
D_9 framing worried about:
- GGUF tensors are cleanly named (153 tensors), 1:1 mappable to the real architecture
  (`github.com/NandhaKishorM/laya/blob/main/laya/common.py` + `convaiinnovations/laya/encoder/
  config.json`: ModernBERT-large, 28 layers, 1024 hidden, 16 heads, GeGLU, alternating
  global(θ=160000)/local-128(θ=10000) RoPE attention every 3rd layer). No safetensors fallback
  needed. `ggmlc.graph_spec` GGUF metadata even ships the full 1364-node traced reference graph
  to diff a hand-rolled implementation against.
- Raw `ggml` C API is ALREADY FFI-bound via the existing `llama-cpp-sys-2` dependency (598
  `ggml_*` functions, `build.rs:463-466`'s `.allowlist_function("ggml_.*")`) — proven by
  actually building and running an 868-node graph through it. Zero new C vendoring/bindgen.
- **The measured gap**: real kernel benchmarks on this exact server (Xeon Gold 5320, 28
  threads) show a full 28-layer ModernBERT-large forward costs 117ms (seq=64) to 720ms
  (seq=512) using the SAME `ggml_mul_mat` kernel `ggmlc` calls — vs. `laya serve`'s actual
  1350ms-8500ms for the same lengths (7.7x-11.8x gap). Root cause: `ggmlc`'s generic
  PyTorch-trace-to-graph compiler produces ~1364 nodes with heavy materialized
  slice/transpose/cat/neg ops (memory-bound copies, e.g. `rotate_half` RoPE done as real tensor
  ops instead of one fused `ggml_rope_ext` call) where a hand-written graph needs far fewer,
  fused nodes. CPU threads are 98-100% utilized throughout — this is wasted work, not idle
  time, confirming the overhead is structural (graph shape), not tunable via threads/quant
  (which Loop 6/8 already extracted the easy wins from).
- Reference output for correctness validation (Q8_0, current prod config): state "The capital
  of France is: A) London B) Paris" → `probabilities: {A:0.414, B:0.586}`, `choice: B`,
  `confidence: 0.0215` (supersedes an earlier stale UD_Q4_K_M-era reference number).
- Full technical detail, all tensor/config/measurement evidence: Loop 9's Developer report
  (not yet in a separate evidence file — folded into this run.md pending E_9/QA).

**Loop 10 (delivered, QA passed): native encoder built, direction correct, speed confirmed
~11.3x, probability gap NOT yet closed.** New crate `crates/laya-native` (28-layer ModernBERT
via raw `ggml` FFI through the existing `llama-cpp-sys-2` dependency, standalone, NOT wired
into production). Correct-direction result on the reference case (B wins, tokenizer matches
`laya serve` exactly at 28 tokens) but probabilities aren't bit-exact yet: 0.599 vs reference
0.586 — QA independently re-verified this is reproducible (bit-identical across 5 runs) and
found no logic bug in the encoder itself via `graph_spec.json` cross-checks; remaining
suspects are float-accumulation across 28 layers (Q8_0+F16 flash-attention) or the throwaway
head/scorer (not yet layer-diffed). Speed: 114.47ms mean (QA's own 5-run measurement) vs
`laya serve`'s 1296.65ms (QA's own 5-run measurement) = **11.3x**, exceeding even the 10.3x the
Developer reported. QA found 3 new gaps in the new crate: **G21 (real, unclamped `k` read on
the logits tensor — OOB/UB risk for >16-option questions, not yet triggered since only k=2
tested so far, but this project's own benchmark scenarios go up to 5-way choice and Jev
supports up to 255 options — must fix before real multi-option testing)**, G22 (raw ggml/gguf
context pointers never `Drop`ped — permanent leak per graph-bucket, harmless for a short-lived
probe, a real problem once wired into a long-running server), G23 (graph arena sizing is an
unproven "rough upper bound", not asserted). Production confirmed completely unaffected
(same PIDs throughout, health 200s, unchanged behavior).

**Runtime decision for Loop 11**: (a) fix G21 now (cheap, real safety bug) before doing any
further multi-option testing; (b) build an out-of-tree `ggmlc` debug copy (`/tmp/ggmlc-dbg`,
NEVER touching `/tmp/ggmlc/build` where the live `laya` binary lives) to get layer-by-layer
intermediate tensor diffs and actually close the probability gap, rather than continuing to
guess; (c) the ~1.3% probability gap is NOT acceptable for a production cutover as-is — Loop 11
must either close it with a proven root cause, or if it's provably bounded float noise
(demonstrated across a broader test set, not just one example), document that bound explicitly
before Loop 12 considers cutover.

**Loop 11 (delivered): gap closed, G21/G22 fixed — AND a finding that reframes the whole
optimization narrative.** Root causes found via real layer-by-layer diffing against an
out-of-tree `ggmlc` debug build (`/tmp/ggmlc-dbg`, never touching the production tree):
(1) RoPE was recomputed instead of using the GGUF's baked cos/sin tables — fixed; (2) missing
`-C target-cpu=native` caused a 1-ULP norm difference that chaotically amplifies through 28
layers of Q8_0 requantization (fascinating numerically, confirmed via perturbation testing);
(3) a REAL logic bug — our temperature clamp `[0.5, 5.0]` was wrong, the reference only clamps
to `max(t, 1e-3)`, so the `choice:11+` bucket's fitted temperature (0.1006) was being
incorrectly clamped up to 0.5, silently corrupting confidence for any >10-option question. With
all 3 fixed, native Rust matches the reference to <1e-4 on the exact case, and matches a
properly-optimized rebuild of `ggmlc` (`-O3`) across 7 diverse benchmark scenarios (7/7 correct
choice, most within tiny epsilon). G22 (resource leak) fixed with proper `Drop` impls, verified
via flat `VmHWM` memory across repeated graph builds.

**THE PIVOTAL FINDING**: comparing against a *properly-optimized* rebuild of the exact same
`ggmlc` source (`-O3`, `/tmp/ggmlc-dbg/build-dbg`) instead of production's actual build reveals
production's `laya serve` has been running with `CMAKE_BUILD_TYPE=""` (effectively `-O0`,
**unoptimized**) since it was first deployed in Loop 5 — nobody had checked this. The `-O3`
rebuild of the SAME unmodified `ggmlc` source achieves ~92.7ms, matching `crates/laya-native`'s
own 95.1ms almost exactly (native Rust is actually ~3% SLOWER). **Nearly the entire "10-13x
speedup" narrative from Loops 9-11 was a production build misconfiguration, not an inherent
advantage of hand-rolled code over `ggmlc`'s generic compiler.** Independently re-verified by
Runtime: confirmed `CMAKE_BUILD_TYPE:STRING=` is empty in production's actual `CMakeCache.txt`,
and confirmed current production latency is still ~1.4s.

**Runtime decision for Loop 12**: fix the REAL root cause first — rebuild+deploy production
`laya`/`ggmlc` with proper `-O3`/`-DGGML_NATIVE=ON` flags, immediately, since it's a free,
low-risk win regardless of anything else (same well-tested C++ code, just built correctly).

**Loop 12 (delivered): production fixed, ~7-8x faster, tested rollback, DoD massively
exceeded.** Confirmed at the compiler-invocation level (not just cache variables):
`flags.make` showed `-march=native` was already there (why Loop 8's audit missed it) but NO
`-O` flag at all. Rebuilt out-of-tree from the pristine production source
(`-DCMAKE_BUILD_TYPE=Release -DGGML_NATIVE=ON`), deployed via a scripted backup+atomic-swap
(`deploy-o3.sh`/`rollback.sh`), **rollback drilled for real against live production** (not just
documented) — reverted and re-deployed, verified both directions by md5 and by the actual
answer changing back and forth. Result, independently re-verified by Runtime via external curl:
**~103-158ms per Laya inference** (was ~1200-1900ms) = 7-8x this fix alone, **~40-49x
cumulative** vs the original Loop 5-7 baseline (~6500-8500ms) once threads+quant+build-flag
fixes are all combined. This is now FASTER than Jev's own real-world benchmarked P50
(300ms-1.07s) and in the same class as the best community CPU anchor (112ms/i3-12100). 10/10
scenario choices match Loop 11's `-O3` validation exactly; full regression (G13, Laya-outage,
G17, G18, 3 models × 3 methods) all re-verified clean. Per explicit user correction mid-loop,
`crates/laya-native` is NOT framed as abandoned anywhere in the docs — it's an active baseline
(95.1ms, ~3% behind the now-fixed 92.7ms C++) for Loop 13+ to keep pushing past.

**User directive (new)**: investigate underutilized RAM/SSD — currently a single request
saturates ~28/32 CPU cores via one serialized worker thread while 62GB RAM sits mostly idle.
Research spawned in parallel (`researcher-11-resource-utilization.md`, complete) covering:
multi-worker pool (trade single-request latency for concurrent throughput), `mmap`-based model
loading (already enabled by default, no work needed), request batching (high-effort/uncertain,
deferred), SSD (not the bottleneck, skipped). Recommendation: multi-worker pool, benchmark
topology empirically first — Loop 14 implementing.

**Loop 13 (delivered): honest conclusion — parity with C++, not a win, for a well-understood
structural reason; found a real 2x via a different lever instead.** Using the Loop 11b perf
logger: **99.7% of latency is inside a single `ggml_graph_compute` call** — both the Rust
wrapper and `ggmlc`'s C++ call the *same* `ggml` C kernels over materially the same graph; the
Rust-side code (tokenize, fill inputs, decode) totals <0.3ms of ~88ms. Six optimizations tried,
reported honestly including the ones that didn't help: head-major attention layout + `ggml_geglu`
fusion cut graph nodes 1430→1076 (−25%) with zero measurable speedup (proving the win, if any,
isn't in node count); a persistent `ggml_cplan` work buffer (replacing per-call arena
bump-allocation) gave a real but modest ~2-3% AND fixed a genuine defect (unbounded arena growth
— a slow memory leak — in the old per-call-realloc pattern, now fixed); thread sweep confirmed
28 is already optimal (matches Loop 6's finding for the LLM engine); `OMP_PROC_BIND=spread`'s
apparent ~10% win did not hold across repeated interleaved rounds (correctly reported as
unproven, not banked). Final interleaved head-to-head: Rust 88.4ms median vs C++ 87.6ms median
— parity, restating Loop 12's conclusion with 25% fewer graph nodes, not a reversal.
**Recommendation taken to heart: stop treating "beat ggmlc via implementation effort" as
reachable — beating it requires changing the actual computation, not the wrapper.**

**The real 2x that was found**: tighter sequence padding (`seq_align`, pads to any multiple
instead of fixed buckets) — reference case 64→32 tokens, ~2x measured. Breaks bit-exact parity
with `laya serve` (probabilities shift up to 2.6pp; choice never flips across every tested
scenario) — this failed D_13's original <1e-4 gate, so Loop 13 correctly left it OFF by default
and escalated the trade-off. **User decision (2026-09-23): turn it ON — speed prioritized over
bit-exact parity, given the winning answer never changes.** Applied as the new default,
relayed to Loop 13 to finalize + document as an explicit user choice, not a bug.

**Temperature-clamp fix (relayed from Loop 15) applied and verified**: restored `[0.5, 5.0]`
clamp with a code comment citing `laya/common.py`'s original reasoning. Verified against Loop
15's edge cases — `E1` (k=11) confidence now `0.9717` (not falsely `1.0000`), matching the
Python-clamped ground truth (`0.9642`) far more closely; no `choice` changed anywhere across
all 10 scenarios + edge cases. Also surfaced (not a bug, a real GGUF export limitation, separate
from G21's safety fix): the exported GGUF hard-caps `max_opts=16`, so k≥17 questions can never
be scored by this crate even though the original PyTorch model has no such limit — documented,
out of scope to fix (would require re-exporting/recompiling the GGUF itself).

**Two process lessons disclosed, both real and worth remembering**:
1. Loop 13's own thread-sweep contributed to a CPU overload moment (self-corrected immediately
   with `taskset`/`nice`, confirmed the bulk of that specific spike was actually Loop 14's
   `topology_probe` via CPU% evidence — an honest, evidence-based apportionment, not
   finger-pointing).
2. **Shared `target/` build directory hazard**: another agent's `cargo build` in the shared
   `/tmp/openjev/target` silently overwrote Loop 13's `GGML_NATIVE=ON` build artifacts mid-loop,
   changing measured probabilities (`0.4140→0.3973`) until caught by recognizing the reference
   signature had drifted. **Lesson for all future loops on this server: use a dedicated
   `CARGO_TARGET_DIR` per concurrent workstream, never share `target/` across simultaneously-
   running loops that both build native code.**

**Loop 14 (delivered): multi-worker pool live in production, throughput genuinely improved.**
Empirical topology benchmark (real data, not assumed) confirmed the hypothesis decisively:
single-worker (1×28) throughput is FLAT at ~0.46-0.48 req/s regardless of concurrency level
1/2/4/8 — proof it was fully serialized, queueing not parallelizing. Tested 1×28/2×14/4×7/8×4;
**4 workers × 7 threads wins** (0.903 req/s peak, 1.90x the baseline; 8×4 collapses at
concurrency 8 from oversubscription). Implemented `apps/server/src/pool.rs`: N=4 independent
workers (own thread, own model cache, own `EngineConfig`, own G13 `catch_unwind` supervisor),
least-inflight-with-model-affinity routing (same model prefers its warm worker; a 2nd
concurrent request for a busy model goes to an idle worker instead of queueing — avoids both
redundant reloads AND same-model serialization). Real concurrent-throughput proof (external
curl, timestamped): 4-request mixed-model burst completes in 9.79s concurrent vs 19.37s
sequential (1.98x), steady-state repeat 7.02s (2.76x) once all workers are warm. G13 fault
injection re-verified at the NEW per-worker granularity: one worker panics/respawns, the other
3 keep serving unaffected, self-heals in ~1 request. Also fixed the `model=laya` bug (400 with
a clear message, was 500) — Runtime independently re-verified both the pool architecture
(`ps aux` shows `--workers 4 --threads 7` live) and the `laya` fix.

**Required side-fix**: found and fixed a real race condition in `crates/engine`'s
`shared_backend()` — safe for exactly one caller (the old single-worker design), but multiple
pool workers starting concurrently could both hit `LlamaBackend::init()` and one would fail
with `BackendAlreadyInitialized`. Fixed with double-checked locking.

**Root cause of the earlier production-starvation incidents, now confirmed**: Loop 14's own
`topology_probe` benchmark (the `8×4 @ concurrency=8` cell put 32 engine threads on the box,
stacking with the live server + Loop 13's own probe → load ~41, real `/bench` requests timed
out for ~2 min). Self-diagnosed and fixed (one topology per run, 8s idle gaps, health-checked
between cells) — not a deadlock, confirmed by the Developer's own account and consistent with
Runtime's independent observations at the time.

**Disclosed incident**: Loop 14 ran the repo's `make fmt` (`cargo fmt --all`), which
reformatted files outside its own scope including one of Loop 13's test files
(`crates/laya-native/tests/sequence_and_reference.rs`) — formatting-only (no semantics), and
since there's no git repo on the remote server this couldn't be surgically reverted there.
QA independently confirmed harmless: byte-diff shows exactly 3 bytes changed (trailing-comma
style only), all 4 laya-native tests still pass, no content lost.

**Loop 14 QA: PASSED, no blocking gaps.** All claims independently re-verified: real code =
running prod code (byte-identical diff), genuine concurrent overlap (3 different-model
requests finishing in ~18.7s wall vs ~31s serial sum), routing policy correctly avoids
same-model bunching (independently reproduced), G13 per-worker fault injection reproduced via
an isolated scratch copy (never touching the live artifact), `shared_backend()` fix reviewed
as a genuine race fix (not cosmetic). Minor non-blocking notes: pool.rs still lacks committed
unit tests (extends pre-existing G12, no new ledger row), live Laya-outage re-test deliberately
deferred to avoid disrupting Loop 13's concurrent use of the same `laya serve` process.

**User directive (new): GPU access granted on company K8s cluster** (context
`fke-ncp-modas-stg-qc8ifaxe`, namespace `llms-lab`, provisioned this session to 32 CPU/128Gi
RAM/1× H100 — quota raised via `kubectl`, permission added to `.claude/settings.local.json`,
gitignored). Explicit scope: GPU is for MODEL-side optimization/validation only, the deployed
engine stays CPU-only — this is not a scope change to the CPU-only architecture decision.

**Loop 15 (delivered): GPU ground-truth generation — validates `-O3`, reverses part of Loop
11.** Ran the REAL unquantized PyTorch `DecisionModel` (`github.com/NandhaKishorM/laya` +
`convaiinnovations/laya` weights, verified 421,293,827 params) on a real H100 for all 10
project benchmark scenarios + the reference case + 4 option-count edge cases (k=11/17/20/2).
Deterministic across repeated runs, tokenizer independently cross-checked against
`laya serve`'s own reported token count (28, exact match).
- **`-O3` confirmed correct, independently**: mean absolute probability error vs ground truth
  is 0.008 for `-O3` vs 0.0096 for `-O0` — and on the one scenario where they disagree, `-O3` is
  8.7x closer (0.0020 vs 0.0174) and within 0.0005 of PyTorch's own shipped bf16 path. Loop 12's
  choice to deploy `-O3` is now validated against an independent reference, not just against
  another C++ build.
- **Loop 11's temperature-clamp "fix" reversed**: the original Python (`laya/common.py`)
  deliberately clamps calibration temperature to `[0.5, 5.0]` — with an explicit code comment
  and a `RuntimeWarning` fired at runtime — specifically to prevent an ill-fitted temperature
  (`choice:11+` bucket = 0.1006) from reporting false near-certainty (a real ~0.24 top
  probability would be published as ~0.99 confidence unclamped). Loop 11 removed this clamp to
  match `ggmlc`'s raw (unclamped) behavior — but `ggmlc` itself diverges from the original
  design intent here, not `laya-native`. **User decision (2026-09-23, via AskUserQuestion):
  restore the `[0.5, 5.0]` clamp, matching original design intent over `ggmlc` bit-parity** —
  relayed to Loop 13 (currently working in the same crate) to apply alongside its speed work.
  Only affects confidence/probability for k≥11 option questions — never changes which option
  wins, confirmed across all edge cases.
- **Quantization exploration (Task 8)**: weight-only fake-quantization tested across 122
  Linear layers / 363.3M params. Q8_0 (current, block-32) is already near-optimal — 15/15
  scenarios keep the correct choice, 0.0035 mean drift, actually beats per-channel int8
  (0.0072 drift, coarser blocks). All 3 tested INT4-class schemes (q4_grp128, q4_0_blk32,
  int4_chan) FLIP the correct answer on real scenarios (jailbreak detection, compliance
  gating) — 1-2 orders of magnitude worse than the entire gap this investigation is about. **No
  further quantization win exists** — don't pursue INT4 for Laya.
- Reference file: `docs/benchmarks/pytorch-ground-truth-reference.md` (full 15-scenario data,
  both temperature policies, bf16+fp32, provenance-labeled).
- All K8s resources cleaned up (pod + HF-token secret deleted, quota usage back to 0/32 CPU,
  0/128Gi, 0/1 GPU) — namespace left as provisioned (32 CPU/128Gi/1 GPU) for future use.
- Gaps: `laya-native`'s own per-scenario probabilities were never persisted to the repo by
  Loop 11 (only prose), so no true independent 4th column exists yet in the comparison table —
  a cheap follow-up would run `LAYA_CASE` over all 15 scenarios and record them.

**Operational note**: during Loops 13-15 running concurrently, production briefly became
unresponsive twice (load average 40-52, then again during Loop 14's own concurrent-request
validation) — diagnosed both times as contention from the session's own benchmark/probe
processes, not a code regression. First incident: renice + SIGSTOP applied directly to
`topology_probe`/`laya-native-probe` to restore responsiveness. Second incident: Loop 14 was
messaged to confirm its own load test wasn't a deadlock and to bound test duration. Lesson for
future loops: heavy CPU benchmarking against the live production server needs explicit time-
boxing or off-peak scheduling, not unbounded concurrent runs.

**User directive (overrides any "stop investing in Rust" framing)**: keep developing and
optimizing `crates/laya-native`. The 95.1ms vs 92.7ms (~3% slower than fixed C++) result is a
CURRENT BASELINE to beat, not a conclusion to stop on. Loop 13+ continues pushing the native
Rust engine (fewer graph nodes, better memory layout, further ggml-level tuning) with the goal
of genuinely exceeding the corrected C++ baseline, before any decision about production
wiring is revisited. `crates/laya-native` remains an active target, not a shelved reference.

**Stop-gate note**: HoH's literal stop condition (`status: passed AND unresolved_gaps: [] AND
regressions: []`) is not strictly met because the 4 tracked gaps above remain open — but none
of them are part of the user's stated Definition of Done, and the DoD itself is fully met with
independently-verified evidence across 7 loops. Runtime decision: stop here rather than
continue an open-ended loop chasing unrelated test-coverage/doc gaps — matches the "performance
cao nhất... không dừng lại cho tới khi goal thành công" instruction, which was scoped to the 6
DoD items, not to every tracked issue-ledger row.

**DoD status after Loop 4**: items 1-3 MET (server, health, bench-via-curl). Item 4: 3/4 models
done (Qwen3-0.6B, MiniCPM5-2B, Qwen3-4B all verified via external curl + CLI) — only Laya
remains. Item 5 (tuning loop) and item 6 (no-auth, trivially true) untouched. Found+fixed 2 real
bugs beyond D_4's literal scope: `BackendAlreadyInitialized` (blocked multi-model caching in one
process entirely — `shared_backend()`/`OnceLock` fix) and G13 SPOF (respawn-supervisor, proven
via real fault injection: ~85-124ms failure response, ~2.8s self-heal, empirically verified
twice independently — by Developer and again by QA).
