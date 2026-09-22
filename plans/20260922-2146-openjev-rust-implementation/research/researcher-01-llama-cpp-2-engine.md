# Research: `llama-cpp-2` engine for OpenJev-rs

Scope: API shape (load/decode/logits), greedy multi-token gen, Windows build, tokenizer/chat-template.
Sources fetched via docs.rs/crates.io/GitHub raw source + web search (2026-09-22). docs.rs rendered
pages returned only nav chrome to the fetch tool (JS-heavy site); source-view pages worked better.

## 1. Crate shape / loading / logits

**Verified** (from crate module docs, docs.rs/llama-cpp-2):
- Modules: `model` (wraps `llama_model`), `context` (wraps `llama_context`), `llama_batch` (wraps
  `llama_batch`), `sampling` (wraps `llama_sampler`), `token` (wraps `llama_token_data` /
  `llama_token_data_array`).
- Error enums: `DecodeError`, `LlamaModelLoadError`, `StringToTokenError`, `TokenToStringError`.
- Tokenizer: `LlamaModel::str_to_token(&self, text, AddBos)` and a `token_to_str` equivalent exist
  (confirmed via `examples/simple`, see below).

**Verified via `examples/simple/src/main.rs`** (github.com/utilityai/llama-cpp-rs, raw source):
```rust
let backend = LlamaBackend::init()?;
let model = LlamaModel::load_from_file(&backend, model_path, &model_params)?;
// LlamaContextParams default n_ctx = 2048
let mut ctx = model.new_context(&backend, ctx_params)?;
let tokens_list = model.str_to_token(&prompt, AddBos::Always)?;

let mut batch = LlamaBatch::new(512, 1);
for (i, token) in (0_i32..).zip(tokens_list.into_iter()) {
    let is_last = i == last_index;
    batch.add(token, i, &[0], is_last)?;   // logits only requested for last=true position
}
ctx.decode(&mut batch)?;
```
This confirms `LlamaBatch::add(token, pos, seq_ids, logits: bool)` — the `logits` bool per-token is
exactly the mechanism to request logits only at the last prompt token, which is what constrained
single-token readout needs (decode the prompt once, read logits at that one position, restrict to
candidate ids).

**Inferred, not directly confirmed in this session** (docs.rs API pages did not render for the
fetch tool; based on crate structure + `examples/simple` calling into a sampler that consumes
context state):
- `LlamaContext::get_logits(&self) -> &[f32]` and `get_logits_ith(&self, i: i32) -> &[f32]` likely
  exist (standard in llama.cpp bindings) to fetch the raw logit vector for a given batch position.
  For constrained readout you'd take this full-vocab `&[f32]`, index the handful of candidate token
  ids yourself, then softmax over just those — the crate does not appear to expose a "restricted
  softmax over N ids" helper; that step is app-side.
- `token::LlamaTokenDataArray` (wrapping `llama_token_data_array`) is the type samplers operate on;
  it can likely be constructed from a logits slice for manual top-k/softmax work if needed instead
  of the built-in sampler chain.
- KV-cache management methods (`kv_cache_clear`, `kv_cache_seq_rm`, `kv_cache_seq_cp`) were not
  independently confirmed this session (page fetch returned only nav chrome); presence is likely
  given they mirror llama.cpp C API, but exact Rust signatures are unverified.

## 2. Greedy multi-token generation

**Verified** (from `examples/simple/src/main.rs`):
```rust
let mut sampler = LlamaSampler::chain_simple([
    LlamaSampler::dist(seed.unwrap_or(1234)),
    LlamaSampler::greedy(),
]);

loop {
    let token = sampler.sample(&ctx, batch.n_tokens() - 1);
    if model.is_eog_token(token) { break; }
    // ...emit token...
    batch.clear();
    batch.add(token, n_cur, &[0], true)?;
    ctx.decode(&mut batch)?;
    n_cur += 1;
    if n_cur >= n_len { break; }   // max_tokens stop condition
}
```
- Stopping at `max_tokens=512` is a simple loop counter (`n_cur >= n_len`), same pattern as OpenJev
  needs; `<think>...</think>` prefix from Qwen3 is just ordinary generated tokens — no special
  handling needed in the engine, only in the app's parsing of the final decoded string.
- KV cache reuse across the batch: the pattern above decodes the full prompt once, then decodes
  *one new token at a time*, incrementing `n_cur` as position — this is standard incremental decode;
  the context's internal KV cache persists across `ctx.decode()` calls within the same context (no
  explicit "reuse" API call needed for the simple single-sequence case).
- `LlamaSampler::greedy()` is confirmed to exist as a chain element for pure argmax decoding —
  directly usable for OpenJev's "greedy" generation requirement.

## 3. Windows build requirements

**Verified** (from `llama-cpp-sys-2` `build.rs` source, docs.rs source view):
- Build orchestration: `cmake` crate drives the actual llama.cpp CMake build; `cc` crate compiles
  wrapper C++ (`wrapper_common.cpp`, `mtmd`) directly; `bindgen` generates FFI bindings (needs
  libclang available).
- **Required on Windows**: CMake, a C++17-capable compiler (MSVC via Build Tools/Visual Studio),
  and clang/libclang for bindgen.
