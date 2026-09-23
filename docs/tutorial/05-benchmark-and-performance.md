# Benchmark and Performance — Real Numbers for Jev, Laya, and LLM Inference

This chapter consolidates real-world latency measurements from production Jev (cloud), published Laya benchmarks (GPU/CPU), and the openjev-rs deployment. Includes honest gaps where performance numbers **don't** exist.

---

## Jev (TypeSafe, Cloud API)

### Marketing Claims

- **70–500 ms per decision** (from typesafe.ai/blog and product pages).
- **40x–200x faster than comparable LLM** (baseline LLM unspecified; likely a large generalist like GPT-3.5).

### Real Third-Party Benchmarks

Reported from independent projects (not TypeSafe's own claims), 2026:

| Source | Scenario | Measured P50 | Notes |
|--------|----------|--------------|-------|
| **dev.to** (aitejiu) | InjecAgent (1,105 calls, jailbreak detection) | **0.30s** | Via TypeSafe cloud API |
| **dev.to** | BEIR SciFact (900 pairs, reranking) | **0.31s** | Via TypeSafe cloud API |
| **dev.to** | SkillRetBench (simple, 1 question) | **0.33s** | Via TypeSafe cloud API |
| **dev.to** | SkillRetBench (hybrid, 10 questions) | **2.07s** | Complex scenario, API call still |
| **dev.to** | General/median figure | **~$0.00004 + 0.3s per call** | Reported cost + latency |
| **sysone-bench** (independent head-to-head Jev vs Laya) | 5-question call, over API | **925–1,068 ms** | Cloud latency includes network RTT |

**Interpretation**: Jev's **real-world P50 is 0.3–1.0s**, not always the 70–500ms marketing range. Complex scenarios (multi-question, larger context) trend toward the higher end. Network round-trip time to TypeSafe's API is included in these numbers.

### What Jev Doesn't Publish (But Should)

- **P95/P99 latency** — no percentile breakdown found anywhere.
- **Latency by question type** (noul vs choice vs score) — no per-type timing exists in public docs.
- **Cold vs. warm latency** — no distinction between first call and subsequent calls.
- **Hardware details** — no official disclosure of Jev's backend (GPU model, region, infrastructure).

**Gap**: Jev/Laya/openjev-rs are the only systems where nobody currently publishes p50/p95/p99 × question-type latency. This is a genuine market gap — first to publish it has a credibility edge.

---

## Laya (Open-Source)

### GPU Performance

From Laya's official HuggingFace BENCHMARKS.md (Tesla T4 GPU, F16+CUDA):

| # Questions | P50 Latency | Throughput |
|-------------|-------------|-----------|
| 1 | 32.8–39.5 ms | — |
| 5 | 40.1–84.5 ms | — |
| 10 | 72.3–158.6 ms | — |
| 50 | 337.4–771.3 ms | — |
| (batched) | — | **103–332 questions/sec** |

The card notes: **"~6–7x faster than Jev"** (comparing T4 Laya vs. Jev's reported 0.23–0.33s; the comparison holds, but see caveats below).

### CPU Performance (Community Benchmarks)

**No official Laya CPU numbers exist** from the Laya project itself. Community runs:

| Hardware | Scenario | Latency | Source |
|----------|----------|---------|--------|
| **Intel i3-12100** (4c/8t, consumer) | 1 question | **112 ms** | pbrehmer-ai/laya-codex-bench |
| **Intel i3-12100** (4c/8t) | 4 questions | **532 ms** | Same repo |
| **Intel i3-12100** (4c/8t) | 12 questions | **2,408 ms** | Same repo |
| **Undisclosed CPU** | 1 question (Laya) | **360 ms** | Shray15/laya-vs-llm-benchmark |
| **Ubuntu 32-core Xeon** (post-tuning, Q8_0, threads=28) | 1 question (HTTP `/v1/decide`) | **1.2–1.4s** | openjev-rs Loop 8 |

**Caveat**: The i3-12100 (4-core consumer) beats the 32-core Xeon by **~11x** on a per-core basis. This gap is **not tuning** (the Xeon had native flags, thread count tuning, quant choice all optimized; see 06-deployment-best-practices.md). The gap likely lives in ggmlc's own C++ implementation efficiency for this specific op set.

### CPU Quant Comparison (Loop 8 of openjev-rs)

After tuning, tested 3 quantizations on the same 32-core CPU:

| Quant | File Size | Mean Latency | Winner? |
|-------|-----------|--------------|---------|
| UD_Q4_K_M (K-quant) | ~430 MB | 1.9s | — |
| Q8_0 (simple 8-bit) | 451.5 MB | **1.5s** | ✓ (faster) |
| F16 (half-precision) | 846.1 MB | 1.5s | (same speed, 2x larger) |

**Finding**: K-quants (Q4_K_M) have a **slower dequant path on CPU** than simple 8-bit (Q8_0). Use Q8_0 for CPU, F16/K-quants for GPU.

---

## LLM-Based Approaches (For Comparison)

### Full Autoregressive LLM Generation (No Constraints)

| Model | Hardware | Latency | Notes |
|-------|----------|---------|-------|
| GPT-3.5 | Cloud API | 2–4s | Typical for a JSON response, ~200 tokens |
| GPT-4 | Cloud API | 5–10s | Larger model |
| Qwen3-4B | Local CPU | **8.5s** (worst case) | From researcher-05 baseline |
| Qwen3-0.6B | Local CPU | ~2–3s | Smaller, faster |

### Constrained Decoding / Structured Output

| Approach | Speed | Notes |
|----------|-------|-------|
| **Outlines** (Python library, structured decoding) | 2–3x slower than unconstrained | Enforces valid JSON at generation time |
| **Jev-style constrained readout** | **100–500 ms** | Single forward pass, no generation |
| **Laya-style encoder scoring** | **30–400 ms** (GPU), **1–8s** (CPU) | Encoder only, not LLM |

**Takeaway**: Jev/Laya are **10–100x faster** than full-LLM generation for the same decision task.

---

## openjev-rs Deployment Results

Real benchmarks from a 32-core Xeon Ubuntu server, after full tuning (Loop 7–8, see 06-deployment-best-practices.md):

### Per-Method Latency (mean across 3 models × 10 scenarios = 30 requests per config)

#### Constrained-Readout Method (fastest)

| Model | Mean | Median | Max |
|-------|------|--------|-----|
| Qwen3-0.6B | **112 ms** | 103 ms | 195 ms |
| MiniCPM5-2B | **179 ms** | 168 ms | 327 ms |
| Qwen3-4B | **291 ms** | 276 ms | 524 ms |

**Verdict**: Faster than Jev's cloud latency (0.3–1.0s P50), especially for small models. Largest model still within Jev's range.

#### Generation Method (slowest, for comparison)

| Model | Mean | Median | Max |
|-------|------|--------|-----|
| Qwen3-0.6B | **4.3s** | 3.5s | 7.3s |
| MiniCPM5-2B | **8.5s** | 9.6s | 13.1s |
| Qwen3-4B | **14.2s** | 15.7s | 20.9s |

**Note**: Generation is **not a Jev equivalent**; it's an intentionally slower validation arm (see 03-real-world-usecases.md § Honest Reframe). Architecturally different from Jev's single-pass scoring.

#### Laya Method (Separate HTTP Service, After Thread Fix)

| Model | Mean | Median | Max |
|-------|------|--------|-----|
| Qwen3-0.6B | **1.2s** | 1.3s | 1.5s |
| MiniCPM5-2B | **1.2s** | 1.2s | 1.4s |
| Qwen3-4B | **1.2s** | 1.2s | 1.4s |

**Note**: Same across models (Laya's latency doesn't scale with LLM size — it's an encoder, not running the 3 different LLM models).

### Raw Data

Full per-request timing data:
- `docs/benchmarks/native-summary.json` — Loop 7 (native build, all tuning applied, baseline).
- `docs/benchmarks/laya-threads28-summary.json` — Loop 8 (Laya fixed, Q8_0 quant, threads=28 for both).

Each file includes: `model_load_ms`, `warmup_ms`, `tokenize_ms`, `constrained_readout_ms`, `generation_ms`, `laya_inference_ms`, `laya_model_load_ms`, `total_requests`, `error_count`.

---

## Latency Targets (For Your Own Implementation)

Proposed benchmarks if you're building a typed-decision system on CPU (from researcher-07 § 4):

| Method | Real anchor | Target on CPU | Rationale |
|--------|-------------|----------------|-----------|
| **Laya-style encoder** | i3-12100 (4c): 112ms; T4 GPU: 32–40ms | **50–150 ms P50** | Expect 2–5x GPU latency on CPU, not parity. 32 cores vs 4 threads gives headroom but isn't magic. |
| **Constrained-readout** (LLM logits) | Jev cloud: 300–1000ms | **150–500 ms P50** | Competitive with Jev's cloud; local CPU won't beat the cloud easily. |
| **Generation** (full decode) | Naive LLM: 8.5s | **1–5s P50** (small models) | Intentionally slower than constrained; still 2–8x faster than naive baseline. |

**Honest advice**: Don't promise "Jev-speed on a $50 CPU box." Jev runs on managed infrastructure (likely GPU-backed cloud). Local CPU will be slower, but reasonable for many use cases.

---

## What's NOT Benchmarked (and Why It Matters)

### Cold vs. Warm Latency

- **Cold**: model load + warmup + first query.
- **Warm**: subsequent queries with cached model.

openjev-rs publishes both (`model_load_ms`, `warmup_ms` in the JSON). Jev's cloud numbers are likely **warm** (model pre-loaded in the service), so a fair CPU comparison is **warm-vs-warm**. If you're benchmarking your own system, always warm up the model first.

### Tail Latency (P95/P99)

- **openjev-rs Loop 7**: Qwen3-4B generation **max 20.9s** vs. median 15.7s — that's a **33% tail latency spike**.
- **Jev**: No p95/p99 published anywhere.

For production systems (SLAs, customer-facing latency), tail latency matters more than median. Collect it; don't ignore it.

### Concurrency / Batching

- openjev-rs benchmarks run **sequential requests** (one at a time).
- Laya's throughput (103–332 q/s) is measured on **batched inference**.
- Real systems often batch multiple requests together.

Your bottleneck may not be per-request latency, but **throughput under load**. Test this separately.

### Memory Footprint

- Not benchmarked here; relevant for resource-constrained deployments (edge, Lambda, etc.).
- Qwen3-0.6B Q8_0: ~640 MB RAM + inference workspace (~200 MB typical for llama.cpp).
- Laya Q8_0: ~450 MB + workspace (~150 MB).

---

## Recommendations for Your Own Benchmarking

1. **Use the same 10 scenarios** from 03-real-world-usecases.md across all configs/models.
2. **Run 3+ times** for each config; report mean/median/p95.
3. **Warm the model first** (don't count the first load in latency figures).
4. **Record per-method timing breakdown** (tokenize, inference, parse), not just wall-clock.
5. **Test concurrency** if your use case involves parallel requests.
6. **Publish p50/p95/p99 × question type** (noul/choice/score) — this is a market gap nobody fills yet.

---

## Sources

- `plans/20260922-2146-openjev-rust-implementation/research/researcher-07-jev-benchmark-target.md` § 1–3 — Jev latency numbers, Laya GPU/CPU, latency targets.
- `docs/benchmarks/server-tuning-results.md` (Loop 6–8) — openjev-rs real numbers, quant comparison, final config.
- `docs/benchmarks/{native,laya-threads28}-{summary.json,raw.jsonl}` — raw per-request data.

