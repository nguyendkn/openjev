# Jev and System 1 Models — What They Are and Why They Matter

## Overview

Jev (TypeSafe AI's product line) implements a class of decision models called "System 1" — fast, calibrated classifiers trained to score 2-255 fixed options in a single forward pass, returning probability distributions and confidence estimates.

**Official reference**: TypeSafe's product page (typesafe.ai) and marketing claims; for this document's API details, see 04-api-contract-reference.md.

---

## Core Design: Not a Chatbot, Not General Text Generation

Jev models are fundamentally different from standard LLMs:

| Aspect | Standard LLM | Jev System 1 |
|--------|--------------|-------------|
| **Task** | Generate any text token-by-token | Score a fixed set of options, return probabilities |
| **Input** | Open-ended prompt | Structured state + typed questions with labeled options |
| **Output** | Arbitrary length, unbounded | Probability distribution over provided options |
| **Speed** | Seconds to minutes (depends on output length) | 70-500ms (TypeSafe marketing); real measured 300-1000ms (see 05-benchmark-and-performance.md) |
| **Confidence** | Not inherent (post-hoc calibration needed) | Built-in, trained via proper-scoring rules (RLCD) |
| **Use case** | Creative writing, translation, summarization | Routing, risk scoring, content moderation, agent control flow |

---

## The Two Inference Paths (from SemIf, the open-source predecessor)

Jev's architecture is rooted in SemIf (Semantic If) — a 2-method comparison:

### 1. **Constrained Single-Token Readout** (fast path)
- Load prompt + options into the model, run one forward pass.
- Read logits **only for the tokens representing option labels** (e.g., "A", "B", "C").
- Apply softmax over **just those label tokens**, not the full vocabulary.
- Return: probabilities for each option.
- **Speed**: ~100-500ms (single decode step).
- **Example**: classify an email as `billing`, `technical-support`, or `spam` by reading logits at the tokens for those words.

### 2. **JSON Generation** (slower path, for validation)
- Run an autoregressive generation loop: decode tokens one-by-one, ask the model to output JSON.
- Example: `{"option": "billing", "reasoning": "..."}` over ≤512 tokens.
- Strip `<think>...</think>` reasoning tags if present.
- Parse and validate against the schema.
- **Speed**: seconds (depends on model size, sequence length, hardware).
- **Purpose**: Verify that constrained-readout and generation agree; detect when the model is uncertain or confused.

Both methods should agree on the answer when the model is confident. Large disagreements signal miscalibration or adversarial inputs.

---

## Three Question Primitives (from Jev official API)

All Jev requests use one of three question types, defined in the API contract:

### **`noul`** (Yes/No)
- Binary decision.
- Returns: single probability (0.0 = "no", 1.0 = "yes"), no confidence field needed.
- **Example**: "Does this message express urgency?" → 0.87 (mostly yes).

### **`choice`** (Multi-way)
- 2-255 labeled options.
- Returns: `{choice: "option_id", probabilities: {"opt_a": 0.6, "opt_b": 0.3, ...}, confidence: 0.65}`.
- **Example**: "Route to: billing (0.71), support (0.22), escalate (0.07)" with confidence 0.71 (fairly sure).

### **`score`** (Ordered Rubric, 2-10 levels)
- Ranked severity/quality/risk scale.
- Returns: weighted score (e.g., 3.4 on a 1-5 scale), probabilities per level, confidence.
- **Example**: "Incident severity: sev1 (0.05), sev2 (0.30), sev3 (0.40), sev4 (0.25)" → expected value ≈ sev3.

---

## Performance Claims vs. Real-World Numbers

### TypeSafe's Marketing
- **70-500ms per decision** (source: typesafe.ai/blog).
- **40x-200x faster than comparable LLM** (compared to a baseline LLM, unspecified; likely a large generalist model like GPT-3.5).

### Real Third-Party Benchmarks
Reported in dev.to and sysone-bench repositories (2026, independent of TypeSafe):

| Scenario | Measured P50 | Notes |
|----------|--------------|-------|
| InjecAgent (1,105 samples, prompt injection detection) | 0.30s | Via TypeSafe cloud API |
| BEIR SciFact (900 samples, reranking/relevance) | 0.31s | Via TypeSafe cloud API |
| SkillRetBench (500 samples, tool routing) | 0.33-2.07s | Simple: 0.33s; complex hybrid: 2.07s |
| **Real Jev user observations** | 0.9-1.0s | From sysone-bench project (5-question batch) |

**Interpretation**: Jev's **real cloud latency is 0.3-1.0s P50**, not always the 70-500ms range. Complex scenarios (multiple questions, large context) push toward the higher end. Network RTT to TypeSafe's cloud API is included in these numbers.

---

## Key Architectural Properties

### 1. **Calibration is Trained, Not Post-Hoc**
- Jev models are fine-tuned with proper-scoring rules (RLCD — Rank-consistent Loss for Distribution).
- Confidence scores directly reflect the model's uncertainty — you can use them to gate autonomous decisions.
- **No manual thresholding needed** (though it's still good practice to validate on your own data).

### 2. **Multiple Independent Questions in One Forward Pass**
- A single request can ask 3+ independent yes/no, routing, or severity questions about the same state.
- The model can answer them in parallel without repeating the forward pass.
- **Use case**: classify an email as urgent AND detect jailbreak attempt AND extract the root cause, all at once.

### 3. **No Retraining for New Options**
- The scoring mechanism (both in Jev and Laya) is **dynamic**:
  - Options are supplied at request time.
  - The model scores them without ever having seen those exact labels in training.
  - Enables ad-hoc classification without fine-tuning.

### 4. **Failure Modes Are Known**
- The model **struggles with ambiguous cases** where two options are semantically very similar.
  - Example: differentiating between "Account Access" and "Technical Support" when the user's issue spans both.
  - Confidence will be lower; autonomous decisions should escalate.
- The model **may hallucinate reasoning** if asked to generate JSON (use `<think>` stripping + validation).
- The model **is not creative** — it can't invent new options or rewrite the user's intent.

---

## From Jev to OpenJev to Local Inference

### Jev (TypeSafe, Cloud)
- Closed source (weights/architecture unpublished).
- Multi-model zoo: `jev-latest` (current default), older generation models available.
- Pricing: per-token (similar to standard LLM APIs, ~$0.00004 per decision call per the benchmarks).
- Auth: Bearer token via `Authorization` header.
- API: `POST /v1/systemone` (details in 04-api-contract-reference.md).

### OpenJev (Community, Web-Based Demo)
- **SemIf** (repo: github.com/workszop/openjev, openjev.com) — constrained-readout proof-of-concept in the browser.
- Runs Qwen3-0.6B via wllama (WASM wrapper of llama.cpp) on the user's GPU (WebGPU).
- No server required, fully local, no API key needed.
- Measures both constrained-readout and JSON-generation paths side-by-side.
- **Not production-ready** (browser memory/stability limits, model download per user, no persistence).

### Local Open Reproductions (CPU/Server)
- **Laya** (repo: github.com/thewh1teagle/laya, HF: convaiinnovations/laya) — ModernBERT encoder fine-tuned on Jev-like task distribution.
  - CPU-inference-optimized, achieves ~0.8 accuracy on TypeSafe's benchmarks.
  - Runs on CPU (25-160ms on GPU, 1-8s on CPU depending on hardware; see 05-benchmark-and-performance.md).
  - Open weights (Apache 2.0), easier to integrate into self-hosted systems.
- **JEV-CPU-Gemma4** (repo: github.com/HeapHeapHooray/JEV-CPU-Gemma4) — older, Gemma-based attempt, less maintained.

---

## Why This Matters for Your Use Case

**You want to use Jev/System 1 models if:**
- You need fast decisions (< 1 second latency).
- Your decision space is finite and well-defined (2-255 options).
- You need calibrated confidence to auto-gate or escalate (not true/false classification).
- You want to avoid LLM hallucination (no free-text generation = no made-up facts).
- You want deployment flexibility (cloud API for managed service, local Laya for self-hosted).

**You do NOT want to use Jev if:**
- Your task requires open-ended text generation (summaries, translations, creative writing).
- Your decision tree is deep or requires intermediate reasoning steps (agents may need multi-hop reasoning).
- You have ≪ 1 second latency targets on consumer CPU (generation-based fallback will slow you down).

---

## Sources

- `docs/research/openjev-rust-research.md` § 1-2 — Jev/SemIf overview, browser demo architecture.
- `plans/20260922-2146-openjev-rust-implementation/research/researcher-05` § 1-2 — use cases, benchmark datasets, Jev benchmarks.
- `plans/20260922-2146-openjev-rust-implementation/research/researcher-06` § 1-7 — official Jev API contract + confidence semantics.
- `plans/20260922-2146-openjev-rust-implementation/research/researcher-07` § 1 — real Jev cloud latency from dev.to + sysone-bench.

