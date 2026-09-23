---
loop: 9
status: pending
preservation_constraints:
  - "openjev-server and laya serve (systemd-managed) both stay live and reachable throughout — this loop is EXPLORATORY, must not touch or risk the production laya serve path"
  - "All 3 LLM models + Laya still correct and unaffected via apps/cli and apps/server"
  - "cargo build --workspace (debug+release) and cargo test --workspace still pass"
---

## Objective
FEASIBILITY SPIKE (not full implementation): can we hand-roll a Rust+ggml-C native Laya
inference path that's faster than `ggmlc`'s generic compiled runtime? Answer with evidence,
don't commit to full implementation yet if the evidence says no.

**User's explicit directive** (verbatim intent): don't give up, try hard, use Rust + raw C if
needed for speed, goal is to genuinely beat Jev's latency, not just approach it. This loop is
the first concrete step: gather the facts needed to make a good go/no-go call on a full
from-scratch reimplementation, since that's a large, correctness-risky undertaking that
shouldn't be started blind.

## Context already gathered (main agent, do not re-derive)
- **Real modeling source found**: `github.com/NandhaKishorM/laya` (`laya/common.py`) has the
  EXACT PyTorch architecture: tokenize as `[CLS] <type> instructions [SEP] [MASK]-opt0
  [MASK]-opt1 ... [SEP] state [SEP]` (each option gets its own `[MASK]` "marker" token
  position) → `ModernBERT` (or configured HF `AutoModel`) encoder → add a `type_emb` (3-way
  choice/score/noul embedding) to every position → 2 extra `TransformerEncoderLayer`s (`head`,
  `norm_first=True`, `batch_first=True`) processing the FULL sequence with a padding mask → for
  each option, gather the hidden state at that option's marker position (`torch.gather`) →
  `scorer` MLP (`LayerNorm → Linear(d,d) → GELU → Linear(d,1)`) → 1 logit per option → mask
  invalid markers with -1e4 → softmax = final per-option probability distribution. A separate
  `act_head` (pooled `[CLS]` + confidence features → 2-3 classes) decides act/escalate,
  unrelated to which option wins. Full function bodies are in `common.py` — read it directly
  (`https://raw.githubusercontent.com/NandhaKishorM/laya/main/laya/common.py`), don't rely on
  this summary for implementation details.
