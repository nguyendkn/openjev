# Research: KV cache / LMCache applicability to openjev-rs

Scope: does openjev-rs already have KV caching; can llama.cpp prefix/session caching help; can
LMCache be used; is a custom prefix-cache worth building given the actual benchmark workload.
Read-only research — no files touched in `apps/`/`crates/`. Sources: local source (verified),
docs.rs/GitHub source fetches, web search (2026-09-23).

## (a) Basic KV cache — already present, nothing to add

**Verified in this repo**: `crates/engine/src/lib.rs:279-282`:
```rust
pub fn reset_context(&mut self) {
    self.ctx.clear_kv_cache();
}
```
Doc-comment: "Clears the KV cache for all sequences. MUST be called between independent pipeline
runs sharing this `Engine`... so the second run's positions/attention are not contaminated by the
first run's state." This confirms two things directly from the codebase:
1. `llama-cpp-2`'s `LlamaContext` already maintains a KV cache internally across every
   `ctx.decode()` call within a run (standard transformer incremental-decode behavior — not an
   add-on, it's how autoregressive generation works at all: prompt is decoded once, then each new
   token's forward pass reuses the K/V tensors of all prior positions instead of recomputing them).
2. The project already manages this cache manually (explicit `clear_kv_cache()` between
   `run_readout`/`run_generate`/laya stages) to prevent cross-run contamination in the single-shared
   `Engine` instance (`researcher-11`: single-sequence, single-worker today).

**Conclusion**: no action needed. Within-request token-by-token KV reuse is already there by
construction of `llama-cpp-2`/llama.cpp; the project's own reset-between-stages logic proves the
team already understands and manages the KV-cache lifecycle correctly.

## (b) Cross-request prefix/session KV reuse — exists in llama.cpp/llama-cpp-2, not used here

**Verified** (docs.rs source, `llama-cpp-2` `context::session` module):
- File-backed: `state_save_file(&self, path, tokens) -> Result<(), SaveSessionError>`,
  `state_load_file(&mut self, path, max_tokens) -> Result<Vec<LlamaToken>, LoadSessionError>`
  (mirrors llama.cpp C API `llama_state_save_file`/`llama_state_load_file`; older
  `save_session_file`/`load_session_file` names are deprecated aliases).
- Per-sequence: `state_seq_save_file(&self, path, seq_id, tokens) -> Result<usize, ...>`,
  `state_seq_load_file(&mut self, path, dest_seq_id, max_tokens) -> Result<(Vec<LlamaToken>, usize), ...>`.
- In-memory: `get_state_size`, `copy_state_data`/`set_state_data` (unsafe raw-buffer variants),
  `state_seq_get`/`state_seq_set` (safe, returns opaque `SeqState`).
- So the Rust binding **does** expose the equivalent of llama.cpp CLI's long-standing
  `--prompt-cache` flag / `llama-server`'s `--slot-save-path` + `/slots` save-restore API — the
  underlying mechanism is: hash/compare the new prompt's tokens against a saved token sequence,
  find the longest common prefix, skip re-decoding that prefix, resume from the saved KV state for
  only the divergent tail.
- **Caveat from web research** (github.com/ggml-org/llama.cpp discussions/issues #8947, #10937,
  #15082, #23030): prefix reuse is brittle — needs an exact stable token prefix (any earlier-token
  change forces full reprocessing), session files are large (~1GB per ~2K ctx tokens at default
  f16 state precision), and `--cache-reuse`/session-file behavior has had real regressions
  reported against recent llama.cpp versions.

**Conclusion**: the capability exists and is reachable from `llama-cpp-2` today (not something to
build from scratch) — but see (d) for whether it's worth wiring up for this project.

## (c) LMCache — does NOT apply, confirmed by evidence

**Verified** (LMCache docs, docs.lmcache.ai/developer_guide/integration.html, fetched 2026-09-23):
> supported inference engines are explicitly listed as **"vLLM"**, **"SGLang"**, and **"TRT-LLM
> (coming soon)"**. llama.cpp is not mentioned anywhere in that integration doc.

LMCache itself (github.com/LMCache/LMCache) is a KV-cache-offload layer: it moves computed KV
cache out of GPU VRAM into CPU RAM / local disk / Redis / S3-class storage / RDMA transports
(Mooncake, InfiniStore, NIXL), so a GPU-bound serving engine can reuse KV for repeated/multi-turn
prefixes without re-running the (expensive, GPU-bound) prefill. Its entire value proposition is
GPU-VRAM-scarcity relief — it plugs into vLLM's/SGLang's internal scheduler/paged-attention hooks,
which llama.cpp does not have and doesn't need (llama.cpp already runs KV cache in CPU RAM by
default; there is no VRAM tier to offload from for a CPU-only deployment).

