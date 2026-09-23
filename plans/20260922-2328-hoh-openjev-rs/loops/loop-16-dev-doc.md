---
loop: 16
status: pending
preservation_constraints:
  - "This loop does NOT touch crates/laya-native, apps/server, apps/cli, or the production server at 103.146.166.46 directly during exploration — findings are proposals, not live changes"
  - "Namespace llms-lab quota (32 CPU / 128Gi / 1 GPU) must not be exceeded"
  - "Any K8s resources created must be cleaned up after use"
  - "Do NOT re-run heavy CPU benchmarks against the live production server (103.146.166.46) — that caused real contention incidents in Loops 13-15. Use the K8s cluster's OWN CPU quota for speed testing instead."
---

## Objective
Use the K8s cluster (GPU for fast accuracy validation, its OWN CPU quota for speed testing —
NOT the production server) to sweep REAL GGUF quantization levels for all 4 models (Laya +
3 LLMs), looking for a faster-than-current option that doesn't lose accuracy. This is
model-side optimization work explicitly requested to run in parallel with the CPU engine
optimization tracks (Loop 13/14), using idle GPU+CPU+RAM+SSD capacity on the K8s cluster
instead of contending with the production Linux server.

Loop 15's Task 8 only tested a NAIVE weight-only fake-quantization simulation in pure Python —
not the actual sophisticated block-wise K-quant schemes (`Q4_K_M`, `Q5_K_M`, `Q6_K`) that
`llama.cpp`/`ggmlc` really use, which calibrate scale/min per block rather than naively
rounding. The naive simulation's conclusion ("INT4 flips answers, don't use it") may not hold
for the REAL K-quant algorithms, which are known to preserve accuracy much better. This loop
tests the real thing.

## Context
- Current production quant: Qwen3-0.6B=Q8_0, MiniCPM5-2B=Q4_K_M, Qwen3-4B=Q4_K_M, Laya=Q8_0
  (all already the "fast" choice per earlier loops, but never systematically swept against
  finer-grained alternatives like Q5_K_M/Q6_K/IQ4_XS).
- Ground truth for Laya: `docs/benchmarks/pytorch-ground-truth-reference.md` (Loop 15, 15
  scenarios, full probability vectors).
- For the 3 LLM models: no PyTorch ground truth exists (out of scope to generate — these are
  standard open-weight models, not a custom architecture like Laya). Use the project's existing
  10 benchmark scenarios' expected answers (already documented with high-confidence expected
  choices in `researcher-05-benchmark-usecases.md`) as the accuracy check — does the quantized
  model still pick the documented expected answer.
- `generation_ms` (the LLM JSON-generation pipeline) is the dominant cost in the overall
  `/bench` latency (multi-second, vs Laya's now-fast ~100-150ms and readout's ~100-500ms) — a
  win here matters more to overall perceived latency than further Laya work.

## Tasks
1. **Provision a CPU+GPU pod in `llms-lab`** with enough CPU (e.g. 16-24 vCPU, within the 32
   quota) to do real quantization + speed testing without touching the production server.
   Install `llama.cpp`'s build (for the 3 standard LLM models — their GGUF quantization is
   standard, unlike Laya's `ggmlc` format) and `ggmlc` (for Laya, already have build
   instructions from Loop 9-12) inside the pod.
2. **For the 3 LLM models**: use `llama.cpp`'s own `llama-quantize` tool to produce REAL
   `Q4_K_M` (current), `Q5_K_M`, `Q6_K`, and `Q8_0` variants from each model's original
   safetensors/GGUF source (check what format is available — may need to start from an
   unquantized GGUF or the original HF weights). Run each variant through the 10 benchmark
   scenarios (using the pod's own CPU, via a quick standalone `llama.cpp` CLI call or a minimal
   harness — does not need the full Rust engine), record: correct-answer agreement rate, and
   relative CPU speed (tokens/sec or wall-clock for generation) on the pod's CPU.
3. **For Laya**: use `ggmlc`'s real quantization path (check `compile_laya.py`/`ggmlc`'s CLI for
   quant-type selection at compile time) to produce `Q4_K_M`/`Q5_K_M`/`Q6_K` variants in
   addition to the current `Q8_0`. Validate each against the 15-scenario PyTorch ground truth
   (`docs/benchmarks/pytorch-ground-truth-reference.md`) for both accuracy AND speed on the
   pod's CPU.
4. **Report a clear recommendation table**: for each of the 4 models, which quant level offers
   the best speed/accuracy trade-off, with real numbers (not simulated) — and explicitly flag
   if the CURRENT choice is already optimal (a valid, useful outcome — don't manufacture a
   change if none is warranted).
5. **If a genuinely better quant is found for any model**: do NOT deploy it to production
   yourself this loop. Write the finding + exact conversion recipe to
   `docs/benchmarks/quantization-sweep-results.md` for a LATER loop (gated on review) to apply
   with the same careful backup/rollback discipline Loop 12 used for the build-flag fix.
6. **Clean up all K8s resources** when done (delete pods, any secrets used for HF token access).

## Preservation
See frontmatter. This loop's CPU speed numbers come from the K8s pod's CPU, which may differ
from the production Xeon Gold 5320 — note this caveat explicitly, treat absolute numbers as
indicative/relative-ranking evidence, not a direct production speed guarantee (same caveat
Loop 9's ggml cost-model had, appropriately caveated there).

## Validation Requirements
- Given each quant variant tested, When run against ground truth/expected-answer data, Then a
  real accuracy number is reported (not assumed) for all 4 models × however many quant levels
  were tested.
- Given the speed comparisons, When reported, Then they're clearly labeled as "K8s pod CPU,
  relative ranking" not conflated with production Xeon Gold 5320 absolute numbers.
- Given the loop ends, When `kubectl get pods,secrets -n llms-lab` is checked, Then no orphaned
  resources remain.

## Out-of-scope
- Deploying any quant change to the production server — that's explicitly a LATER, reviewed
  step.
- Re-running heavy load against 103.146.166.46 for any reason.
- Touching `crates/laya-native`, `apps/server`, `apps/cli` — Loop 13/14's territory.
