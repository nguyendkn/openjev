---
loop: 18
status: pending
preservation_constraints:
  - "openjev-server (pool architecture from Loop 14) and laya serve both stay live and serving correctly throughout; any model-file swap uses backup+rollback discipline (Loop 12 precedent) — never leave the server in a broken/half-swapped state"
  - "Bit-exact correctness is NOT the bar here (quant swap inherently changes numbers) but documented-scenario answer agreement must not regress vs current production behavior on the 10 benchmark scenarios + reference case"
  - "G13 respawn-supervisor, Laya graceful degradation, G17/G18 firewall all still functional after any change"
  - "cargo build --workspace (debug+release) and cargo test --workspace still pass"
  - "Do NOT touch crates/laya-native or attempt the Laya F16 swap — Loop 16 found it's blocked on laya-native's hardcoded GGML_TYPE_Q8_0 (graph.rs, head.rs); out of scope this loop"
  - "Use a dedicated CARGO_TARGET_DIR for any build in this loop, separate from other concurrent work"
---

## Objective
Act on Loop 16's K8s quantization-sweep findings, but ONLY after re-validating the two
proposed quant swaps on the ACTUAL production hardware (Xeon Gold 5320, Ice Lake, no
AMX-INT8) — Loop 16 explicitly flagged that its speed numbers came from a K8s pod with
AMX-INT8, which may not transfer to production. Also fix two real, unrelated bugs Loop 16
surfaced as side-findings.

## Context
Loop 16's full report: `docs/benchmarks/quantization-sweep-results.md` +
`docs/benchmarks/quant-sweep-loop16/` (raw data, reusable harness scripts incl.
`mkquants.sh`, `ppl.sh`, `bench_llm.py`). Key claims to verify on real hardware:
- **MiniCPM5-2B**: swap `Q4_K_M` → `Q8_0`. K8s pod measured this as BOTH more accurate
  (PPL +5.28% → +0.06%) AND 6.6% faster generation. If the speed win doesn't hold on
  Ice Lake (no AMX), it's still a valid accuracy-only upgrade (worth it if speed is
  merely equal, reconsider if speed regresses).
- **Qwen3-4B**: swap `Q4_K_M` → `Q5_K_M` (or `Q8_0` if RAM allows) — current production
  file has a real quality defect (+15.24% PPL, abnormal for Q4_K_M, likely missing an
  imatrix at quantize time), independent of the AMX question.
- Qwen3-0.6B: Loop 16 says keep as-is (already optimal) — no action needed.
- Exact conversion recipe (source files/revisions already pinned) is in
  `quantization-sweep-results.md` §7 — use it, don't re-derive.

## Tasks
1. **Fix the MAX_TOKENS=512 truncation bug** (Loop 16 side-finding, `crates/pipeline`'s
   `run_generate`): Qwen3's `<think>` reasoning is being cut mid-block by the 512-token
   cap, which Loop 16 measured as causing most observed "invalid JSON" failures —
   independent of and likely bigger than any quant change. Investigate: raise the cap
   (e.g. 1024-2048, measure the generation_ms cost), and/or suppress `<think>` mode via
   Qwen3's chat-template `enable_thinking=False` equivalent if `apply_chat_template`
   supports it (check `crates/engine`). Pick whichever fixes JSON validity without a
   disproportionate latency cost; report the trade-off honestly. Re-run the 10 benchmark
   scenarios for Qwen3-4B (worst-affected per Loop 16) to confirm valid-JSON rate
   improves.
2. **Fix the registry/reality mismatch** (Loop 16 side-finding): `crates/models/src/
   download.rs`'s Laya entry references `laya_english_ud_q4_k_m.gguf` but production
   `laya serve` actually runs `laya_english_q8_0.gguf` (per `scripts/start-laya-serve.sh`).
   Correct the registry entry to match what's actually served (or remove it if the
   registry path is genuinely dead code for Laya — confirm which before deciding).
   Low-risk, mechanical fix.
3. **Re-validate MiniCPM5-2B Q8_0 on production hardware**: download/build the Q8_0 GGUF
   per the pinned recipe, run `llama-bench` (or equivalent) directly on
   103.146.166.46 alongside the current Q4_K_M for a real head-to-head (tokens/sec,
   Ice Lake, no AMX) — do NOT trust the K8s pod's relative speed number as-is. Also spot-
   check the 10 benchmark scenarios' expected-answer agreement (already documented in
   `researcher-05-benchmark-usecases.md`) isn't regressed.
4. **Re-validate Qwen3-4B Q5_K_M (or Q8_0) on production hardware** the same way — real
   speed comparison against the current (defective) Q4_K_M file, plus scenario spot-check.
5. **Deploy whichever swaps pass validation**, using Loop 12's backup/rollback discipline
   exactly: back up the current GGUF before swapping, test live via curl, keep the backup
   path documented and ready to restore if anything regresses post-deploy. If a swap's
   production speed contradicts Loop 16's K8s number (e.g. MiniCPM5-2B Q8_0 turns out
   slower on Ice Lake), report that honestly and either skip the swap or present the
   accuracy-for-speed trade-off plainly — don't force a deploy that doesn't hold up.
6. **Full regression**: 3 models × 3 methods via CLI + external curl, G13 fault-injection
   re-test, Laya-outage graceful degrade, G17/G18 firewall unaffected, `cargo test
   --workspace` clean.
7. **Update `docs/benchmarks/quantization-sweep-results.md`** with a short "Loop 18:
   production validation" addendum recording the real Ice Lake numbers next to Loop 16's
   K8s numbers, and the final deploy/no-deploy decision per model with reasoning.

## Preservation
See frontmatter.

## Validation Requirements
- Given each proposed quant swap, When tested on the real production Xeon Gold 5320
  (not K8s), Then a real measured tokens/sec number is reported before any deploy
  decision — Loop 16's K8s number is a hypothesis to verify, not a given.
- Given the MAX_TOKENS fix, When the 10 benchmark scenarios are re-run for Qwen3-4B,
  Then valid-JSON rate is measured before/after and reported honestly (including the
  generation_ms cost of any token-cap increase).
- Given any deployed swap, Then a full regression pass (3×3 models×methods, G13, Laya
  degrade, firewall) shows no regression, and a rollback path is documented and was
  test-exercised (not just claimed).

## Out-of-scope
- Laya F16 swap — blocked on `crates/laya-native` hardcoded Q8_0, needs a separate
  coordinated loop.
- Any change to `crates/laya-native` itself.
- IQ-quant exploration (IQ4_XS etc.) — Loop 16 flagged this needs imatrix calibration,
  bigger effort, deferred.
