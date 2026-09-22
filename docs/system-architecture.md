# System Architecture — OpenJev Rust Port

**Purpose:** CPU-only MCQ benchmark tool comparing 3 independent scoring pipelines for LLM-based classification. Measures latency per stage (model load, warmup, tokenization, inference). Single codebase, dual deployment: CLI (`openjev-cli`) + HTTP API (`openjev-server`/axum).

---

## 1. System Overview

```
┌─────────────────────────────────────────────────────────┐
│  Client Layer                                           │
│  ┌──────────────────┐  ┌──────────────────────────────┐ │
│  │ CLI (local)      │  │ HTTP (POST /bench)           │ │
│  └──────────────────┘  └──────────────────────────────┘ │
└────────────────┬──────────────────────────────────────┬─┘
                 │                                        │
    ┌────────────▼─────────────┐  ┌────────────────────▼──┐
    │ openjev-cli binary        │  │ openjev-server (axum) │
    │ (one-shot bench run)      │  │ (async HTTP listener) │
    └────────────┬──────────────┘  └──────────────┬───────┘
                 │                                 │
                 └────────────┬──────────────────┬─┘
                              │                  │
        ┌─────────────────────▼──────────────────▼──────┐
        │  Shared Pipeline Layer                        │
        │  ┌────────────────────────────────────────┐  │
        │  │ Pipeline (3 independent paths)         │  │
        │  │ • Readout   (constrained logits)      │  │
        │  │ • Generate  (JSON greedy 512-token)   │  │
        │  │ • Laya      (external ggmlc-run CLI)  │  │
        │  └────────────────────────────────────────┘  │
        └──────┬──────────────────────┬────────────────┘
               │                      │
    ┌──────────▼────────────┐  ┌──────▼──────────┐
    │ Engine (llama-cpp-2)  │  │ Models (registry)
    │ • GGUF loading        │  │ • Model download │
    │ • LlamaContext        │  │ • hf-hub client  │
    │ • Logits extraction   │  │ • ggmlc-run wrap │
    └──────┬───────────────┘  └──────┬──────────┘
           │                         │
    ┌──────▼──────────────────────────▼──────┐
    │ External Systems                       │
    │ • Hugging Face (model download)        │
    │ • Local filesystem (GGUF cache)        │
    │ • Subprocess: ggmlc-run (Laya path)    │
    └────────────────────────────────────────┘
```

---

## 2. Crate Dependency Graph

| Crate | Responsibility | Depends On |
|-------|---|---|
| **timing** | Wall-clock measurement struct (7 fields). Serializable output shape. | serde |
| **models** | Static registry (4 models), hf-hub download client, ggmlc-run subprocess wrapper, error types. | hf-hub, timing |
| **engine** | GGUF loader (llama-cpp-2), LlamaContext wrapper, warmup, tokenization, logits extraction. | llama-cpp-2, models, timing |
| **pipeline** | 3 independent scoring submodules: `readout`, `generate`, `laya`. Each transforms prompt + options → probabilities. | engine, models, timing |
| **cli** (app) | `openjev-cli` binary: CLI args parser (clap), bench orchestration, report output. | pipeline, engine, models, timing |
| **server** (app) | `openjev-server` binary: axum HTTP listener, `/bench` route handler (shared payload), `/health`. | axum, tokio, pipeline, engine, models, timing |

**Load order (bottom-up):** `timing` → `models` → `engine` → `pipeline` → CLI/server.

---

## 3. Scoring Pipelines (End-to-End)

### 3.1 Constrained Single-Token Readout

**Input:** MCQ prompt + option strings (e.g. `["A", "B", "C"]`).

**Steps:**
1. Tokenize prompt → token IDs.
2. Tokenize each option string (single token expected per option).
3. Forward pass (1×) through llama-cpp-2 engine.
4. Extract logits for last token position.
5. **Restrict logits to option-token indices only.**
6. Apply softmax to restricted set (normalized across valid options only).
7. Argmax → answer label + probabilities.

**Timing capture:** model_load_ms, warmup_ms, tokenize_ms, constrained_readout_ms.

**Output:** `{"answer": "A", "probabilities": {"A": 0.87, "B": 0.10, "C": 0.03}}`.

### 3.2 JSON Generation Path

**Input:** Same prompt + options.

**Steps:**
1. Tokenize prompt → token IDs.
2. Greedy decode loop (max 512 tokens):
   - Sample next token from model logits.
   - Append to output.
   - Stop if EOS or max reached.
3. **Strip `<think>...</think>` blocks from decoded text** (reasoning tags).
4. Parse remaining text as JSON: `{"option": float, ...}`.
5. Validate schema matches provided options.
6. Normalize probabilities (softmax over parsed values).

**Timing capture:** model_load_ms, warmup_ms, tokenize_ms, generation_ms.

**Output:** `{"answer": "A", "probabilities": {"A": 0.75, "B": 0.15, "C": 0.10}}` (parsed + normalized from generated JSON).

### 3.3 Laya Single-Pass Encoder (External Subprocess)

**Input:** Same prompt + options.

