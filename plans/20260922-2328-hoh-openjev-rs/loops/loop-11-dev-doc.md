---
loop: 11
status: pending
preservation_constraints:
  - "openjev-server and laya serve (systemd) BOTH stay live and unaffected — crates/laya-native remains standalone, not wired into production"
  - "All 3 LLM models + existing Laya (via laya serve) still correct via apps/cli and apps/server"
  - "cargo build --workspace (debug+release) and cargo test --workspace still pass"
  - "The reference case (capital of France, A/B) still picks the correct option (B) after every change this loop"
  - "Never build inside /tmp/ggmlc/build — that's where the live production laya binary lives"
---

## Objective
Close the probability gap (0.599 vs 0.586) via real layer-by-layer diffing; fix G21; implement the real head/scorer/act_head.

Loop 10 built a correct-direction, 11.3x-faster native encoder but the final probabilities
don't bit-match `laya serve`'s reference output yet, and QA found a real safety bug (G21:
unclamped option count can read out-of-bounds on >16-option questions). This loop must close
both before Loop 12 can even consider a production cutover.

## Tasks
1. **Fix G21 immediately, first**: clamp/validate the option count `k` against `max_opts=16`
   (and against the actual logits tensor size) before any read in `LayaNative::score()` —
   return a proper `Result::Err` for out-of-range `k`, don't just silently truncate (a silent
   truncation would answer a >16-option question wrong without any error signal, which is
   arguably worse than a crash). Add a test with a >16-option question proving the new bounds
   check triggers cleanly instead of reading OOB.
2. **Build an out-of-tree `ggmlc` debug copy**: `git clone` (or `cp -r`) the `ggmlc` source to
   `/tmp/ggmlc-dbg` (a fresh, separate directory — NEVER `/tmp/ggmlc/build`, which is where the
   live production `laya`/`laya serve` binary that `apps/server` depends on lives). Build its
   Python `_runtime` extension there (whatever Loop 10 attempted and correctly declined to do
   in the production tree). This is read-only research infrastructure, isolated from
   production by construction.
3. **Get real layer-by-layer intermediate tensors** from the debug build for the exact
   reference input (28-token sequence, markers [10,12]) — at minimum: post-embedding, output
   of layer 0, output of layer 13 (mid), output of layer 27 (last encoder layer), post-head
   (both head layers), pre-scorer, final logits. Compare each against `crates/laya-native`'s
   own intermediate tensors for the SAME input (add temporary debug-dump capability to
   `graph.rs` if needed — gate it behind a debug feature/env var, don't leave it as
   always-on log spam).
4. **Find and fix the actual divergence point**: the first layer where intermediates diverge
   beyond expected float noise (~1e-5 relative) tells you exactly where the bug is — is it
   the flash-attention path, GeGLU, a RoPE detail, residual ordering, or somewhere else
   entirely? Fix it there, don't keep guessing at the whole-pipeline level.
5. **Implement the REAL head/scorer/act_head** (Loop 10's was explicitly throwaway) per
   `common.py`'s `DecisionModel.forward`: `type_emb` addition, the 2 extra
   `TransformerEncoderLayer`s (`head.layers.0/1`, note: default PyTorch `TransformerEncoderLayer`
   activation is ReLU, confirmed by Loop 10's `relu` node trace — not GELU, don't copy the
   encoder's activation choice here by mistake), `scorer` MLP, and (if time permits — lower
   priority than the choice/score probability path) the `act_head` for act/escalate confidence.
6. **Re-validate against the reference AND broader cases**: once the gap is closed (or proven
   to be bounded float noise), test against several of the project's own 10 benchmark scenarios
   (`plans/20260922-2146-openjev-rust-implementation/research/researcher-05-benchmark-usecases.md`)
   — not just the France/Paris case — comparing against fresh `laya serve` reference calls for
   each. This is also where G21's fix gets exercised for real (several scenarios have 3-5+
   options).
7. **Address G22 (resource leak) if the fix is straightforward**: implement `Drop` for the
   raw ggml/gguf context wrappers. If this requires non-trivial redesign, document it as a
   known gap for Loop 12 rather than rushing a fix that risks a double-free.
8. **Benchmark the corrected implementation**: confirm the fix for the probability gap didn't
   regress the speed win — report final measured latency vs. `laya serve`.

## Preservation
See frontmatter. `/tmp/ggmlc-dbg` is a new, separate research artifact — don't let it get
confused with or accidentally symlinked into `/tmp/ggmlc/build`.

## Validation Requirements
- Given a >16-option question, When passed to `LayaNative::score()`, Then it returns a clear
  `Err` (not a panic, not a silent truncation, not UB) — proven by a real test.
- Given the layer-by-layer diff, When the divergence point is found, Then it's reported with
  the specific layer + tensor + relative error, not just "it's probably float noise."
- Given the fix, When run against the reference case, Then probabilities match `laya serve`
  within a small, explicitly-stated tolerance (e.g. <0.1% absolute, or exact if achievable) —
  state the actual achieved tolerance, don't round favorably.
- Given the same fix, When run against ≥3 of the project's other benchmark scenarios (not just
  France/Paris), Then results are compared against fresh `laya serve` calls for each, with
  real numbers reported for all of them (including any that DON'T match well — don't
  cherry-pick only the passing ones).

## Out-of-scope
- Wiring `crates/laya-native` into `apps/cli`/`apps/server`/production — Loop 12, gated on
  this loop's correctness results being genuinely solid (small, understood, bounded error) —
  not just "close enough that we stopped looking."
- Multilingual/typed-decisions Laya variants — English-only stays in scope, matching the rest
  of the project.
- Full `act_head` if it proves to need significant extra time — the choice/score probability
  path (what actually drives `best_option`/`probabilities` in `BenchReport`) is the priority.
