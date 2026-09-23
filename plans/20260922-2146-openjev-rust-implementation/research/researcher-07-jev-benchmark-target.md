# Research: Jev/Laya Benchmark Numbers — Latency Target for openjev-rs

Scope: pin down the most precise latency numbers found for Jev (cloud) and Laya (open, local) to
set a CPU latency target for `openjev-rs`'s 3 methods (readout / generate / laya-style encoder). No
code touched. Builds on `researcher-05` and `researcher-06` — not repeated here.

## 1. Jev (TypeSafe cloud API) — latency numbers found

| Source | Metric | Value | Baseline compared | Hardware |
|---|---|---|---|---|
| dev.to benchmark (aitejiu, ~22.5k calls/10 datasets) | InjecAgent (1,105 calls) P50 | **0.30s** | — (no LLM latency baseline given, only accuracy: ChatGPT 69.1%, GPT-4o-mini 67.3%, GPT-5.4-mini 66.0% Hit@1 on MetaTool/SkillRouter tasks) | not disclosed (Jev = cloud API, opaque) |
| same | BEIR SciFact (900 pairs) P50 | **0.31s** | — | — |
| same | SkillRetBench hybrid (500 q) P50 | **2.07s** | — | — |
| same | SkillRetBench production (BM25+Jev) P50 | **0.33s** | — | — |
| same | general/call | "~$0.00004 and 0.3s per call" | — | — |
| refix.ai (jev-pricing-latency-benchmarks) | parallel-questions cookbook | batched 13-Q call "12.2x cheaper, 10x faster" vs 13 separate calls | separate single-Q calls | — |
| refix.ai | explicit caveat | "first-party results... not a substitute for testing"; recommends measuring own p50/p95/p99 | — | — |
| WebSearch synthesis (unverified, low confidence — not traced to one primary doc) | "P50 ~0.23s" general figure; a separate claim of "P50 11-15ms against TypeSafe production cluster" | inconsistent w/ all other sources by ~20x — **flag as likely search-summarizer artifact/wrong context, do not trust** | — | — |
| sysone-bench (instax-dutta, independent Laya-vs-Jev head-to-head, byte-identical inputs) | Jev, 5-question call, over API | **925–1,068 ms** | Laya same 5-Q call: **180–660 ms local (M2 CPU)** | Jev=cloud API; Laya=Apple M2 CPU |
| Prior research (researcher-05/06, TypeSafe marketing) | claimed range | 70–500 ms/decision; "40x–200x faster than comparable LLM"; example 0.114s vs 8.566s | unnamed "comparable-intelligence" LLM | undisclosed (near-certainly GPU cloud) |

**Read:** Jev's own cloud numbers cluster **0.3–1.0s per call** in every independently-run
third-party benchmark (dev.to, sysone-bench), consistent with TypeSafe's own 70–500ms claim only at
the low end — actual measured P50s under real multi-question or harder-retrieval workloads (e.g.
SkillRetBench hybrid 2.07s) run well above TypeSafe's marketed ceiling. No p95/p99 or noul/choice/score-specific
breakdown was found anywhere (not in dev.to, not in docs.typesafe.ai per researcher-06 — confirmed
absent, not just missed this pass). No official CPU number exists for Jev — it is a cloud API,
presumably GPU-backed, and TypeSafe has never published deployment hardware.

## 2. Laya (open reproduction) — latency numbers found, GPU and CPU

| Source | Hardware | Metric | Value |
|---|---|---|---|
| Prior research (researcher-05, Laya HF card) | RTX 4050, F16+CUDA graph | single decision | ~25 ms |
| same | RTX 4050 | 7-question preset | ~143 ms |
| NandhaKishorM/laya BENCHMARKS.md | **Tesla T4 GPU** | 1 question (laya-multilingual / laya) | 32.8 ms / 39.5 ms |
| same | Tesla T4 | 5 questions | 40.1 ms / 84.5 ms |
| same | Tesla T4 | 10 questions | 72.3 ms / 158.6 ms |
| same | Tesla T4 | 50 questions | 337.4 ms / 771.3 ms |
| same | Tesla T4 | throughput | 103–332 questions/sec batched; "~6–7x faster than Jev" (Jev baseline used: 236–276 ms/question) |
| same | **CPU** | 51-language CPU sweep | referenced (`research/results/cpu_51_language_sweep.json`) but **no numbers exposed** in fetched doc |
| **pbrehmer-ai/laya-codex-bench** | **Intel i3-12100 CPU (4c/8t, consumer-grade)** | 1 / 4 / 12-question probes | **112 ms / 532 ms / 2,408 ms** |
| same | i3-12100 CPU | Laya-only case (cascade pilot) median | **422 ms** e2e |
| same | i3-12100 CPU | cascade (Laya+Codex fallback) median / mean | 4,124 ms / 3,221 ms (blended, not pure-CPU-Laya) |
| same | — | explicit note | i3 "not comparable to upstream Tesla T4 latency figures" |
| **Shray15/laya-vs-llm-benchmark** | **Laya on CPU** (model/cores undisclosed) vs qwen3:4b on GPU | single routing-decision latency | **Laya 360 ms (CPU)** vs **qwen3:4b 10,901 ms (GPU)** — ~30x |
| same | — | tool-execution latency (post-decision) | ~70 ms both |
| same | — | cold start | Laya 25.9s vs qwen3:4b 14.4s (Laya cold start *higher* — model-load overhead, not inference) |

