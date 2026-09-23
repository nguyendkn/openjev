---
loop: 17
status: pending
preservation_constraints:
  - "openjev-server (running the pool architecture from Loop 14) and laya serve both stay live and unaffected during development; only cut over at the end after validation"
  - "All 3 LLM models + Laya still correct via apps/cli and apps/server"
  - "G13 respawn-supervisor, graceful Laya degradation, G17/G18 firewall all still functional"
  - "cargo build --workspace (debug+release) and cargo test --workspace still pass"
  - "Bit-exact correctness vs the current (no-KV-reuse) behavior is the hard gate — a faster-but-different-output implementation is not acceptable here, unlike Loop 13's seq_align (which was an explicit, disclosed, user-approved trade-off) — this feature must not change any output"
  - "Use a dedicated CARGO_TARGET_DIR for this loop's builds (Loop 13 found a shared target/ directory hazard where concurrent builds silently overwrote each other's artifacts) — do not build in the same target/ another concurrent loop might be using"
---

## Objective
Implement KV-cache prefix/session reuse — NOT full-response caching (explicit user
instruction). Primary target: eliminate the redundant prompt prefill that happens TWICE per
request today.

## Context — the real opportunity, not the one originally hypothesized
Research (`researcher-12-kvcache-lmcache.md`) found LMCache doesn't apply (vLLM/SGLang-only,
solves GPU VRAM offload — a problem this CPU-only project doesn't have) and that cross-request
prefix reuse has limited value here because the 10 benchmark scenarios don't share a long
common prefix (no system prompt, minimal shared ChatML header).

**But there IS a real, guaranteed-applicable opportunity already in the existing code path**:
today, a single `/bench` request calls `run_readout(engine, prompt, options)` — which internally
calls `engine.decode_prompt(prompt)` (full prefill) — then `Engine::reset_context()` (clears the
KV cache specifically to avoid cross-pipeline contamination), then `run_generate(engine, prompt,
options)` — which calls `engine.decode_prompt(prompt)` AGAIN on the IDENTICAL prompt tokens.
**The exact same prompt is prefilled twice per request.** This is a real, currently-existing
redundancy — not a hypothetical cross-request scenario — and eliminating it is unambiguously
correct (same tokens, same result) rather than a speed/correctness trade-off.

`llama-cpp-2` exposes the primitives needed (confirmed in research): `get_state_size`/
`copy_state_data`/`set_state_data` for in-memory KV state snapshotting, or `state_seq_get`/
`state_seq_set` for sequence-scoped state. Use whichever is the correct, currently-supported
API — read the actual bound signatures in the resolved crate source (same "read the real API"
discipline this project has used throughout, not the general docs summary) before implementing.

## Tasks
1. **Confirm the exact bound API**: read `llama-cpp-2`'s actual resolved source
   (`~/.cargo/registry/.../llama-cpp-2-0.1.156/src/context/...`) on the server for the real
   signatures of the state-save/restore functions research found. Confirm which one is
   appropriate for "snapshot KV state after prefill, restore it before the second pipeline call
   within the same request" (likely the in-memory `copy_state_data`/`set_state_data` path —
   avoids file I/O entirely since this is same-process, same-request reuse, not
   cross-process/cross-run persistence).
2. **Implement the reuse in `crates/engine`**: after `decode_prompt(prompt)` finishes (the
   first time, in `run_readout`), snapshot the KV state instead of relying on
   `Engine::reset_context()`'s full clear. When `run_generate` needs to decode the SAME prompt
   again, restore the snapshotted state instead of re-running the full prefill, then proceed
   directly to the generation loop from that restored state.
3. **Preserve correctness-critical behavior**: the two pipelines must NOT contaminate each
   other's results (the original reason `reset_context()` existed) — snapshot/restore must give
   bit-identical KV state to what a full fresh prefill would produce, not an approximation.
   Verify this rigorously (see Validation Requirements) before considering this done.
4. **Wire into `apps/cli` and `apps/server`'s request flow**: both currently call
   `run_readout` → `reset_context` → `run_generate` in sequence — update this flow to use
   snapshot/restore instead of full reset+redecode, in both call sites.
5. **Benchmark the real speedup**: measure `constrained_readout_ms` + the prefill portion of
   `generation_ms` before/after, for all 3 LLM models, using the existing
   `scripts/bench-harness.sh`. Report honestly — the win is bounded by how much of total
   request time was spent on the (now-eliminated) second prefill vs. the (unchanged)
   autoregressive generation loop, which research/prior loops established as the dominant cost.
   A modest but real win is an acceptable, honest outcome — don't inflate it.
6. **(Stretch, only if time permits after 1-5 are solid)**: also explore the smaller
   cross-request win research identified — caching the fixed ChatML template header
   (`<|im_start|>user\n` etc., identical across every request to a given model) so its few
   tokens don't need re-tokenizing/re-decoding on every fresh request. Lower priority than
   Task 1-5's guaranteed intra-request win; skip if time-constrained and say so.
7. **Full regression**: 3 models × 3 methods via CLI + external curl, bit-exact output
   comparison against pre-change behavior for all 10 benchmark scenarios (not just the
   reference case), G13 fault-injection re-test, Laya-outage graceful degrade (unaffected,
   different code path), G17/G18 unaffected.

## Preservation
See frontmatter.

## Validation Requirements
- Given the snapshot/restore implementation, When compared against the OLD full-reset-and-
  redecode behavior on all 10 benchmark scenarios + the reference case, Then `readout` and
  `generate` results are BIT-IDENTICAL (not "close", not "within tolerance" — this is
  eliminating redundant identical computation, so the output must be identical) — prove this
  with a real diff, not an assumption.
- Given the real speedup measurement, When reported, Then it's the actual measured number
  (could be small — e.g. if prefill is only ~5-10% of total request time, that's the realistic
  ceiling) — do not round up or present a best-case number as typical.
- Given the full regression pass, Then no existing behavior (G13, Laya degradation, firewall,
  all 3 models × 3 methods) regresses.

## Out-of-scope
- Full-response caching — explicitly excluded by direct user instruction.
- LMCache or any vLLM-ecosystem tool — confirmed inapplicable by research.
- Cross-request KV persistence to disk/session files — the in-memory intra-request reuse
  (Task 1-5) is the primary deliverable; disk-based session caching adds I/O overhead/brittleness
  research flagged (large files, prefix-stability requirements) for uncertain benefit here.
- `crates/laya-native` — separate architecture (encoder-only, no KV cache/generation loop
  concept applies there).
