# Researcher-08: Native Rust Laya Scorer (candle/ort) — feasibility

## TL;DR
**Feasible in theory, risky in practice for the actual goal (speed).** candle-transformers HAS a `modernbert` module and Laya ships plain `.safetensors`+`config.json` (no GGUF needed), so a candle port is *buildable*. But public CPU benchmarks show candle's own CPU kernels lag ONNX Runtime (`ort`) by ~5x and llama.cpp-family kernels are hand-SIMD-tuned — so swapping `ggmlc` (C++/ggml-style) for candle's default CPU backend has a real chance of landing **at or below current perf**, not above it. `ort` (ONNX Runtime Rust binding) is the safer bet for "faster than ggml on CPU," conditional on being able to export Laya to ONNX — unverified, not yet done by anyone publicly for this model.

## Q1 — candle-transformers ModernBERT support
Confirmed via `docs.rs/candle-transformers/latest/candle_transformers/models/`. BERT-family modules present:
`bert`, `distilbert`, `modernbert`, `jina_bert`, `nomic_bert`, `xlm_roberta`.
Official example: `candle-examples/examples/modernbert` (huggingface/candle repo) — loads `modern-bert-large`, runs fill-mask (`[MASK]` token prediction), CLI: `cargo run --example modernbert --release -- --model modern-bert-large --prompt '...'`. This is masked-LM demo code, **not classification** — would need adaptation for Laya's head (see Q5). No third-party candle+ModernBERT classification example found in the time budget; a higher-level wrapper crate (`ljt019/transformers`, built on candle) claims a `ModernBertSize` sentiment-analysis pipeline builder — unverified maturity/quality, small/unofficial crate, not from HF.

## Q2 — Laya HF repo format
`https://huggingface.co/api/models/convaiinnovations/laya` confirms **standard transformers format**:
- `.safetensors` present (base + multilingual + "typed-decisions" variants), F32/F16 mix, 421.3M params total.
- `config.json` per encoder dir, `tokenizer.json`/tokenizer config present.
- No GGUF files listed. Pipeline tag: `text-classification`.
→ candle (or `transformers`/`optimum` for ONNX export) can load this directly; no reliance on ggmlc's GGUF conversion.

## Q3 — Alternative: ort (ONNX Runtime)
No public ONNX export of `convaiinnovations/laya` found on HF (only safetensors variants listed in the API response). Path would be: HF `optimum-cli export onnx` (or `torch.onnx.export`) on the PyTorch/transformers checkpoint → load in Rust via `ort` crate. This is a **standard, low-risk conversion path in theory** for a vanilla ModernBERT encoder + linear head — ModernBERT is supported by `optimum`/`transformers` ONNX export in principle — but:
- Not verified against Laya's actual modeling code (custom option-marker head, see Q5) — a custom head may not export cleanly if it isn't a standard `AutoModelForSequenceClassification`.
- Nobody has done/published this export for Laya specifically; would be new work, done outside this repo (Python env, `optimum`, correctness validation against the `laya serve` HTTP baseline) before any Rust code is written.
- `ort`'s own CPU perf is strong per Q4 evidence below — this is likely the actual answer to "faster than ggml," not candle.

