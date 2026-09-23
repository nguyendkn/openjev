---
loop: 15
status: pending
preservation_constraints:
  - "This loop does NOT touch crates/laya-native, apps/server, apps/cli, or the production server at 103.146.166.46 — it is pure GPU-side reference generation on the K8s cluster"
  - "Namespace llms-lab quota (32 CPU / 128Gi / 1 GPU) must not be exceeded — request within budget"
  - "Any K8s resources created (pods/jobs) must be cleaned up after use, or clearly left running with justification"
---

## Objective
Use the K8s cluster's GPU (namespace `llms-lab`, context `fke-ncp-modas-stg-qc8ifaxe`, 1×
NVIDIA H100 available) to generate DEFINITIVE ground-truth reference outputs from the ORIGINAL
unquantized PyTorch implementation of Laya — resolving Loop 11's open question ("which build is
the real reference, `-O0` or `-O3`? both are quantized C++ reimplementations that disagree with
each other by up to 0.08 absolute probability on some inputs").

This is model-side work supporting the CPU engine optimization effort (per explicit user
direction: GPU is for model reference/optimization only, the deployed engine stays CPU-only).
Output is a reference dataset file for Loop 13+ (native Rust engine work) to validate against —
this loop does not modify the engine itself.

## Context
- Real modeling source: `github.com/NandhaKishorM/laya` (`laya/common.py`, `DecisionModel`
  class — architecture already fully reverse-engineered in Loop 9-11, see
  `plans/20260922-2328-hoh-openjev-rs/run.md` for the full spec: tokenization via
  `build_sequence`, ModernBERT-large encoder, 2-layer head, scorer, temperature calibration).
- Original unquantized weights: `convaiinnovations/laya` on Hugging Face (safetensors,
  `encoder/config.json` already read in Loop 11 — full architecture confirmed).
- 10 benchmark scenarios already defined:
  `plans/20260922-2146-openjev-rust-implementation/research/researcher-05-benchmark-usecases.md`.
- K8s: `kubectl config use-context fke-ncp-modas-stg-qc8ifaxe`, namespace `llms-lab`, quota 32
  CPU / 128Gi RAM / 1 GPU (already provisioned this session — don't request more).

## Tasks
1. **Deploy a GPU pod/Job** in `llms-lab` using an appropriate PyTorch+CUDA base image (e.g.
   `pytorch/pytorch:2.x-cudaXX.X-cudnnX-runtime` or similar — pick one compatible with the
   node's driver/CUDA version, check node labels for `nvidia.com/cuda.driver-version` or similar
   if needed), requesting `nvidia.com/gpu: 1`. Verify GPU is actually visible inside the pod
   (`nvidia-smi` or `torch.cuda.is_available()`) before proceeding — don't assume scheduling
   succeeded silently onto a non-GPU path.
2. **Install dependencies + fetch the real modeling code + weights** inside the pod: `pip
   install torch transformers` (matching versions `common.py`/`convaiinnovations/laya`'s
   `encoder/config.json` expect — check for a `requirements.txt`/`pyproject.toml` in the
   `NandhaKishorM/laya` repo if one exists), clone/download `laya/common.py`, download
   `convaiinnovations/laya`'s weights from Hugging Face (the HF token used earlier this session
   is available if needed — do NOT hardcode it in any committed file, pass via env var/K8s
   secret only, and do not print it in logs).
3. **Run inference for ALL 10 benchmark scenarios + the reference "capital of France" case**
   using the REAL `DecisionModel.forward` (unquantized, full float precision, on GPU) — record
   full output distributions (probabilities per option, confidence, act_probability) for each.
4. **Also test a few option-count edge cases** (the >10-option "11+" bucket that Loop 11 found
   a real temperature-clamping bug for, and a >16-option case matching G21's fix) to have
   ground truth for those specific edge cases too.
5. **Retrieve results** (kubectl cp, or a K8s ConfigMap/log capture — whichever is simplest) and
   write them to `docs/benchmarks/pytorch-ground-truth-reference.json` (or `.md` with embedded
   JSON) in the repo — format clearly labeled as "unquantized PyTorch, GPU, float32/float16
   precision — the true mathematical reference, not a quantized reimplementation."
6. **Compare against BOTH C++ builds' results** (Loop 11/12's `-O0`/`-O3` numbers, already
   recorded in `docs/benchmarks/server-tuning-results.md`) and against `crates/laya-native`'s
   own numbers (Loop 11's results) — report which one the true PyTorch ground truth is actually
   closest to. This may finally answer whether `-O3` (which Loop 11/12 assumed was "more
   correct") or something else is the right target.
7. **Clean up K8s resources** after retrieving results (delete the pod/Job) unless there's a
   clear reason to keep it running — namespace quota is shared, don't leave GPU reserved idle.
8. **Optional, if time permits**: since GPU is available, quickly test whether a different
   quantization scheme (e.g. simulate INT4/AWQ-style quantization numerically in Python against
   the real weights) would preserve accuracy well enough to be worth trying on the CPU engine
   later — this is exploratory, report findings but don't over-invest here, the priority is
   Tasks 1-7.

## Preservation
See frontmatter. Do not touch the production Linux server (103.146.166.46) or
`crates/laya-native` — this is entirely a K8s-side GPU task producing a reference data file.

## Validation Requirements
- Given the GPU pod, When `nvidia-smi`/`torch.cuda.is_available()` is checked, Then GPU access
  is confirmed real, not assumed.
- Given the 10+ scenarios run through the real PyTorch model, Then full output distributions
  are recorded and written to a committed reference file with clear provenance labeling.
- Given the 3-way comparison (PyTorch ground truth vs `-O0` vs `-O3` vs `laya-native`), Then a
  clear table is produced showing which is closest to ground truth for each scenario — reported
  honestly even if the answer complicates prior loops' conclusions.
- Given the loop ends, When `kubectl get pods -n llms-lab` is checked, Then no orphaned
  GPU-holding resources remain (or their retention is explicitly justified).

## Out-of-scope
- Modifying `crates/laya-native` to chase the PyTorch ground truth exactly — that's a decision
  for a LATER loop once this data exists, not this loop's job.
- Production server changes.
- Full quantization-scheme redesign (Task 8 is exploratory/optional only).
