# OpenJev Tutorial — Understanding Jev, System 1 Models, and Typed-Decision Inference

This tutorial consolidates research, experimentation, and real-world deployment lessons from the `openjev-rs` project — a Rust implementation of Jev-style typed-decision inference engines (SemIf/OpenJev/Laya).

**Purpose**: help users and agents understand how Jev/System 1 models work, their real-world use cases, deployment patterns, and performance-tuning strategies on CPU hardware.

**Who should read this**: engineers deploying LLM-based decision systems, researchers benchmarking typed-decision models, and teams building AI agents with structured output requirements.

---

## Files

| File | Topic | Key takeaway |
|------|-------|---|
| **01-jev-and-system-one-models.md** | Jev/TypeSafe's System 1 model class | "System 1" = fast, calibrated single-pass scoring (not generation) over 2-255 fixed options; ~0.3-1.0s cloud latency, marketed as "40x-200x faster than comparable LLMs" |
| **02-openjev-semif-and-laya.md** | Open-source reproductions (SemIf, Laya, ggmlc) | SemIf = browser-based constrained-readout demo (Qwen3 on WebGPU); Laya = ModernBERT encoder + custom scoring head, achieves 0.8 accuracy on typed-decision benchmarks; both run on CPU |
| **03-real-world-usecases.md** | 14+ documented use cases + 10 benchmark scenarios | Email routing, jailbreak detection, invoice categorization, agent tool routing, incident severity, content moderation — each with sample input/output; derived from production Jev usage and academic benchmarks |
| **04-api-contract-reference.md** | Jev/TypeSafe official API schema | `POST /v1/systemone`, request: `{state, model, questions: {name: {type, instructions, criteria}}}`, response: `{model, answers: {...}, usage: {...}}` — enables drop-in-compatible endpoints |
| **05-benchmark-and-performance.md** | Measured latency & throughput (GPU/CPU/cloud) | Jev cloud: 0.3-1.0s P50; Laya GPU (T4): 25-160ms for 1-10 questions; Laya CPU (32-core): 1.2-1.4s after optimization; generation: 1-15s (intentionally slower, uses full LLM) |
| **06-deployment-best-practices.md** | Real production lessons from openjev-rs deployment | CPU tuning: `n_threads=28` beats 16/32 on a 32-core box; native builds cut constrained-readout ~10-25%; watch model defaults (Laya: threads=4 was a bug); graceful degradation when components fail |
| **07-coding-agent-collaboration-lessons.md** | How coding agents (Claude Code) deployed this system | Always verify self-report with independent QA; always read actual source code, not docs; always check binary defaults; split work into small verifiable loops with real testing |

---

## Quick Start

1. **New to Jev?** Read 01 + 03 to understand the problem space and use cases.
2. **Building a local system?** Read 02 + 04 + 06 for architecture and deployment.
3. **Optimizing for performance?** Read 05 + 06 + 07 for benchmarking methodology and real tuning results.
4. **Using AI agents for implementation?** Read 07 for the discipline that actually caught bugs.

---

## Sources & Recency

All content is sourced from:
- **openjev-rust-research.md** — baseline research on Jev/SemIf architecture (2026-09-22)
- **researcher-01 through researcher-08** — in-depth API/architecture/use-case analysis (2026-09-22)
- **HoH run (hoh-openjev-rs)** — 8-loop real deployment to Linux server with continuous optimization (2026-09-23)
- **Issue ledger, final report, server-tuning-results.md** — cross-verified real production metrics and bug findings (2026-09-23)

Content is accurate as of **2026-09-23**. Jev/Laya/llama.cpp ecosystems move fast; verify specific version numbers and APIs at implementation time.

---

## Glossary

- **System 1 models** (TypeSafe/Jev): trained decision classifiers, score 2-255 fixed options in a single forward pass, return calibrated probabilities + confidence.
- **Constrained readout**: read logits only over valid token IDs (option labels), apply softmax — gives restricted-space probability without decoding the rest of the vocab.
- **Typed-decision question**: structured input with options (`noul`/binary, `choice`/multi-way, `score`/ordered rubric) — not free-text generation.
- **ggmlc**: C++ library (external, not this project's code) that wraps llama.cpp for inference; Laya is built on top of it.
- **Laya**: open-source reproduction of Jev's approach using ModernBERT; achieves similar accuracy, can run on CPU.
- **Constrained decoding**: generate tokens but restrict next-token choices to a valid set (e.g., JSON structure) to avoid invalid output — slower than constrained readout but faster than free-form LLM.

