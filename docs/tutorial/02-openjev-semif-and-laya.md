# OpenJev, SemIf, and Laya — Open-Source Implementations on CPU

## Overview

Two main open-source projects reproduce Jev's typed-decision inference on commodity hardware (GPU or CPU):

1. **SemIf (Semantic If)** — browser-based proof-of-concept using WebGPU/WASM.
2. **Laya** — server-ready encoder + custom scoring head, can run on CPU.

Both are Apache 2.0 licensed and actively maintained as of 2026.

---

## SemIf (Browser Demo)

**Repository**: github.com/workszop/openjev  
**Live demo**: openjev.com  
**License**: Apache 2.0  
**Tech stack**: Vue 3, wllama (WASM llama.cpp), WebGPU

### What It Does

SemIf measures **two inference paths side-by-side** on the same prompt:
- **Constrained readout**: read logits for option tokens, apply softmax.
- **JSON generation**: decode autoregressive JSON response.

Compare latency and accuracy, e.g., "Readout took 1.02s, generation took 5.33s, both said 'A'."

### Architecture

```
┌─────────────────────────────────────────────┐
│ Browser (User's Machine)                    │
├─────────────────────────────────────────────┤
│ Vue 3 UI                    (index.html)     │
│ ↓                                            │
│ Web Worker              (worker.js)          │
│ ↓                                            │
│ wllama (WASM)           (vendored)           │
│ ↓                                            │
│ llama.cpp (WASM)        (compiled C++)       │
│ ↓                                            │
│ WebGPU backend (or WebGL fallback)          │
│ ↓                                            │
│ GPU (user's machine) OR CPU fallback         │
└─────────────────────────────────────────────┘
```

### Models Tested

- Qwen3-0.6B (Q8_0, ~639 MB) — default.
- MiniCPM4 2B — optional.
- Qwen3 4B — optional (rarely used due to browser memory).

Model binary is fetched from Hugging Face Hub on first load, cached in the browser's IndexedDB, reused across sessions.

### Performance (Reported)

RTX 3090 (GPU):
- Constrained readout (21 binary questions): **~1.02s**
- JSON generation (same payload): **~5.33s**

Note: This is GPU; browser CPU fallback is significantly slower.

### Deployment

- **Static hosting** with COOP/COEP headers (Cloudflare Pages, Netlify).
- **No backend required** (fully client-side).
- Trade-off: user's browser must support WebGPU + have adequate VRAM; mobile/older GPUs will fall back to CPU and be slow.

### Strengths
- Zero infrastructure cost (user-paid compute).
- Privacy: model runs locally, no data leaves the browser.
- Instant feedback for learning/prototyping.

### Weaknesses
- WebGPU support is still rolling out (not all browsers/GPUs supported).
- WebAssembly + WASM-to-native boundary has overhead vs. native Rust/C++.
- Model download baked into first load (1-2 GB for large models).
- No server-side persistence or batch processing.

---

## Laya (CPU/GPU Server)

**Repository**: github.com/thewh1teagle/laya  
**HuggingFace Model**: convaiinnovations/laya  
**License**: Apache 2.0  
**Tech stack**: C++ (ggmlc foundation), ModernBERT encoder, custom scoring head

### What It Does

Laya is a **drop-in reproduction of Jev's speed/accuracy** without requiring TypeSafe's cloud API. It achieves ~0.8 accuracy on Jev-like decision tasks and can run on CPU.

**Key difference from SemIf**: Laya is **not a constrained-readout baseline**. It's a **custom-trained encoder** (ModernBERT-large + 2 transformer layers + option-marker head) that does something architecturally distinct from llama-cpp's constrained logits:

- Each **option is represented as a marked slot** in the input sequence (e.g., `[MASK]` tokens).
- Encoder processes the full context + all option slots in **one forward pass**.
- Scorer reads a **single logit per option position**, not over a vocabulary.
- Result: faster inference than reading logits over vocab (no full softmax).

### Architecture

```
User Input (state) + Options
    ↓
Tokenize + Inject option markers
    ↓
ModernBERT Encoder
    + 2 extra transformer layers
    + Calibration training (RLCD)
    ↓
Option Marker Scorer
    (per-position logit → softmax over options)
    ↓
Output: {option_id, probabilities, confidence, (act/escalate signal)}
```

### Model Details

**Base model**: `convaiinnovations/laya` (421.3M parameters)

| Variant | Size | Format | Best for |
|---------|------|--------|----------|
| `laya` (base/multilingual) | 421 MB (F32) | safetensors | GPU inference |
| `laya` (quantized, Q4_K_M) | ~430 MB | gguf | CPU inference (older default, slower) |
| `laya` (quantized, Q8_0) | 451.5 MB | gguf | CPU inference (faster quant) |
| `laya_english` | — | safetensors | English-only, smaller (research build) |

### Quantization Lesson (from production deployment)

Loop 8 of the HoH deployment (see 06-deployment-best-practices.md) discovered that **K-quants (Q4_K_M) are slow on CPU** for this model:

| Quant | Speed (P50) | Trade-off |
|-------|------------|-----------|
| Q4_K_M (K-quant) | 1.9s | Smaller (430 MB), slower dequant path |
| Q8_0 (simple 8-bit) | 1.5s | Faster (only read, no unpack), 451 MB |
| F16 (half-precision) | 1.5s | Largest (846 MB), same speed as Q8_0 |

