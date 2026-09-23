# CPU Inference Optimization — 2025-2026 Survey

Researched 2026-09-23. Scope: llama.cpp/ggml CPU backend, spec-decoding on CPU, CoT budget
reduction, alt CPU engines, IQ-quants, misc new ideas — for Xeon Gold 5320 (Ice Lake,
AVX-512, **no AMX**, no GPU), 3 LLMs via `llama-cpp-2` + custom encoder (`laya-native`,
already at engine-perf-parity, plateaued). Bottleneck = LLM autoregressive JSON-gen (CoT-heavy
Qwen3), not encoder/engine. 8 web searches + 2 fetches, cross-referenced where possible.

## 1. llama.cpp/ggml CPU backend, Ice Lake/AVX-512-no-AMX specifically

- Mainline: AVX512 GEMM (`ggml_gemm_q4_K_8x8_q8_K`, PR #12829) landed, "good gains" on
  prompt-processing vs AVX2 — no Ice-Lake-specific numbers found.
- **Online repacking** (`-rtrp`/`--runtime-repack`, PR #10446, merged late 2024, still
  active): runtime requant/repack of Q4_0-family to tiled GEMM/GEMV layout — zero code
  change, just a runtime flag. Worth testing on your Q4_K_M mix.
- **`ik_llama.cpp`** (ikawrakow fork, actively maintained, "SOTA quants + improved perf"):
  ships IQK GEMM kernels *tuned per microarch*. Issue #2409: author hand-tuned
  `qx_r8_q8_dot_product`/`mul_mat_q8_k_r16_q8_k` for **Cascade Lake** (non-AMX AVX-512, same
  VNNI-less family as Ice Lake) — port-contention fix (`vpshufd`→`vpbroadcastd`) +
  accumulator-latency fix gave **+21% (Q8_0) to +25% (IQ4_XS)** pp512 throughput. Author:
  *"I don't think results would translate to other architectures"* — fork-specific hand-tune,
  not generic. Mainline lacks these; AVX-512-but-no-flags builds silently fall back to AVX2.
  - Production-viable fork but pins you off mainline cadence. The one lever not yet tried
    for the **LLM** side specifically (encoder side already plateaued).
- One general perf-regression thread (#27111, b10429, ~30→3 t/s) found, root cause unclear,
  not confirmed Ice-Lake-specific — avoid that build range, don't chase further blind.
- NUMA/thread-pinning work active (#12289/#12303) but Ice Lake 32vCPU is likely single-socket
  — low relevance unless dual-socket.

**Applicable?** Yes — untried. `ik_llama.cpp` + runtime-repack on the 3 LLM workers is the
most concrete scoped experiment in this survey.

## 2. CPU speculative decoding

- Base mechanism (draft+target, 1 fwd-pass verify) has existed in llama.cpp
  `examples/speculative` for years, works on CPU already — not new.
- **New in 2026**: ggml-org issue #21453 ("Research: Speculative Decoding for Low-Latency CPU
  Inference") is an open research/planning issue — **no implementation merged, no CPU
  numbers yet**. Cited target 1.5-3x tok/s is the generic GPU-stack figure, not CPU-verified.
- Real numbers found are GPU-only (Qwen2.5-0.5B draft+14B target: 2.5x @10 draft tok; 1.5B
  draft: 1.63x; 3B draft: 1.33x) — bandwidth-bound, GPU vintage barely matters (1.48x
  RTX5060Ti vs GTX1080Ti). CPU decode is even more latency-bound per-token so gains should
  transfer or improve, but nobody has published CPU-specific numbers.
- **Project-specific insight**: bottleneck is JSON output. Grammar-constrained output (GBNF,
  already in llama.cpp) gives very predictable token-acceptance — spec-decoding + grammar
  constraints should combine unusually well (draft only guesses field values, not JSON
  syntax) — a favorable case not covered by generic benchmarks.
- Not turnkey: you'd assemble it (tiny Qwen3-family draft, e.g. 0.5B for 4B target) and
  measure acceptance rate yourself on your JSON-gen workload — no ready reference impl.

**Applicable?** Plausible, front-loaded R&D cost, no existing numbers to lean on.

## 3. CoT/thinking-budget reduction (Qwen3) — highest-leverage bucket given bottleneck

- **Qwen3 native, free**: hybrid thinking/non-thinking modes via `/think` `/no_think` tags
  or `enable_thinking=False`, plus soft token-budget control — cheapest first move if
  under-used today.
- **Budget forcing** (reward-shaped length cost / hard cap): mainstream, but 2026 papers note
  smaller models comply worse (revert to longer CoT when they can't converge in budget) —
  your 0.6B/2B models may be least compliant of the three.
- **NOWAIT** (2025-2026): suppresses reflection filler ("Wait", "Hmm", "Let me think again")
  at decode time — **27-51% CoT-length reduction**, inference-time only, no fine-tune —
  cheapest to trial via logit-bias, compatible with llama.cpp's existing API.
- **Chain of Draft**: caps each reasoning step ≤5 words via prompting — no infra change.
- **Concise-CoT prompting**: "be concise" instruction, ~49% avg reduction reported (GPT-3.5/4
  numbers, not Qwen3-verified — treat as upper bound).
- **SelfBudgeter / BudgetThinker** (2025 papers): fine-tune model to self-predict+comply with
  budget — heavier lift, needs fine-tuning infra, likely out of scope for off-the-shelf Qwen3.
- **Distilled short-CoT variants**: general 2025-2026 trend, but no confirmed official
  short-CoT Qwen3-0.6B/4B release found — would need a dedicated HF look.

**Applicable?** **Yes, most directly** — zero engine work: (a) confirm non-thinking/budgeted
mode is actually used where task allows, (b) trial NOWAIT-style logit-bias filler
suppression, (c) trial Chain-of-Draft/concise-CoT prompting — all inference-time, reversible,
cheap to A/B on your existing bench harness.

## 4. Alternative CPU engines

- **bitnet.cpp** (Microsoft, llama.cpp-based, ternary kernels): 2.37-6.17x x86 speedup but
  **only for natively 1.58-bit-trained models** (3 official models exist). Does not apply to
  post-hoc quantizing Qwen3/MiniCPM — closed, no BitNet-native equivalent for your family.
- **PowerInfer**: activation-sparsity; the one cited number (36 tok/s, Xeon) itself rides on
  BitNet quantization, not generalizable. Sparse-activation gains favor ReLU-sparse models;
  Qwen3/MiniCPM are SwiGLU — weaker sparsity story. **Low payoff for this stack.**
- **OpenVINO / IPEX-LLM**: no 2026 comparative data surfaced this round — gap, see open
  questions. Both Intel-maintained; IPEX-LLM leans on AMX paths you don't have. Worth a
  dedicated follow-up.
- **ik_llama.cpp**: see §1 — most concrete near-term win among alternatives, since it's
  llama.cpp-API-compatible (lower migration cost than re-exporting to OpenVINO/IPEX-LLM).

## 5. IQ-quants on CPU (Ice Lake, no VNNI)

- IQ4_XS vs Q4_K_M (Llama-3.1-8B reference): IQ4_XS smaller (4.46 vs 4.89 bpw), faster at
  generation, **slower at prompt-processing** — split result, no clean win.
- 2025-2026 consensus: IQ-quants still slower on CPU-only due to extra dequant cost; no
  source claims this flipped for non-AMX/non-VNNI AVX-512 in 2026. Consistent with your own
  quant-sweep finding (K-quant vs Q8_0 tradeoff varies by model) — no evidence IQ-quants beat
  that on Ice Lake without VNNI/AMX.
- ik_llama.cpp's IQK kernels (§1) are specifically what closes this dequant-cost gap — but
  fork-specific again.

**Applicable?** Low priority standalone; revisit only if `ik_llama.cpp` (§1) is adopted.

## 6. Other 2025-2026 ideas worth flagging

- **Grammar-constrained decoding for the JSON step** (GBNF, already in llama.cpp, zero new
  dependency): forces the grammar directly, eliminates malformed/retried generations, skips
  syntax deliberation — cheap, mature, orthogonal to §3. **Closest thing to a free win** in
  this whole survey if not already in use.
- **Token-Budget-Aware reasoning** as a general 2026 trend (Redis blog; multiple Q1-Q3 2026
  arxiv papers, e.g. "Reasoning as Compression"/Conditional Information Bottleneck framing) —
  active fast-moving area, expect better tooling within months, revisit ~Q1 2027.
- Speculative/unverified: "mechanistic early-detection of reasoning non-convergence" (arxiv
  2607.21433) — detects stalled CoT to early-stop. No implementation/tooling found, pure
  research paper — flag only.

## Recommendation — if ONE thing next

**Combine §3(a) + §6 grammar-constrained decoding first** (days not weeks; inference-time
only, reversible, targets the confirmed bottleneck directly): verify Qwen3 non-thinking/
budget mode is used correctly per task shape, add NOWAIT-style logit-bias filler suppression,
add GBNF grammar-forcing for JSON output. Lower risk and faster to validate than engine swaps,
and unlike engine-level work (already exhausted per project notes) it targets the actual named
bottleneck. Pursue `ik_llama.cpp` (§1) only as a parallel/follow-up track if CoT-budget work
alone doesn't close the gap — next real engine-level lever untried, but fork-maintenance risk.

## Unresolved questions

- No OpenVINO/IPEX-LLM vs llama.cpp 2026 head-to-head found on non-AMX Xeon — needs follow-up.
- No CPU-specific (vs GPU-only) spec-decoding acceptance-rate numbers exist for <5B
  Qwen3-family target+draft pairs — would need in-house measurement.
- Unclear if a short-CoT-distilled Qwen3-0.6B/4B community variant already exists on HF —
  not checked this pass.
- ik_llama.cpp's Cascade-Lake hand-tune (§1) — unverified transfer to Ice Lake (same
  non-AMX AVX-512 family, author warns against generalizing) — needs your own benchmark.
- b10429 perf-regression (#27111) root cause / Ice-Lake-specificity unresolved in search
  results — check current mainline isn't still affected before pinning a build.
