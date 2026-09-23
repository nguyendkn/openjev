---
loop: 10
status: pending
preservation_constraints:
  - "openjev-server and laya serve (systemd) BOTH stay live and unaffected — this loop builds a NEW, separate code path, never touches the production HTTP-based Laya integration"
  - "All 3 LLM models + existing Laya (via laya serve) still correct via apps/cli and apps/server"
  - "cargo build --workspace (debug+release) and cargo test --workspace still pass"
  - "No production traffic routed to the new native path yet — it is validated standalone first"
---

## Objective
Implement + validate the ModernBERT-large encoder forward pass natively in Rust+ggml (28 layers).

Loop 9's feasibility spike returned a strong GO with measured evidence: hand-rolling this
architecture via raw `ggml` FFI (already available through `llama-cpp-sys-2`) has ~8-12x
headroom over the current `ggmlc`-based `laya serve` path, because `ggmlc`'s generic
PyTorch-trace compiler produces ~1364 graph nodes full of memory-bound materialized ops
(slice/transpose/cat/neg for things like RoPE's `rotate_half`) where a hand-written graph needs
far fewer, fused ggml ops (`ggml_rope_ext`, `ggml_soft_max_ext`, etc.) doing the same math.

This loop builds ONLY the encoder backbone (28-layer ModernBERT-large) as a new, isolated
component — NOT wired into production yet. Correctness is the gate, not speed, this loop:
a fast-but-wrong encoder is worthless. Loop 11 will add the custom head (type_emb + 2 extra
transformer layers + scorer + act_head) and wire the full pipeline; Loop 12 will do the actual
production cutover, gated on Loop 11's full-pipeline correctness match.

## Architecture spec (from Loop 9's research — read the cited sources directly, don't rely on
this summary for exact op ordering)
- Tokenize per `common.py`'s `build_sequence`: `[CLS] <type> instructions [SEP] [MASK]-opt0
  [MASK]-opt1 ... [SEP] state [SEP]`, GPT-2 BPE tokenizer (50368 vocab + 50009 merges — baked
  into the GGUF's own metadata, no external tokenizer file needed. Special token ids:
  mask=50284, cls=50281, sep=50282, pad=50283 — VERIFY these against the actual GGUF metadata
  yourself, don't trust this without re-checking).
- Encoder: ModernBERT-large, 28 layers, hidden=1024, 16 heads (head_dim=64), GeGLU
  intermediate=2624 (fused `mlp_wi` weight is [1024,5248] = 2×2624), activation=gelu,
  `norm_eps=1e-5`, `norm_bias=false`, `attention_bias=false`, `mlp_bias=false`.
- Attention pattern: `global_attn_every_n_layers=3` — layers 0,3,6,9,12,15,18,21,24,27 use FULL
  attention with RoPE θ=160000; the other 18 layers use LOCAL/sliding-window(128) attention
  with RoPE θ=10000. GET THIS EXACTLY RIGHT — mixing these up silently produces wrong (but
  plausible-looking) outputs, the single highest-risk correctness trap per Loop 9.
- **Critical, unverified-numerically finding from Loop 9 (verify FIRST before writing the
  encoder)**: per-layer LayerNorm weights are ABSENT from the GGUF tensor list (only
  `emb_norm_w`/`final_norm_w` exist) — Loop 9 hypothesizes `ggmlc` folded each norm's γ into the
  following Linear layer (`norm_bias:false` makes this mathematically valid: LayerNorm without
  affine, then a Linear whose weight absorbed the γ scale). Confirm this by inspecting
  `ggmlc.graph_spec`'s per-layer `layer_norm` node `inputs` count (1 input = no-affine-norm
  fold happened; 2+ = there's a weight tensor being passed that Loop 9's tensor-name matching
  missed) — this JSON is at `/tmp/spike-graphspec.json` on the server (Loop 9 left it there) or
  re-extract from the GGUF's `ggmlc.graph_spec` KV. Get this right before writing a single
  ggml_norm call — a wrong assumption here breaks EVERY layer silently.
- Also resolve Loop 9's Gap #2 (32 unexplained `relu` nodes — ModernBERT has no ReLU in its
  standard architecture, so figure out what these correspond to before assuming they're safe to
  omit).

## Tasks
1. **Set up the crate**: new `crates/laya-native` (lib) — depends on `llama-cpp-sys-2` (for the
   raw `ggml_*`/`gguf_*` FFI, already available) + `timing`. Do NOT wire it into
   `crates/models`/`crates/pipeline`/`apps/*` yet — this loop's output is a standalone,
   independently-testable library + a throwaway test binary, not a production integration.
2. **Resolve the LayerNorm-fold and relu-node questions** (see above) using `ggmlc.graph_spec`'s
   node list as ground truth before writing encoder code.
3. **GGUF loading**: parse `laya_english_q8_0.gguf` via `gguf_init_from_file`, extract all 153
   tensors + the 29 KV metadata entries (special token ids, RoPE thetas, layer_types array,
   etc.) into a Rust struct. Verify tensor shapes/types match Loop 9's inventory exactly before
   proceeding (a shape mismatch here means a stale/wrong assumption, not a subtle bug — fail
   loud).
4. **Build the tokenizer + sequence construction** (pure Rust, no ggml needed): port
   `build_sequence`/`render_options` from `common.py` faithfully. This is the highest-value,
   lowest-risk part to get exactly right first since it's pure logic, unit-testable without any
   ggml graph involved.
5. **Build the 28-layer encoder ggml graph, ONCE, reused across requests** (per Loop 9's own
   recommendation — build the graph once, only update input tensor DATA per request, don't
   rebuild the graph structure every call — this is itself part of the expected speedup vs.
   `ggmlc`'s apparent per-request graph handling). Alternating global/local attention per layer,
   correct RoPE theta per layer, GeGLU MLP, residuals, correct norm handling (per Task 2's
   finding).
6. **Validate the encoder in isolation**: since `last_hidden_state` isn't directly exposed by
   `laya serve`'s HTTP API, validate via the closest available signal — either (a) find a way to
   get `ggmlc-run`'s own intermediate tensor dump for the same input (check `ggmlc-run`'s
   `--output` flag or similar debug capability), or (b) if no direct intermediate-tensor
   comparison is available, proceed to a MINIMAL version of the head+scorer (Loop 11's job, but
   a tiny inline version here is acceptable if it's the only way to get an end-to-end
   correctness signal this loop) and compare final probabilities against the reference
   (A=0.414, B=0.586, confidence=0.0215) — document honestly which validation method was used
   and its limitations.
7. **Benchmark the validated encoder**: real forward-pass timing (not the garbage-weights cost
   model from Loop 9 — real loaded weights, real correctness-checked graph), compare against
   Loop 9's cost-model prediction (117-720ms depending on sequence length) and against current
   production (`laya serve`, 1350-8500ms) — confirm the speedup is real, not just theoretical.

## Preservation
See frontmatter — this is new, isolated code. Don't touch `apps/server`, `apps/cli`,
`crates/models::laya`, `crates/pipeline::laya`, or the running `laya serve`/`openjev-server`
systemd services.

## Validation Requirements
- Given the LayerNorm-fold question, When resolved via `graph_spec` evidence, Then the answer
  is stated with the specific evidence (node input counts), not assumed.
- Given the built encoder + tokenizer, When run on "The capital of France is: A) London B)
  Paris" / options A,B, Then the full pipeline (even with a minimal/temporary head+scorer)
  produces a result in the same ballpark as the reference (B favored, not A) — exact probability
  match isn't required THIS loop (that's Loop 11's full-pipeline gate), but the correct option
  winning is the minimum bar to claim "the encoder is plausibly correct."
- Given a timed real forward pass, When compared to Loop 9's cost-model numbers, Then the
  actual measured time is reported honestly (whether it matches, exceeds, or falls short of the
  117-720ms prediction) — do not round up a worse-than-predicted number to match the pitch.

## Out-of-scope
- The custom head (type_emb, 2 extra transformer layers, scorer, act_head) — Loop 11, unless a
  minimal inline version is needed for Task 6's end-to-end signal (in which case, clearly mark
  it as throwaway/temporary, not the real Loop 11 implementation).
- Wiring into `apps/cli`/`apps/server`/production — Loop 12, gated on Loop 11.
- Batching multiple questions/options beyond what's needed for the single reference test case.