## Q4 — candle CPU performance maturity (the load-bearing question)
Cargo features exist: `candle-core` has `mkl` (Intel MKL via `intel-mkl-src`) and `accelerate` (Apple Accelerate via `accelerate-src`), gated opt-in, off by default. So a naive `cargo add candle-*` build gets **no BLAS acceleration** unless the feature is explicitly turned on and the target platform matches (mkl = x86 Intel only; accelerate = macOS only — **neither helps a generic Linux/Windows CPU deployment**, which openjev-rs presumably targets; AVX2/AVX512 gemm kernels in candle's pure-Rust CPU backend would carry the load there).

Best real-world CPU number found: `jerrythomas/rust-embedding-bench` (Apple M4 Max, fp32, sentence-transformer-class encoder model — architecturally close to what we care about):

| Backend | single-query p50 | batch=32 short (embeds/sec) |
|---|---|---|
| llama-cpp-2 (Metal) | 1.26 ms | 9,676 |
| ort fp32 | 1.10 ms | 3,052 |
| fastembed | 1.71 ms | 2,849 |
| **candle** | **8.15 ms** | **603** |
| ollama (HTTP) | 11.05 ms | 433 |

candle finished **4th of 5**, ~5x slower than `ort` and ~16x slower than llama.cpp/Metal on batch throughput, on the *same hardware/model*. Author's own conclusion: gap "likely reflects pure-Rust implementation maturity" of candle's CPU kernels vs ONNX Runtime's tuned backend. This benchmark is Mac/Metal-context (not the mkl/x86 case), and doesn't test candle with `accelerate` feature on, so it's not dispositive for a Windows/Linux x86 CPU target — but it's the closest available apples-to-apples signal, and it points the wrong direction for the user's actual goal.

Separately, some other source claims "candle 47% faster than [unspecified baseline] for BERT" — vague, no methodology, contradicts the embedding-bench numbers, low confidence, not reconciled.

**Implication:** `ggmlc`'s underlying kernel is presumably a ggml/llama.cpp-style hand-tuned CPU SIMD implementation (same family as the "llama-cpp-2" row above, which wins the batch-throughput race). If so, rewriting in candle's default CPU path risks **matching or losing to** ggmlc, not beating it — undermining the entire motivation for the rewrite. `ort` is the backend with actual CPU-perf credibility in this data.

## Q5 — Laya's actual classification mechanism (from HF README)
Not a simple "CLS token → linear → softmax." Architecture per Laya's own README:
- Backbone: ModernBERT-large (421M) + **2 extra transformer layers** + an **option-marker scorer** + an **act/escalate head**.
- Mechanism: each answer *option* is scored **at its own `[MASK]`-token position** in the input (options are injected into the sequence as marked slots), giving one logit per option; softmax is taken **across that question's option positions only** (open answer-space, "defined at request time, no retraining for new schemas") — i.e. dynamic-cardinality classification via position-wise scoring, not a fixed N-way head.
- Multiple questions can be answered in one forward pass (batched option slots).
- Context budget: 512 tokens (192 for options) English checkpoint; 1024/256 multilingual.
- Trained via RLCD (proper scoring rules) for calibration; benchmarks: typed-decisions 0.766 acc, AG News 0.950 acc, ECE 0.081.
- Latency reference (GPU, Tesla T4): 32.8–39.5ms single-question, 72–159ms batched-10. **This is GPU, not CPU** — not directly comparable to the CPU-only ggmlc baseline the user is optimizing; no CPU number published by Laya's authors to benchmark against.

This is structurally similar to what the task description calls "constrained logits over labels," but the "option-marker" mechanism (score-at-position-of-each-option's mask token, not a static classifier head) is a **custom architecture piece not present in stock `AutoModelForSequenceClassification`/ModernBERT**. This is the part a candle or ort port must reimplement/re-export correctly — it's the highest-risk, least-verified part of either path (Q1 and Q3 both assume "the encoder loads," but the scoring head is bespoke and undocumented at code level from what's public; README describes behavior, not code).

## Verdict
- **Theoretically feasible**: yes, both candle (modernbert module exists, safetensors loads directly) and ort (needs an ONNX export step first) are viable Rust-side runtimes.
- **Practically the safer/faster choice is likely `ort`, not `candle`** — public CPU benchmark data has candle ~5x slower than ort on a comparable encoder workload, which would make the rewrite pointless or actively regressive versus the current ggml-based `ggmlc`. This is the single most important finding: **"go native Rust" doesn't automatically mean "go fast" — the runtime choice determines that, and candle's default CPU path is the weaker performer of the two candidates.**
- **Biggest unverified risk isn't the runtime, it's the custom option-marker/act-escalate head** — nobody has published a candle or ONNX implementation of Laya's exact scoring mechanism; that has to be reverse-engineered from the README/possibly the model's Python inference code (not checked here — HF repo may have a `modeling_laya.py` or similar; not fetched in this pass) and reimplemented correctly before any speed comparison is meaningful.

## Unresolved questions / unverified assumptions (for follow-up)
1. Does `convaiinnovations/laya`'s HF repo include actual Python modeling code (`modeling_laya.py`/custom `trust_remote_code` module)? Not fetched — needed to know exact tensor shapes/ops for the option-marker head before any Rust port starts.
2. Is `ggmlc`'s current CPU kernel actually AVX2/AVX512-tuned ggml (i.e., is it fair to assume it's near the llama-cpp-2 performance class in the benchmark table above), or is it an unoptimized generic C++ path? Not verified — changes how much headroom `ort` realistically offers.
3. Does candle's CPU perf improve meaningfully with `mkl`(x86)/manually-tuned target-cpu=native flags to close the ~5x gap vs ort? Not tested/found in available benchmarks.
4. Can Laya's option-marker head be expressed as a static ONNX graph (fixed op set) given options are injected at request time with variable count/position? If option count is dynamic, ONNX export may need a fixed max-options padding scheme — not verified against the actual modeling code.
5. No CPU-specific latency numbers exist anywhere (published) for Laya itself — the 8-25x community gap in the task's own bg is the only CPU signal available; no independent confirmation of what "good" CPU latency should look like for this exact model/head.
