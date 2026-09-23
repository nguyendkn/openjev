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
| **Ubuntu 32-core Xeon** (Q8_0, threads=28, **unoptimized `-O0` build**) | 1 question (HTTP `/v1/decide`) | 1.2–1.4s | openjev-rs Loop 8 — *superseded, see below* |
| **Ubuntu 32-core Xeon** (Q8_0, threads=28, **`-O3 -march=native` build**) | 1 question (HTTP `/v1/decide`) | **90–160 ms** | openjev-rs Loop 12 |

### The 11x "Xeon Loses to a Consumer i3" Anomaly — Resolved (Loop 11–12)

Loops 8–10 recorded an unexplained result: a 32-core Xeon Gold 5320 was ~11x *slower* per question
than a 4-core consumer i3-12100. Thread count, quantization format and `GGML_NATIVE`/`-march=native`
had all been checked and ruled out, so the gap was written down as "probably ggmlc's C++
implementation efficiency."

That was wrong, and the real cause was simpler: the `ggmlc` build on the Xeon had been configured
with **no `-DCMAKE_BUILD_TYPE`**, whose empty default means CMake adds **no optimization flags at
all** — an effectively `-O0` binary. `-march=native` was genuinely present (which is why the
earlier flag audit passed), but on its own it only widens the *available* instruction set; without
`-O3` the compiler barely uses it.

Rebuilding the identical source with `-DCMAKE_BUILD_TYPE=Release -DGGML_NATIVE=ON` and swapping the
binary into production:

| Metric (external `curl`, nothing else changed) | Before | After |
|---|---|---|
| `laya_inference_ms` | 1202 / 1267 / 1910 ms | **113 / 113 / 129 / 131 / 158 ms** |
| 10-scenario `/v1/decide` direct | 1.02–1.78 s | **0.090–0.167 s** |

That lands the Xeon at **~90–160ms**, i.e. the **same class as the i3-12100's 112ms** community
anchor — and inside this chapter's own "50–150ms P50" recommended target below. The anomaly was a
build-configuration bug, not an upstream-efficiency finding. See
06-deployment-best-practices.md § Finding 4 for the full diagnosis and the general lesson.

### Side Effect: This Also Recalibrated Our Own Rust Rewrite

Loops 9–11 built `crates/laya-native`, a from-scratch Rust + raw-ggml reimplementation of the Laya
model, and measured it at **95.1 ms** against the then-production C++ at ~1262 ms — apparently a
13x architectural win. Against the *correctly built* C++ (92.7 ms), it is **~3% slower — parity,
not a win**.

This is reported as the current standing, not a conclusion: the native Rust path is still being
optimized (fewer graph nodes, tighter memory behaviour, kernel-level experiments) with the explicit
goal of beating the `-O3` C++ baseline, and it is not wired into production until it does. The
durable lesson is about measurement, not about Rust vs C++: **an impressive-looking speedup is a
claim about two configurations, and you own the burden of proof on both of them.**

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

#### Laya Method (Separate HTTP Service, After Thread Fix + `-O3` Build Fix)

Current production (Loop 12, `ggmlc` built `-O3 -march=native`, 30 requests, `error_count: 0`):

| Model | Mean | Median | Max | Loop 8 mean (`-O0` build) | Speedup |
|-------|------|--------|-----|---------------------------|---------|
| Qwen3-0.6B | **158 ms** | 154 ms | 202 ms | 1244 ms | 7.9x |
| MiniCPM5-2B | **167 ms** | 164 ms | 209 ms | 1157 ms | 6.9x |
| Qwen3-4B | **146 ms** | 142 ms | 217 ms | 1206 ms | 8.3x |

**Note**: Roughly constant across models, as expected — Laya's latency doesn't scale with LLM size; it's a separate encoder, not running the 3 different LLM models.

**Note on the two fixes**: Loop 8 fixed `--threads 4 → 28` (~5.8x). Loop 12 fixed
`CMAKE_BUILD_TYPE="" → Release` (~7-8x). Combined, vs the Loop 7 baseline of 6543–7209 ms:
**~40–49x**. Neither fix was an optimization; both were *misconfigurations*. That ratio — two
config bugs worth 40x, versus every genuine tuning lever (thread sweep, quant choice, `n_batch`)
worth 10–25% combined — is the single most useful number in this chapter.

### Raw Data

Full per-request timing data:
- `docs/benchmarks/native-summary.json` — Loop 7 (native build, all tuning applied, baseline).
- `docs/benchmarks/laya-threads28-summary.json` — Loop 8 (Laya fixed, Q8_0 quant, threads=28 for both).
- `docs/benchmarks/laya-o3-loop12-summary.json` — Loop 12 (`ggmlc` rebuilt `-O3 -march=native`, **current production**).

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

