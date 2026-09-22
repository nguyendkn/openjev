---
loop: 2
status: pending
preservation_constraints:
  - "6-crate workspace skeleton (apps/cli, apps/server, crates/{engine,models,pipeline,timing}) structure and path-dependency graph"
  - "cargo build --workspace PASSES on Linux server (root@103.146.166.46, /tmp/openjev) — this is now the required Runtime.check target"
  - "Timings struct keeps exactly 7 fields (model_load_ms, warmup_ms, tokenize_ms, constrained_readout_ms, generation_ms, laya_model_load_ms, laya_inference_ms), Serialize"
  - "4 GGUF models remain cached at ~/.cache/huggingface/hub/ on the server — do not re-download unnecessarily"
  - "ggmlc-run binary at /tmp/ggmlc/build/runtime/ggmlc-run remains built and runnable"
---

## Objective
Real `engine`+`models` implementation; Qwen3-0.6B readout+generate work end-to-end on the server.

Close gaps G3/G4/G5 from `issue-ledger.md` with REAL primary-source verification (not
inference), then implement actual `llama-cpp-2`-backed inference in `crates/engine` and
`hf-hub`-backed download/cache in `crates/models`, wire `crates/pipeline`'s `readout.rs` and
`generate.rs` against a real loaded Qwen3-0.6B model, and prove it end-to-end via
`apps/cli` on the Linux server. Laya (G6/G7/G8) and the other 2 LLM models are explicitly
OUT of this loop — narrow scope to get ONE model working correctly first.

## Tasks
1. **Close G4 for real** (`llama-cpp-2` API): on the Linux server, `cargo doc --open` is not
   interactive-friendly over SSH — instead `find ~/.cargo/registry/src -path '*llama-cpp-2-*/src/context/llama_context.rs'` and read it directly (`cat`/`grep`), plus `src/model.rs`,
   `src/sampling.rs` (or wherever samplers live). Confirm exact signatures for:
   `get_logits`/`get_logits_ith`, KV-cache reset (`kv_cache_clear`/`kv_cache_seq_rm` or actual
   names), `LlamaBatch::add`, greedy sampling API, tokenizer (`str_to_token`/similar), and
   whether `llama_chat_apply_template` is wrapped (fallback: hand-build ChatML strings if not
   — Qwen3 uses `<|im_start|>role\n...<|im_end|>`). Cite exact file:line in evidence.
2. **Close G3 for real** (`hf-hub` API): same approach — read the actual resolved `hf-hub`
   crate source in `~/.cargo/registry/src/.../hf-hub-1.0.0/src/` (version already pinned per
   Loop 1's `Cargo.lock`) to confirm the real download API (builder pattern, blocking client,
   method names for "get file, given repo+filename+revision, cache-aware"). Implement
   `crates/models/src/download.rs` using the CONFIRMED API — no guessing.
3. **Close G5** (licensing): `curl -s https://huggingface.co/api/models/<repo>` for all 4
   registered repos, extract `.cardData.license` or `.license` field, record actual value per
   repo (expect apache-2.0, but verify don't assume).
4. **Implement `crates/models`**: registry with (at minimum) the Qwen3-0.6B entry fully wired
   (hf_repo_id, revision — pin to the actual commit SHA `hf-hub` resolved, not "main" — filename
   `Qwen3-0.6B-Q8_0.gguf`, license from Task 3); `ensure_downloaded()` using the confirmed
   `hf-hub` API, checksum verification if the API exposes an etag/sha256 conveniently (if not
   trivially available, note as a documented gap, don't block on it). Other 2 LLM + Laya
   entries can stay as registry stubs (data present, not necessarily exercised this loop).
5. **Implement `crates/engine`**: `Engine::load(model_path, EngineConfig{n_threads,
   batch_size})` using confirmed API from Task 1, CPU-only (no GPU features — verify
   `crates/engine/Cargo.toml` still has zero cuda/vulkan/rocm/opencl features), `tokenize`,
   `decode_prompt` (returns logits for last position), `sample_greedy`, `reset_context`
   (KV-cache reset between pipeline runs). `n_threads` default near the server's 32 cores
   (e.g. 28-32, leave headroom), configurable.
6. **Implement `crates/pipeline::readout`**: constrained single-token logit readout —
   restrict logits to candidate option-label token ids, softmax over that restricted set
   only (not full-vocab softmax then filter). Unit-testable `constrained_softmax` pure
   function + `run_readout(engine, prompt, options) -> ReadoutResult` wiring it to a real
   `Engine`.
7. **Implement `crates/pipeline::generate`**: greedy JSON generation, max_tokens=512, strip
   `<think>...</think>`, parse+validate JSON against the option schema. Pure
   `strip_think`/`parse_and_validate` functions (never panic on adversarial input) +
   `run_generate(engine, prompt, options) -> GenerateResult`.
8. **Wire `apps/cli`**: real flat-arg CLI (`--model qwen3-0.6b --prompt "..." --options A,B
   --format json`) that does: `models::ensure_downloaded` → `Engine::load` → warmup →
   `run_readout` → `Engine::reset_context` → `run_generate` → print `Timings` + both results
   as JSON. `--skip-laya` flag accepted but currently a no-op (Laya not implemented yet —
   document this explicitly in `--help` text, don't silently pretend it works).
9. **Prove it end-to-end on the Linux server**: sync workspace to `/tmp/openjev`, `cargo build
   --workspace --release` (release this time, not just debug — perf matters for later tuning
   loops), run `./target/release/openjev-cli --model qwen3-0.6b --prompt "The capital of
   France is:" --options A,B --format json` (A=London, B=Paris or similar obviously-biased
   question) and confirm real, sane output (not a crash, not garbage).

## Preservation
See frontmatter `preservation_constraints` — re-verify all of them still hold (Linux build
still passes, workspace structure intact, models still cached, ggmlc-run still present,
Timings struct unchanged) before declaring this loop done.

## Validation Requirements
- Given the confirmed `llama-cpp-2` API (Task 1), When cited in evidence, Then it references
  an exact file:line from the actual resolved crate source — not "inferred"/"should be".
- Given the confirmed `hf-hub` API (Task 2), When `crates/models/src/download.rs` is read,
  Then it contains real API calls (not a stub comment) matching the confirmed signatures.
- Given `cargo build --workspace --release` on the Linux server, When run, Then it exits 0.
- Given `openjev-cli --model qwen3-0.6b --prompt "The capital of France is:" --options
  A,B --format json` run on the Linux server, When executed, Then it prints valid JSON with
  all 7 `Timings` fields (laya fields may be `null`/0 — Laya not implemented yet, that's
  expected) and both `readout`/`generate` results populated with real, non-garbage values
  (readout should favor the correct option strongly given an obvious prompt).
- Given `run_readout` and `run_generate` with a real loaded model, When run twice in
  sequence (as `apps/cli` does), Then the second pipeline's results are not contaminated by
  the first's KV-cache state (verify `reset_context` is actually called between them).

## Out-of-scope
- Laya (`pipeline::laya`, `models::laya`, `ggmlc-run` invocation wiring) — Loop 3+.
- MiniCPM-2B and Qwen-4B actual inference (registry entries can exist, but don't need to be
  exercised/downloaded-and-run this loop) — Loop 3+.
- `apps/server` (HTTP/axum) — Loop 3+.
- Performance tuning — later loop, after the full 3-method/4-model surface works correctly
  once.
- Windows build fix (G1) — descoped per `run.md`'s Runtime decision, opportunistic only.