**Recommendation**: Use **Q8_0** for CPU deployment (same speed as F16, half the size). K-quants benefit GPU more than CPU.

### Performance

| Hardware | Task | Latency |
|----------|------|---------|
| **Tesla T4 GPU** | 1 question | 32.8 ms |
| **Tesla T4 GPU** | 5 questions | 84.5 ms |
| **Tesla T4 GPU** | 10 questions | 158.6 ms |
| **Intel i3-12100 CPU** (4c/8t, consumer) | 1 question | 112 ms |
| **Ubuntu 32-core Xeon (CPU, after tuning)** | 1 question (Q8_0, threads=28) | 1.2-1.4s |

The 32-core number looks slow until you realize:
- Different processor architecture (consumer 4-core vs. server 32-core); ggmlc's CPU kernels have different scaling characteristics.
- No SIMD optimization per-core is done in ggmlc (it relies on llama.cpp's ggml, which has some, but may not be fully exploited at this model size on this architecture).
- Still **5-8x faster than a full LLM generation** and **within Jev's real P50 range** (300-1000ms).

### Accuracy & Calibration

From Laya's HuggingFace model card:
- **Typed-decision accuracy**: 0.766 (binary classification on TypeSafe-style tasks).
- **ECE (Expected Calibration Error)**: 0.081 (well-calibrated confidence).
- Comparable to Jev's reported accuracy (unquantified in public docs; community benchmarks show Jev at ~0.70-0.78 on similar tasks).

### API & Deployment

Laya ships a **`laya serve`** binary (HTTP server, compatible with Jev's `/v1/systemone` protocol):

```bash
laya serve <model_path.gguf> --port 8090 --device cpu --threads 28
```

Endpoints:
- `POST /v1/systemone` — Jev-compatible endpoint (requests/responses match Jev schema exactly).
- `POST /v1/decide` — Alternative endpoint (same semantics, different field names; less common).
- `GET /health` — Health check.

**Important caveat** (from Loop 5 of HoH deployment): the `serve` binary's default is **`--threads 4`** — on a 32-core machine, this was a bug that went unnoticed until Loop 8 when Laya's latency was benchmarked in isolation. Always verify binary defaults; never assume.

### Deployment in openjev-rs

The openjev-rs project runs Laya as a **persistent HTTP service**, not a CLI tool:

```
┌──────────────────────────────────┐
│ openjev-server (Rust)            │
│ (handles /health, /bench routes) │
├──────────────────────────────────┤
│ 1. Constrained-readout method    │ ← uses llama-cpp-2 (llama-cpp C++ bindings)
│ 2. Generation method              │ ← uses llama-cpp-2
│ 3. Laya method                    │ ← HTTP client to local `laya serve` process
└──────────────────────────────────┘
         ↓ (HTTP localhost:8090)
┌──────────────────────────────────┐
│ laya serve (C++, ggmlc)          │
│ (background process)              │
└──────────────────────────────────┘
```

Rationale:
- Laya is developed/maintained externally (separate repo, C++ codebase).
- Running it as a service avoids shell-out overhead per request.
- Allows independent scaling/tuning of Laya vs. the LLM methods.
- If Laya crashes, the LLM methods continue (graceful degradation).

---

## Comparison: SemIf vs. Laya

| Aspect | SemIf | Laya |
|--------|-------|------|
| **Model type** | Quantized LLM (Qwen3) | Encoder + custom head (ModernBERT) |
| **Inference method** | Constrained logit readout | Position-wise option scoring |
| **Hardware** | Browser GPU (WebGPU) | CPU/GPU (ggmlc) |
| **Typical latency** | 1-5s (browser, GPU) | 30-160ms (GPU), 1-10s (CPU) |
| **Approach** | Reuse existing LLM + score at labels | Train task-specific encoder |
| **Accuracy** | ~0.60-0.75 (on benchmarks) | ~0.77 (on benchmarks) |
| **Deployment** | Static website + user's hardware | Server or local CLI |
| **Calibration** | No (post-hoc in generation path) | Yes (trained via RLCD) |
| **Use case** | Learning, prototyping, privacy-first | Production, SLA-required, multi-tenant |

---

## Why Laya Exists (and Why It Matters)

Jev's real strength is **calibrated confidence** — the model's uncertainty directly reflects decision quality. Off-the-shelf LLMs (even constrained) don't have this without extra fine-tuning.

Laya was created to:
1. Prove the approach can be reproduced in open source.
2. Enable self-hosted deployment (no API dependency, no cost, full control).
3. Optimize for CPU (servers typically don't have GPUs; cloud GPU is expensive).
4. Match or exceed Jev's accuracy on typed-decision tasks.

The trade-off: Laya's accuracy (0.77) is **not better than Jev's** (real-world ~0.75-0.80), just comparable — you don't get extra performance by using open source, but you get independence and control.

---

## Sources

- `docs/research/openjev-rust-research.md` § 2-4 — SemIf architecture, browser deployment, model list.
- `plans/20260922-2146-openjev-rust-implementation/research/researcher-08` — Laya architecture, ModernBERT support, quant analysis.
- `plans/20260922-2328-hoh-openjev-rs/run.md` § "Corrected understanding: Laya's real tool is laya, not ggmlc-run" — deployment architecture.
- `docs/benchmarks/server-tuning-results.md` § Loop 8 — quant comparison and final thread-count configuration.