**Read:** two independent CPU data points now exist for Laya-family models (both unofficial,
community-run, not TypeSafe/Laya's own release): **~112–360 ms for a single question on a 4-core
consumer CPU**, scaling to ~2.4s for 12 questions on the same box. That is roughly **5–15x slower
than the RTX 4050/T4 GPU numbers** (25–40ms/question) — consistent with "CPU vs entry GPU" being a
5–15x gap for this model class, not 100x+.

## 3. Percentile / cold-warm breakdown (item 4 of task)

Not found anywhere, confirmed on this pass: no p50/p95/p99 split by question type (noul/choice/score)
in docs.typesafe.ai (already checked in researcher-06 — absent from OpenAPI/quickstart), and no
cold-vs-warm split in any of the 6 sources fetched this pass except the informal "cold start" number
in Shray15's LLM comparison (25.9s Laya cold start — likely one-time model load, not decision
latency) and laya-codex-bench's "CPU idle 0%" note (service always warm, no cold-start data given).
**Conclusion: Jev/Laya do not publish percentile-by-type latency. This is a real gap**, not a
research miss — flag it as an item `openjev-rs`'s own benchmark harness should fill (i.e., be the
first to publish p50/p95/p99 × noul/choice/score, which none of the three "competitors" currently do).

## 4. Proposed latency targets for `openjev-rs` on CPU (32-core)

No official Jev/Laya CPU benchmark exists at 32-core scale — extrapolating from 1–4-core numbers
above, not measured fact. Treat as **order-of-magnitude targets**, not committed SLAs.

| Method | Nearest real anchor | Target on 32-core CPU (single decision, warm) | Rationale |
|---|---|---|---|
| **Laya-style single-pass encoder** | i3-12100 (4c) 112ms/Q; Shray15 CPU 360ms/Q; T4 GPU 32–40ms/Q | **50–150 ms P50** | Same model class as the two CPU anchors, but 32 cores vs 4–8 threads gives real parallelism headroom (batching/threading), quantized (Q4/Q8) small encoder. Being *faster* than the 112–360ms consumer-CPU anchors is a fair ask given 4–8x more cores; matching the 25–40ms GPU number is not — accept 2–5x slower than GPU as the honest floor. |
| **LLM constrained-readout** (single forward pass, logit read on option tokens, no decode loop) | Sits between encoder and full-generation; no direct third-party number found for this exact technique | **150–500 ms P50** | Should track the Jev cloud API's own 300ms–1s cluster (§1) since architecturally closest to Jev's approach (typed answer, no free text) — but on local CPU vs Jev's (likely GPU) cloud backend, aim for "same order of magnitude," not parity. |
| **LLM JSON-generation** (full decode of a JSON object) | Prior research's own "8.566s" LLM baseline (researcher-05); SkillRetBench hybrid Jev P50 2.07s as an upper anchor for "hard" cases | **1–5 s P50** on a small (≤8B, quantized) local model | Decode-loop overhead is the dominant cost and doesn't disappear with more cores the way encoder-only forward passes do; realistic CPU target is "keep it under Jev's own worst-case (2.07s) for simple 1–2 field JSON," not match the sub-second cases. |

**Bottom line recommendation:** state the target as *relative*, not absolute — "readout and laya
methods within 2–5x of Jev's/Laya's own best disclosed numbers despite running on CPU instead of
GPU/cloud; generate method allowed to be the slowest of the three by design (it's the one paying
the full LLM-decode tax) but should still beat the ~8.5s naive-LLM baseline by using constrained
decoding." Do not promise absolute ms numbers publicly until openjev-rs's own harness has run and
produced first-party p50/p95/p99 — which would also close the gap noted in §3 (nobody in this space
currently publishes percentile-by-question-type data).

## Unresolved questions

1. Jev's own hardware (GPU model, region) is undisclosed — the 0.3–1s cluster from dev.to/sysone-bench
   could include network RTT to a remote API, inflating it vs a pure-inference number. No way to
   separate network vs compute time from published sources.
2. Laya's CPU numbers (i3-12100, Shray15's undisclosed CPU) are both from small, unofficial
   community repos, not from TypeSafe/Laya's own release — treat as directional, not authoritative.
3. No source gives 32-core (or any high-core-count server CPU) numbers for either Jev or Laya —
   the "50–150ms" target in §4 is an extrapolation from 4–8-thread consumer hardware, not measured.
4. "P50 11–15ms" and "P50 0.23s" claims surfaced by one WebSearch synthesis are inconsistent with
   every directly-fetched source by ~20x and were not traced to a primary document — excluded from
   the table, flagged as likely wrong, do not cite.