- **Why this might beat `ggmlc`**: `ggmlc` is a GENERIC compiler ("lowers PyTorch, JAX, Flax,
  Keras... to GGML") — it must handle arbitrary traced graphs, which typically costs dispatch/
  abstraction overhead vs. hand-written code for ONE known architecture. It already uses `ggml`
  (the same tensor/kernel library `llama.cpp` uses), so the CEILING is "at best, ggmlc's own
  kernel speed" if our hand-rolled graph does no better at the op level — the realistic win is
  from graph-level efficiency (avoiding unneeded generality), not from finding faster raw
  kernels than `ggml` provides.
- **Ruled out this session**: graph-reallocation warnings (`ggml_gallocr_needs_realloc`) seen in
  `laya bench` do NOT appear in `laya serve`'s log across real requests — not a per-request
  bottleneck.
- **Already-applied wins** (Loop 6/8, keep as-is): `--threads 28` (was defaulting to 4 — the
  single biggest win, 5-6x), Q8_0 quant (vs UD_Q4_K_M, ~20% win). Current Laya latency:
  ~1.2-1.7s/request (down from ~6.5-8.4s at session start).
- **Prior research ruled OUT `candle`** (CPU backend benchmarked 5-16x slower than ggml-class
  kernels — a candle rewrite risks being SLOWER) — see
  `plans/20260922-2146-openjev-rust-implementation/research/researcher-08-native-laya-rust.md`.
  `ort` requires an unproven ONNX export of this bespoke architecture. The remaining credible
  path is: raw `ggml` C API via Rust FFI, hand-building this exact graph — reusing `ggml`'s
  kernels directly (same performance ceiling as `ggmlc`, but potentially without its generic-
  compiler dispatch overhead).

## Tasks (SPIKE — answer questions with evidence, minimal throwaway code is fine, don't build
production code yet)
1. **Inspect the actual GGUF's tensor names**: on the server, find a way to dump ALL tensor
   names + shapes from `laya_english_q8_0.gguf` (try `laya info <model> --verbose` if such a
   flag exists per `laya help`; if not, `gguf-dump.py` from the llama.cpp repo, or write a tiny
   Python script using the `gguf` pip package, or a tiny Rust program using an existing GGUF-
   parsing crate — whatever's fastest). Map what you find against `common.py`'s module names
   (`encoder.*`, `head.layers.N.*`, `scorer.0/1/3.*`, `type_emb.*`, `act_head.0/2.*`,
   `temperature`) — can you identify which raw tensors correspond to which parts of
   `DecisionModel`? This determines whether hand-rolling a loader is even tractable, or whether
   `ggmlc`'s GGUF is such a fundamentally different graph-serialization format (recall
   `ggmlc-run`'s earlier `info` output showed generic op names like `add_119`, `linear_123` —
   suggesting it may store a TRACED GRAPH, not clean named weights) that this path is a dead
   end and we should instead load the ORIGINAL `.safetensors` from `convaiinnovations/laya`
   directly (bypassing `ggmlc`'s GGUF entirely) and do the format conversion ourselves.
2. **Check for an existing raw `ggml` Rust binding we can reuse**: does `llama-cpp-sys-2`
   (already a dependency) expose the low-level `ggml_*` C functions (tensor creation, matmul,
   graph building) directly, or only `llama.cpp`'s higher-level `llama_*` API? Check
   `~/.cargo/registry/.../llama-cpp-sys-2-*/` bindings — if raw `ggml` symbols are already
   FFI-bound, that's a huge head start (no new C library to vendor/build). If not, check
   whether a standalone `ggml` Rust crate exists on crates.io, or whether vendoring `ggml`'s
   headers directly (it's a small, header-light C library) and writing a minimal `bindgen`-based
   FFI crate ourselves is more practical.
3. **Get `convaiinnovations/laya`'s exact HF config** (`config.json` wasn't found at the naive
   path earlier — check the actual repo file listing via the HF API, it may be nested or the
   repo may gate direct raw access) to confirm encoder hyperparameters (layer count, hidden
   size, attention pattern — ModernBERT alternates local/global attention and uses RoPE, this
   MUST be gotten exactly right or the encoder forward pass will be silently wrong).
4. **Write a tiny throwaway correctness harness**: pick ONE simple test input (e.g. the
   "capital of France" question already used throughout this project), get `laya serve`'s
   current output for it (already have this from earlier: `probs: {A:0.4426, B:0.5574}`,
   confidence 0.0095) as the REFERENCE to validate against later — any future from-scratch
   implementation must reproduce this (within floating-point tolerance) before being trusted,
   not just "run fast."

## Preservation
See frontmatter — this is read-only investigation against the production deployment, don't
modify `laya serve`'s running config or the systemd units.

## Validation Requirements (spike — a clear go/no-go answer counts as success, not a working impl)
- Given the GGUF tensor dump, When compared to `common.py`'s module names, Then this loop
  states clearly: "tractable to hand-load" or "effectively an opaque traced graph, would need
  the original safetensors instead" — with the actual tensor name list as evidence.
- Given the `llama-cpp-sys-2` binding check, Then this loop states clearly whether raw `ggml`
  FFI is already available or needs new bindgen work, with file:line evidence.
- Given `convaiinnovations/laya`'s config, Then encoder hyperparameters (layers, hidden size,
  attention type/pattern, activation) are recorded with a source citation, or explicitly marked
  unavailable if truly not found.

## Out-of-scope
- Actually implementing the full forward pass — that's Loop 10+, gated on this loop's findings.
- Touching the production `laya serve`/`openjev-server` deployment.
- Giving up early if the first approach (reading GGUF tensor names) looks hard — try the
  fallback (original safetensors) before concluding infeasibility.