**Conclusion**: **LMCache does not apply to this project.** Confirmed by direct evidence, not
inference — its own docs list only vLLM/SGLang/TRT-LLM as backends, and its architecture (offload
from GPU VRAM to CPU RAM) solves a problem this CPU-only project doesn't have (RAM is already the
first tier, per `researcher-11`: 62GB RAM, models 0.6-2.5GB, headroom is large).

## (d) No Rust/llama.cpp-native LMCache equivalent found; is it worth building anyway — NO

Search for a Rust-native or llama.cpp-native equivalent of LMCache turned up nothing beyond
llama.cpp's own built-in mechanism from (b) — there is no third-party "LMCache for llama.cpp"
project; the built-in session/state-save API in `llama-cpp-2` is already the closest equivalent
and requires no new dependency.

**Whether it's worth wiring up here — verified against this project's actual prompt structure**:
- `crates/pipeline/src/readout.rs:64-68` and `generate.rs:89-95`: each prompt is
  `apply_chat_template(instruction)` where `instruction = "{prompt}\nOptions: {options_list}\n..."`
  — `prompt` is the per-scenario question text, different for essentially every one of the 10
  benchmark scenarios and every option set.
- `engine.apply_chat_template` (`crates/engine/src/lib.rs:167-180`) sends only a single **user**
  message, no system message — the literal shared prefix across every call is just the ChatML
  header `<|im_start|>user\n` (a handful of tokens), not a long system-prompt/instructions block.
- So there is **no long common prefix** across the 10 scenarios to exploit — prefix-KV reuse only
  pays off when many requests share a long stable prefix (e.g. a chatbot's fixed system prompt +
  varying user turns, or multi-turn conversation history). This benchmark tool's requests are
  closer to "10 independent short single-turn prompts," not that shape.
- Where the *same exact* prompt does recur — repeated tuning rounds re-running the same 10 fixed
  scenarios — that is an **exact full-request match**, not a partial-prefix match. A prefix-KV
  cache would still redundantly re-run the tiny non-cached tail and pay session-file I/O overhead
  (verified above: ~1GB/2K-ctx-tokens at f16) for effectively zero net benefit versus simply
  short-circuiting the entire request.
- The simple exact-match response `HashMap` already recommended for the Redis/LangCache question
  (prior research, not independently re-verified this session but consistent with the prompt
  structure confirmed here) dominates prefix-KV-reuse for this exact use case: it skips 100% of
  compute (readout forward pass + generation loop) on a hit, with none of session-file I/O cost,
  brittleness to prefix drift, or extra dependency surface.

**Conclusion**: prefix/session-KV-cache reuse is **not worth building** for this project. The
workload has no long shared prefix to amortize, and the cases where it *would* trivially help
(exact repeat across tuning rounds) are already better served by the simpler full-response cache.

## Summary

| Question | Answer |
|---|---|
| (a) Basic KV cache | Already present via `llama-cpp-2`/llama.cpp internals; project already manages lifecycle correctly (`reset_context`/`clear_kv_cache`). No action. |
| (b) Prefix/session KV reuse | Exists in `llama-cpp-2` (`state_save_file`/`state_load_file`/`state_seq_*`), unused here; capability confirmed, not currently needed. |
| (c) LMCache | **Does not apply** — confirmed vLLM/SGLang/TRT-LLM only, no llama.cpp mention; solves a GPU-VRAM-offload problem this CPU project doesn't have. |
| (d) Build a llama.cpp-native equivalent? | No such library exists; not worth building — benchmark prompts share no long prefix (single user-turn ChatML, no system message), and exact-repeat cases are better handled by the already-recommended simple response `HashMap`. |

## Unresolved / not independently verified this session
1. Exact byte-for-byte function bodies of `state_save_file`/`state_seq_get` in the installed
   `llama-cpp-2` version (`0.1.156`, per `researcher-11`) — signatures confirmed via docs.rs
   source render, not compiled/tested locally.
2. Whether llama.cpp's `--cache-reuse` regressions (GitHub issue #15082) affect the exact
   llama.cpp version vendored by `llama-cpp-sys-2` in this project's `Cargo.lock` — not checked.
3. The prior researcher-09 Redis/LangCache conclusion ("exact-match HashMap sufficient") was
   taken as given per the task brief, not re-derived from that report directly in this session.