- Cargo features found in build.rs: `cuda`, `rocm`, `vulkan` (needs `VULKAN_SDK` env var), `opencl`,
  `mkl` (x86_64 only, needs `MKLROOT`), `openmp` (`static-openmp` for static `libgomp.a`),
  `dynamic-link` (build `.dll`/`.so`/`.dylib` instead of static `.a`), `common`, `mtmd`. No explicit
  "cpu-only" feature name was found — CPU-only is presumably just the default with no GPU feature
  flags enabled.
- MSVC-specific handling: detects `*-windows-msvc` target triple, extracts MSVC include paths via
  `cc` crate env probing; `LLAMA_STATIC_CRT` env var toggles static vs dynamic CRT (`libcmtd` vs
  `msvcrtd` in debug); for MSVC Release-under-Rust-debug builds it injects `/O2 /DNDEBUG /Ob2`; sets
  `TrackFileAccess=false` and `/FS` to work around MSBuild FileTracker long-path issues.
- When `dynamic-link` is enabled, the script hard-links produced `.dll`s into `target/`,
  `target/examples/`, `target/deps/` so the built binary can find them at runtime — implies that
  with default (static) linking, no separate DLL-copy step is needed, but with `dynamic-link` (often
  paired with CUDA on Windows) you must ensure the DLL is discoverable.
- CUDA feature: Windows uses **dynamic** linking (`cudart`, `cublas`), Linux uses static — i.e. CUDA
  on Windows implies a runtime DLL dependency even if you don't explicitly request `dynamic-link`.
- Vulkan on Windows requires `VULKAN_SDK` env var set (standard Vulkan SDK install); no vcpkg
  dependency found in build.rs — this crate builds llama.cpp itself via CMake+cc, it does not shell
  out to vcpkg.

**Not independently verified this session**: an exhaustive list of every cmake cache variable passed
(`build.rs` was summarized, not read verbatim in full); whether a `metal` feature exists (macOS-only,
likely irrelevant for this Windows-targeted project so not chased further).

## 4. Tokenizer / chat template

**Verified**: `LlamaModel::str_to_token(&self, text: &str, add_bos: AddBos) -> Result<Vec<LlamaToken>, StringToTokenError>` exists and is used directly in `examples/simple` — no separate tokenizer crate is needed for basic prompt tokenization; the crate wraps llama.cpp's own tokenizer (which reads the GGUF's embedded vocab/merges, so it will correctly tokenize per-model, e.g. Qwen3's BPE vocab).
A `token_to_str`-style decode method is referenced in crate docs (module `model`) but exact signature wasn't independently confirmed this session.

**Unverified / needs follow-up**: whether `llama-cpp-2` exposes a wrapper around llama.cpp's `llama_chat_apply_template` (the C API function that renders ChatML/Jinja-style chat templates embedded in GGUF metadata). This session found no direct evidence either way — crate module list (`model`, `context`, `llama_batch`, `sampling`, `token`) doesn't obviously suggest a `chat_template` module, but that doesn't rule out a method on `LlamaModel`. **Practical fallback if absent**: OpenJev can hand-build the ChatML prompt string for Qwen3 (`<|im_start|>system...<|im_end|>\n<|im_start|>user...<|im_end|>\n<|im_start|>assistant\n`) and pass it straight to `str_to_token`, avoiding a dependency on the template API entirely. Given OpenJev only targets Qwen3/MiniCPM4 (small, known set of chat formats), hardcoding the template is likely simpler than relying on a possibly-absent binding.

## Unresolved questions

1. Exact signature/existence of `LlamaContext::get_logits_ith` (or equivalent) for per-position logits — needs a direct docs.rs API-page read (fetch tool couldn't render it this session; try `cargo doc --open` locally or GitHub source `src/context/llama_context.rs` directly instead of docs.rs).
2. Whether `llama-cpp-2` wraps `llama_chat_apply_template` — decide via local `cargo doc` or grep of `llama-cpp-2/src/model.rs`; fallback (hand-rolled ChatML string) is viable regardless.
3. Exact KV-cache API names/signatures (`kv_cache_clear`, `kv_cache_seq_rm`) — needed to confirm cache-reuse/reset behavior between independent benchmark runs (each OpenJev prompt should likely start from a clean KV cache; need to confirm the right call).
4. Full enumerated cmake cache-variable list in `build.rs` (only feature-to-flag mapping was captured, not every raw `-D` flag) — relevant only if a custom CMake toolchain/vcpkg integration is later needed.
5. No "cpu-only" named feature was found — confirm default (no GPU feature) build actually produces a working CPU-only binary on Windows without CUDA/Vulkan SDKs installed.

## Sources
- https://docs.rs/llama-cpp-2/latest/llama_cpp_2/ (module list, error types)
- https://github.com/utilityai/llama-cpp-rs (repo root, CUDA feature flag mention)
- https://raw.githubusercontent.com/utilityai/llama-cpp-rs/main/examples/simple/src/main.rs (generation loop, tokenizer, sampler — via WebFetch summarization)
- https://docs.rs/crate/llama-cpp-sys-2/latest/source/build.rs (Windows build logic, feature flags)
- https://crates.io/crates/llama-cpp-2 , https://crates.io/crates/llama-cpp-sys-4 , https://lib.rs/crates/llama-cpp-sys-2 (surfaced via search, not deep-read)