**Steps:**
1. Serialize prompt + options to JSON.
2. Spawn subprocess: `ggmlc-run laya-model.ggml < input.json`.
3. Capture stdout (calibrated probabilities).
4. Parse output JSON.
5. Validate schema.

**Timing capture:** laya_model_load_ms (first run), laya_inference_ms (inference only).

**Output:** `{"answer": "B", "probabilities": {...}}` (claimed to be already calibrated by the ModernBERT encoder).

**Critical difference:** Laya is NOT part of the llama-cpp-2 engine. It runs entirely out-of-process, cross-platform (Windows dev, Linux prod). Failure in ggmlc-run subprocess is a timeout/error, not a memory-safety issue.

---

## 4. Request/Response Shape (Shared by CLI & Server)

Both `openjev-cli` and `openjev-server` accept the same payload structure and produce identical output.

### Input Payload

```json
{
  "prompt": "Paris is the capital of...",
  "options": ["France", "Germany", "Spain"],
  "model_id": "Qwen3-0.6B-Q8_0",
  "pipelines": ["readout", "generate", "laya"]
}
```

### Output Payload (Report)

```json
{
  "dataset_id": "mcq_benchmark_20260922",
  "model_id": "Qwen3-0.6B-Q8_0",
  "question": "Paris is the capital of...",
  "options": ["France", "Germany", "Spain"],
  "pipelines": {
    "readout": {
      "answer": "France",
      "probabilities": {"France": 0.92, "Germany": 0.05, "Spain": 0.03},
      "timings": {
        "model_load_ms": 2150,
        "warmup_ms": 45,
        "tokenize_ms": 8,
        "constrained_readout_ms": 120
      }
    },
    "generate": {
      "answer": "France",
      "probabilities": {"France": 0.78, "Germany": 0.12, "Spain": 0.10},
      "timings": {
        "model_load_ms": 2150,
        "warmup_ms": 45,
        "tokenize_ms": 8,
        "generation_ms": 380
      }
    },
    "laya": {
      "answer": "France",
      "probabilities": {"France": 0.85, "Germany": 0.10, "Spain": 0.05},
      "timings": {
        "laya_model_load_ms": 890,
        "laya_inference_ms": 65
      }
    }
  }
}
```

### CLI Interface

```bash
openjev-cli bench \
  --prompt "Paris is the capital of..." \
  --options France Germany Spain \
  --model Qwen3-0.6B-Q8_0 \
  --pipelines readout generate laya \
  --output report.json
```

### Server Interface

```
POST /bench
Content-Type: application/json

(payload as above)

Response: 200 OK (JSON as above) or 400/500 on error
GET /health
Response: 200 OK { "status": "ready", "model": "Qwen3-0.6B-Q8_0", "version": "0.1.0" }
```

---

## 5. External System Integration

### 5.1 Hugging Face Model Download (hf-hub)

**Flow:**
1. `models` crate exposes static registry: 4 model entries (Qwen3-0.6B, MiniCPM-2B, Qwen3-4B, Laya-en).
2. Each entry maps to a HF repo ID + revision (pinned).
3. On first bench run, `hf_hub::api::sync::Api::new()` downloads GGUF to local cache (`~/.cache/huggingface/hub/...`).
4. Subsequent runs reuse cached file (checksum verified).
5. Failure: network timeout, 404, corrupted download → error propagated to client.

### 5.2 ggmlc-run Subprocess (Laya Path)

**Boundary:** Laya pipeline spawns a separate OS process, NOT a Rust library call.

**Interface:**
- Input: serialized JSON (prompt, options) written to child's stdin.
- Output: stdout captured, parsed as JSON probabilities.
- Synchronous call (blocks until subprocess exits).
- Failure modes:
  - Process not found → error.
  - Timeout (default ~30s) → kill child, return error.
  - Invalid JSON output → parse error.
  - Segfault in ggmlc-run → OS signals child death, parent detects exit code ≠0.

**Rationale:** ggmlc-run is a proprietary closed-source binary; Rust bindings don't exist. Subprocess wrapper is the only integration point. Decouples Laya stability from the main Rust binary (segfault in ggmlc doesn't crash openjev-server).

---

## 6. Deployment Topology

### 6.1 Development (Windows)

- **Machine:** Windows 11+ (dev laptop).
- **Build:** Rust (1.70+), MSVC C++17, CMake 3.18+ (llama-cpp-2 native build).
- **Runtime:** Native x86-64 binary, CPU-only (no GPU detection, no CUDA).
- **Model cache:** `%USERPROFILE%/.cache/huggingface/hub/`.
- **Typical latency:** Qwen3-0.6B readout ~100–150ms on single vCPU (not a multicore perf target; development/validation only).

### 6.2 Production (Linux Server)

- **Machine:** Xeon Gold 5320 (32 vCPU, 256 GB RAM), Linux kernel 5.10+.
- **Build:** Rust, GCC C++17, CMake 3.18+.
- **Runtime:** Native x86-64 binary, CPU-only (focus on scalability across cores, not GPU).
- **Model cache:** `/opt/openjev/cache/` (shared mount or local SSD for throughput).
- **Deployment:** Docker container (optional), systemd service (openjev-server as daemon), reverse proxy (nginx) for /bench + /health.
- **Typical throughput:** Readout ~3–5 req/sec per core (32 cores ≈ 96–160 req/sec concurrent, single-threaded per request; tokio async allows interleaving during I/O, though llama-cpp-2 forward passes are CPU-bound and serialize).

### 6.3 Why CPU-Only, v1

1. **Benchmark purity:** GPU (CUDA/HIP) adds deployment complexity, vendor lock-in; CPU comparison baseline is cleaner.
2. **Hardware universality:** CPU-only code runs on any server without GPU dependency.
3. **Scale-out:** Easier to parallelize across distributed CPUs than manage GPU queue.
4. **Phase 7 target:** Real Linux server tuning focuses on CPU cache locality, thread scaling, not GPU optimization.

---

## 7. Key Constraints & Trade-offs

| Aspect | Constraint | Rationale |
|---|---|---|
| **Engine choice** | `llama-cpp-2` bindings (not candle pure-Rust). | Matches original wllama/llama.cpp perf; unresolved kernel optimizations in candle as of 2026-Q3. |
| **Logits extraction** | Restricted softmax (option-token set only) before argmax. | Probabilistic interpretation differs from full-vocab softmax; matches original OpenJev methodology. |
| **Generation limit** | max 512 tokens, greedy (no beam search). | Benchmark purity; beam search adds latency variance. |
| **Laya integration** | Subprocess, not library. | No Rust bindings available for ggmlc-run. Isolation is a feature (segfault containment). |
| **Model pinning** | HF revision hardcoded per model. | Reproducibility; no auto-update. |
| **Cache locality** | No explicit NUMA pinning (Phase 7 optimization candidate). | Linux prod tuning phase; dev doesn't require it. |
| **Warm-up** | Single sanity check ("Paris is capital of..."). | Original methodology; not statistically representative. |

---

## 8. Timing Measurements Breakdown

**Captured at each stage:**

| Stage | When Starts | When Stops | Note |
|---|---|---|---|
| `model_load_ms` | GGUF file open | LlamaContext ready | Network I/O (first run) + GGUF parse. |
| `warmup_ms` | Warmup prompt tokenized | Warmup inference complete | Single forward pass, throws away logits. |
| `tokenize_ms` | MCQ prompt string input | Token IDs ready | tokenizer runtime only. |
| `constrained_readout_ms` | Forward pass start (readout pipeline) | Argmax computed | llama-cpp-2 forward + logits restrict + softmax. |
| `generation_ms` | Greedy decode loop start | JSON parse complete | llama-cpp-2 multipass decode + text processing + JSON parse. |
| `laya_model_load_ms` | ggmlc-run subprocess spawned | Model loaded in subprocess (timing from ggmlc stdout) | Subprocess init + first load. |
| `laya_inference_ms` | Prompt serialized | Output JSON parsed | Subprocess inference only (excludes I/O + parse). |

**Each pipeline runs independently with its own model load.** KV-cache is NOT reused between readout and generate (separate engine instances or explicit reset).

---

## 9. Error Handling Philosophy

| Layer | Failure Mode | Propagation |
|---|---|---|
| **hf-hub download** | Network, 404, corruption. | `models::Error::HubDownloadFailed` → client gets HTTP 500 (server) or CLI error. |
| **llama-cpp-2 load** | Invalid GGUF, OOM, corrupted weights. | `engine::Error::ContextCreateFailed` → propagated up. |
| **llama-cpp-2 inference** | NaN in output, segfault (rare). | Logged, output marked `generation_ms: -1` (sentinel). |
| **ggmlc-run subprocess** | Process not found, timeout, segfault. | `models::Error::LayaSubprocessError` → returned as partial result (readout + generate still succeed). |
| **JSON parse (generation)** | Malformed JSON after think-block strip. | `pipeline::Error::JsonParseError` → output marked invalid, no probabilities. |

---

## 10. Future Optimization Surfaces (Post-Phase 7)

- **KV-cache reuse:** Share context across pipelines (same prompt, different inference modes).
- **Batch inference:** Multiple prompts in parallel (Phase 7 e2e uses single requests; batching is next).
- **GPU fallback:** Conditional CUDA/HIP path if available (v2 stretch goal, not v1).
- **NUMA pinning:** Explicit thread affinity on multi-socket production servers.
- **Quantization trading:** Dynamic Q4 vs Q8 selector per available RAM (currently hardcoded).

---

## Glossary

- **GGUF:** GPU GGML Universal Format, quantized model file (llama.cpp native).
- **llama-cpp-2:** Rust bindings to llama.cpp C library (thin wrapper, exposes raw C API).
- **Logits:** Raw unnormalized scores from model output layer (before softmax).
- **ggmlc-run:** External subprocess binary for ModernBERT (Laya) inference (not open-source).
- **hf-hub:** Hugging Face hub sync client crate (downloads models from HF).
- **Constrained readout:** Logits restricted to valid option tokens before softmax (not full vocabulary).
- **Greedy decode:** One-token-at-a-time generation, always pick max-probability token (no sampling or beam search).

