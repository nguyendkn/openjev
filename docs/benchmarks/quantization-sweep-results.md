# Quantization sweep — real GGUF K-quants, all 4 models (Loop 16)

**What this file is.** A REAL (not simulated) sweep of GGUF quantization levels for the four
models this project runs, measured on the K8s cluster's own CPU, not on the production server.
Loop 15's Task 8 only tested a *naive weight-only fake-quant simulation in NumPy*; this loop ran
the actual `llama.cpp` K-quant kernels and the actual `ggmlc` quantizer, loaded the resulting
GGUFs, and measured real accuracy and real CPU speed.

**Nothing here has been deployed.** Per D_16 Task 5, every recommendation below is a proposal for
a later, reviewed loop to apply with backup/rollback discipline. No production file, service or
registry entry was changed by this loop.

## Provenance and caveats

| Field | Value |
|---|---|
| cluster | `fke-ncp-modas-stg-qc8ifaxe`, namespace `llms-lab`, pod `quant-sweep` (deleted afterwards) |
| pod CPU | **Intel Xeon Platinum 8462Y+ (Sapphire Rapids)**, cgroup limit 24 vCPU, all runs pinned `-t 16` |
| ggml CPU backend loaded | `libggml-cpu-sapphirerapids.so` — `AVX512_VNNI=1, AVX512_BF16=1, **AMX_INT8=1**` |
| production CPU (for contrast) | Xeon Gold 5320 (Ice Lake) — **no AMX**, would load `libggml-cpu-icelake.so` |
| `llama.cpp` build | `ghcr.io/ggml-org/llama.cpp:full`, libggml 0.24.0 |
| `ggmlc` | `github.com/monatis/ggmlc` @ main, built `-DCMAKE_BUILD_TYPE=Release` (`-O3`, per Loop 12) |
| Laya ground truth | `docs/benchmarks/pytorch-ground-truth-reference.md` (15 scenarios, fp32 H100) |
| LLM scenario set | the 10 project benchmark scenarios (`researcher-05-benchmark-usecases.md`) |
| LLM perplexity corpus | WikiText-2 raw **test** split, 120 chunks @ `n_ctx=512` (~61k tokens), real data |
| GPU used | none — the whole sweep is CPU work; the namespace's 1 GPU was never requested |

> **Speed caveat (explicit, per D_16 Preservation).** Every tok/s and ms number here is
> **K8s-pod CPU, relative ranking only**. The pod is Sapphire Rapids *with* AMX-INT8; production is
> Ice Lake *without* it. AMX-INT8 specifically accelerates the **Q8_0** integer kernels, so the
> "Q8_0 is as fast as Q4_K_M" result below is expected to be **weaker on the production box**.
> Treat the ordering as evidence, not the absolute numbers, and re-measure on the Xeon Gold 5320
> before any deploy decision. This is the same caveat Loop 9's ggml cost-model carried.

Raw data, per-scenario answers and the exact harness scripts: `docs/benchmarks/quant-sweep-loop16/`.

---

## 1. Headline: what was actually tested

| model | current production quant | variants really built + run this loop |
|---|---|---|
| Qwen3-0.6B | `Q8_0` (official repo file) | Q4_K_M, Q5_K_M, Q6_K, Q8_0 (self-quantized from F16) + official Q8_0 + F16 — **6** |
| MiniCPM5-2B | `Q4_K_M` (official) | Q4_K_M (official), Q5_K_M, Q6_K (self-quantized from official F16), Q8_0 (official), F16 — **5** |
| Qwen3-4B | `Q4_K_M` (official) | Q4_K_M, Q5_K_M, Q6_K, Q8_0 — all four **official** repo files — **4** |
| Laya | `Q8_0` (what `laya serve` actually loads) | F16, Q8_0, UD_Q4_K_M (shipped) + Q8_0, Q4_K_M, Q4_0 (recompiled with `ggmlc`) — **6** |

Qwen3-0.6B and MiniCPM5-2B had no repo-provided K-quants beyond one level, so they were produced
locally with `llama-quantize` (Qwen3-0.6B needed `convert_hf_to_gguf.py` from the original
`Qwen/Qwen3-0.6B` safetensors first — its GGUF repo ships **only** `Q8_0`).

---

## 2. FINDING: Laya cannot have real K-quants at all

This invalidates D_16's Task 3 as written, and it is the single most important structural result
of the loop.

`ggmlc`'s quantizer implements **only `Q8_0` and `Q4_0`** block formats:

```
/work/ggmlc/python/ggmlc/quantization/quantize.py:1:
"""Block-level quantization and dequantization algorithms for GGML formats (Q8_0, Q4_0)."""

/work/ggmlc/examples/laya/compile_laya.py:61:
QUANT_CHOICES = ["f32", "f16", "q8_0", "q4_0", "q4_k_m", "ud_q4_k_m"]
```

`q4_k_m` and `ud_q4_k_m` in that list are **mixed-precision *policies* named after llama.cpp's
recipes, not K-quant block formats**. `policies.py`'s `Q4_K_M_POLICY` and `UNSLOTH_DYNAMIC_POLICY`
map tensor *roles* onto `{F32, F16, Q8_0, Q4_0}` only. Confirmed by reading the shipped GGUFs'
actual tensor types — **there is not one K-quant tensor in any Laya GGUF**:

| Laya GGUF | tensor type histogram (153 tensors) |
|---|---|
| `laya_english_f16.gguf` | `F32: 28, F16: 125` |
| `laya_english_q8_0.gguf` | `F32: 28, Q8_0: 124, F16: 1` |
| `laya_english_ud_q4_k_m.gguf` | `F32: 28, Q8_0: 91, **Q4_0: 31**, F16: 3` |

So the D_16 hypothesis — "real K-quants calibrate scale/min per block and may preserve accuracy
where Loop 15's naive simulation said they wouldn't" — **cannot be tested for Laya**. Producing a
true `Q5_K_M`/`Q6_K` Laya GGUF would require extending `ggmlc`'s quantizer *and* verifying its
runtime kernels handle K-quant tensors. That is a real, scoped piece of upstream work, not a
config change. Recorded as a gap, not attempted here.

Corollary: Loop 15's simulated `q4_0_blk32` row **was** the right model of Laya's 4-bit option —
and this loop measured the real thing to check it (§3.2).

## 3. Laya — real measurements against PyTorch fp32 ground truth

Harness: the production path exactly — `laya serve <gguf> --port 8090 --device cpu --threads 16`,
`POST /v1/decide` with the same byte-identical `choice` payload `crates/models/src/laya.rs` sends.
Each of the 15 ground-truth scenarios: 1 warmup + 3 measured requests, median latency reported.
`d_top` = |P(ground-truth winning option) − PyTorch fp32 value|.

### 3.1 The three SHIPPED variants (`mys/laya-GGUF`, rev `713ae6f…`)

| variant | file size | choice agreement vs PT fp32 | mean `d_top` | max `d_top` | median latency (pod CPU) |
|---|---|---|---|---|---|
| **F16** | 846 MB | **15/15** | **0.000618** | **0.004395** | **61.37 ms** |
| **Q8_0** *(production)* | 452 MB | **15/15** | 0.004221 | 0.024990 | 67.40 ms |
| UD_Q4_K_M | 420 MB | 14/15 | 0.051252 | 0.227537 | 68.81 ms |

UD_Q4_K_M's single flip is `2_jailbreak_detection` → `injection-attempt` instead of ground truth
`benign`. (Amusing but irrelevant: the 4-bit build gives the *humanly* better answer there. It is
still a deviation from the reference model, which is what is being measured.)

**F16 is simultaneously the most accurate AND the fastest.** It is 6.8x closer to ground truth
than Q8_0 (mean `d_top` 0.00062 vs 0.00422) and 9% faster per request. This reproduces the speed
ordering Loop 8 found on the production box (F16 1.45s < Q8_0 1.49s < UD_Q4_K_M 1.90s, threads=28)
on completely different hardware — so the ordering is robust, and Loop 8 chose Q8_0 purely on
file-size/RAM grounds without any accuracy data (the ground truth did not exist until Loop 15).

F16 also closes most of the "Q8_0 GGUF gap" that `pytorch-ground-truth-reference.md` characterised:
the 0.0250 offset on the reference case and the 0.0290/0.0247 gaps on scenarios 8 and 6 are a
**weight-quantization** effect after all, in larger part than that document estimated — the F16
build's worst deviation across all 15 scenarios is 0.0044.

### 3.2 Controlled `ggmlc` recompiles (all compiled here, identical flags: `--family english --device cpu --max-batch 8 --min-seq 64`)

| variant | file size | choice agreement vs PT fp32 | agreement vs own `Q8_0` | mean `d_top` | median latency |
|---|---|---|---|---|---|
| Q8_0 (recompile) | 452 MB | 14/15 | 15/15 | 0.071628 | 67.61 ms |
| Q4_K_M (ggmlc policy) | 346 MB | 14/15 | **13/15** | 0.059688 | 68.54 ms |
| Q4_0 (pure 4-bit) | 241 MB | **13/15** | **12/15** | 0.089731 | 71.97 ms |

⚠️ **Read this block only as an intra-set comparison.** The recompiled `Q8_0` is 451,507,104 bytes
vs the shipped `Q8_0`'s 451,505,440 — same weights, ~1.6 KB of metadata apart — yet it flips
`E2_17opt_g21_over16` to `other` with P(correct)=0. That is the **option-budget truncation at
k=17** (`head_max_len=192`, the `opt_budget < 16` branch), i.e. a **compile-shape artifact of the
default `--min-seq 64`**, not a quantization effect. The shipped GGUFs were evidently compiled with
different sequence-shape flags. Cross-comparing §3.1 and §3.2 rows is therefore invalid; within
§3.2 the flags are identical and the comparison is clean.

**Real `Q4_0` scores 12/15 agreement with its own Q8_0 baseline.** Loop 15's NumPy simulation
predicted `q4_0_blk32` → **12/15**. The simulation was right. The two scenarios real Q4_0 flips are
`9_compliance_gating` and `E4_2opt_binary_risk` — both security/compliance gating questions, the
same failure *class* the simulation flagged (`2_jailbreak_detection` and `9_compliance_gating`).

### 3.3 Laya verdict

* **Do NOT ship any 4-bit Laya build.** Confirmed with real kernels, not simulation: it flips
  compliance/safety-gating answers and buys nothing — it is also the *slowest* of the three
  (K-quant-style mixed dequant on the CPU path costs more than the smaller weights save).
* **A move from Q8_0 → F16 is the one genuinely attractive change found in this loop for Laya**:
  strictly better accuracy (6.8x closer to the true model) *and* ~9% lower latency, for +394 MB of
  file/RAM. **But it is not deployable as-is** — see §6 blockers.

---

## 4. The 3 LLMs — perplexity (the trustworthy accuracy metric)

WikiText-2 **test** split, 120 chunks @ `n_ctx=512` (~61k real tokens), `llama-perplexity -t 16`.
Speed from a separate clean `llama-bench -t 16 -p 256 -n 128 -r 3` run with nothing else on the pod.
`tg` = token generation (dominates `generation_ms`), `pp` = prompt processing.

### Qwen3-0.6B — baseline F16 PPL 22.5220

| variant | size | PPL | Δ vs F16 | tg tok/s | pp tok/s |
|---|---|---|---|---|---|
| **Q8_0 (official, PRODUCTION)** | 0.64 GB | 22.5774 | **+0.25%** | **164.49** | 1580 |
| Q8_0 (self-quantized) | 0.80 GB | 22.5322 | +0.05% | **170.28** | 2379 |
| Q6_K | 0.62 GB | 22.5521 | +0.13% | 128.75 | 1800 |
| Q5_K_M | 0.55 GB | 23.4267 | +4.02% | 131.28 | 2139 |
| Q4_K_M | 0.48 GB | 24.4901 | **+8.73%** | 149.88 | 2190 |
| F16 | 1.51 GB | 22.5220 | — | 85.83 | 994 |

### MiniCPM5-2B — baseline F16 PPL 15.2977

| variant | size | PPL | Δ vs F16 | tg tok/s | pp tok/s |
|---|---|---|---|---|---|
| **Q4_K_M (official, PRODUCTION)** | 1.56 GB | 16.1048 | **+5.28%** | 52.04 | 609 |
| Q5_K_M | 1.81 GB | 15.4738 | +1.15% | 41.57 | 657 |
| Q6_K | 2.07 GB | 15.3420 | +0.29% | 40.96 | 426 |
| **Q8_0 (official)** | 2.68 GB | **15.3075** | **+0.06%** | **55.47** | 677 |
| F16 | 5.04 GB | 15.2977 | — | 28.02 | 267 |

### Qwen3-4B — baseline Q8_0 PPL 14.6658 (no F16 built; Q8_0 is within ~0.1% of F16 on the other two models)

| variant | size | PPL | Δ vs Q8_0 | tg tok/s | pp tok/s |
|---|---|---|---|---|---|
| **Q4_K_M (official, PRODUCTION)** | 2.50 GB | 16.9020 | **+15.24%** | **31.81** | 343 |
| Q5_K_M (official) | 2.89 GB | 14.6671 | +0.01% | 24.16 | 314 |
| Q6_K (official) | 3.31 GB | 14.7191 | +0.36% | 25.03 | 299 |
| **Q8_0 (official)** | 4.28 GB | 14.6658 | — | 29.59 | 342 |

**Two results that were not expected and matter:**

1. **Q8_0 is the *fastest or second-fastest* generation quant on all three models**, beating
   Q5_K_M and Q6_K outright and matching/beating Q4_K_M on two of three. K-quants trade memory
   bandwidth for per-block dequant arithmetic; on a machine with AMX-INT8 and plenty of bandwidth
   the dequant work dominates. **This is the part most likely to shrink on the production Ice Lake
   box** (no AMX) — re-measure before deciding.
2. **`Qwen/Qwen3-4B-GGUF`'s official `Q4_K_M` is unusually bad: +15.2% perplexity.** Q5_K_M
   recovers essentially all of it (+0.01%). That is far outside normal Q4_K_M degradation (~2-5%)
   and suggests the upstream Q4_K_M was produced without an importance matrix. This is a real
   quality defect in the file production currently runs.

## 5. The 10 project benchmark scenarios — and why they can't settle this

The 10 scenarios were run through every LLM variant via `llama-server /v1/chat/completions`,
temperature 0 / top_k 1, `n_predict 512`, reproducing `pipeline::run_generate`'s prompt,
`<think>` stripping and `{"answer": ...}` validation byte-for-byte.

| model | variant | valid JSON | agrees w/ highest-fidelity variant | matches documented expected answer |
|---|---|---|---|---|
| Qwen3-0.6B | Q8_0 (prod) | 8/10 | 6/10 | 5/9 |
| Qwen3-0.6B | Q4_K_M | 7/10 | 7/10 | 4/9 |
| Qwen3-0.6B | Q5_K_M | 9/10 | 5/10 | 3/9 |
| Qwen3-0.6B | Q6_K | 9/10 | 6/10 | 4/9 |
| Qwen3-0.6B | F16 (ref) | 9/10 | 9/10 | 5/9 |
| MiniCPM5-2B | Q4_K_M (prod) | 9/10 | 6/10 | 7/9 |
| MiniCPM5-2B | Q5_K_M | 9/10 | 8/10 | 7/9 |
| MiniCPM5-2B | Q6_K | 10/10 | 7/10 | 6/9 |
| MiniCPM5-2B | Q8_0 | 8/10 | 7/10 | 5/9 |
| MiniCPM5-2B | F16 (ref) | 8/10 | 8/10 | 6/9 |
| Qwen3-4B | Q4_K_M (prod) | 7/10 | 4/10 | 7/9 |
| Qwen3-4B | Q5_K_M | 6/10 | 5/10 | 4/9 |
| Qwen3-4B | Q6_K | 5/10 | 4/10 | 4/9 |
| Qwen3-4B | Q8_0 (ref) | 5/10 | 5/10 | 4/9 |

**These numbers are real but they do not rank the quants, and must not be used to.** Two
independent Q8_0 builds of the *same* Qwen3-0.6B weights (official vs self-quantized, PPL 22.577 vs
22.532) agree with each other on only 6/10 scenarios. The variance is dominated by Qwen3's
long chain-of-thought: greedy decoding runs 130-512 tokens of `<think>` per scenario, frequently
hitting the 512-token `MAX_TOKENS` cap (`8_adversarial_ambiguity` and `10_loan_credit_risk`
truncate to invalid JSON on nearly every 4B variant), and a single early token divergence rewrites
the whole trajectory. A 10-item single-shot suite cannot resolve a sub-1% quality difference
through that. **Perplexity (§4) is the metric this loop's recommendations rest on**; §5 is
published for completeness and as evidence of the harness's fidelity, not as a ranking.

Secondary but actionable observation: **the low valid-JSON rates (5/10 on Qwen3-4B) are a pipeline
problem, not a quantization problem** — they are `MAX_TOKENS=512` truncating mid-`<think>`. Raising
`MAX_TOKENS`, or disabling Qwen3 thinking mode (`/no_think` or `enable_thinking=false` in the chat
template), would plausibly do far more for both accuracy and `generation_ms` than any quant change
in this document. Out of scope here; flagged for the issue ledger.

---

## 6. Recommendations

| model | current | recommendation | why | risk |
|---|---|---|---|---|
| **Qwen3-0.6B** | `Q8_0` | **KEEP — already optimal** | Fastest tg of any quant tested (164 tok/s; only the self-quantized Q8_0 beats it, by 3.5%, for +0.16 GB) *and* +0.25% PPL. Every K-quant alternative is both slower and worse. | none |
| **MiniCPM5-2B** | `Q4_K_M` | **CHANGE → `Q8_0`** | Strictly dominant: PPL +0.06% vs +5.28%, **and 6.6% faster** tg (55.47 vs 52.04). Costs +1.12 GB RAM. | low — official repo file, one-line registry change |
| **Qwen3-4B** | `Q4_K_M` | **CHANGE → `Q5_K_M`** (or `Q8_0` if RAM allows) | Current file is defective: **+15.24% PPL**. Q5_K_M costs 24% tg speed for a ~13% perplexity recovery; Q8_0 costs only 7% speed for the same recovery but +1.78 GB. | medium — real latency cost; `generation_ms` is the dominant term in `/bench` |
| **Laya** | `Q8_0` | **CHANGE → `F16`, but BLOCKED** (see below) | 6.8x closer to PyTorch ground truth *and* ~9% faster. +394 MB. | **blocked**, not low |

### Why the Laya change is blocked, not merely unreviewed

`crates/laya-native` hardcodes `GGML_TYPE_Q8_0` in its hand-built graph for every weight it loads
(`graph.rs:108,141,147,157,165`, `head.rs:46,73` — `w.expect(..., s::GGML_TYPE_Q8_0)`), and its
tests point at `/root/.cache/laya-models/laya_english_q8_0.gguf`. Swapping the GGUF `laya serve`
loads would silently desynchronise the two Laya implementations, and `crates/laya-native` is Loop
13/14's territory. **Any Laya quant change must be coordinated with that crate, not applied to the
serve script alone.**

### Pre-existing inconsistency found while doing this (not introduced by this loop)

`crates/models/src/download.rs:71` registers Laya as `laya_english_ud_q4_k_m.gguf`, but production
`laya serve` loads `laya_english_q8_0.gguf` (`scripts/start-laya-serve.sh:21`) and
`crates/laya-native` expects Q8_0. The registry entry is not on the live Laya path (`models::laya`
is an HTTP client, it never consults the registry), so nothing is broken today — but it names the
**one variant this loop measured as worst**, and would mislead anyone who wires the registry up.
Worth fixing to `laya_english_q8_0.gguf` in a later loop. Not touched here (out of D_16 scope).

---

## 7. Exact conversion recipes (for the later, reviewed loop)

No new tooling is needed for the two LLM changes — both target files already exist upstream, in the
repos and at the revisions already pinned in `crates/models/src/download.rs`.

**MiniCPM5-2B → Q8_0** — edit the registry entry only:

```rust
// crates/models/src/download.rs
ModelEntry {
    id: "minicpm5-2b",
    hf_repo_id: "openbmb/MiniCPM5-2B-GGUF",
    revision: "2079a22f3beaa4e306449978533478fe0522f4b3", // unchanged, file exists at this rev
    filename: "MiniCPM5-2B-Q8_0.gguf",                    // was MiniCPM5-2B-Q4_K_M.gguf
    license: "apache-2.0",
},
```

**Qwen3-4B → Q5_K_M** — same, file exists at the pinned revision:

```rust
ModelEntry {
    id: "qwen3-4b",
    hf_repo_id: "Qwen/Qwen3-4B-GGUF",
    revision: "bc640142c66e1fdd12af0bd68f40445458f3869b", // unchanged
    filename: "Qwen3-4B-Q5_K_M.gguf",                     // was Qwen3-4B-Q4_K_M.gguf
    license: "apache-2.0",
},
```

**Laya → F16** (blocked; recipe recorded for completeness) — the file already exists at the pinned
revision as `laya_english_f16.gguf` in `mys/laya-GGUF`, so no recompile is needed. Changing it
means updating `scripts/start-laya-serve.sh`'s `GGUF` default **and** `crates/laya-native`'s
hardcoded `GGML_TYPE_Q8_0` expectations together.

**If a quant not published upstream is ever wanted**, the reproducible path used here:

```bash
# from original HF weights (only needed when the GGUF repo lacks an F16, e.g. Qwen3-0.6B):
python3 convert_hf_to_gguf.py <hf_snapshot_dir> --outfile model-F16.gguf --outtype f16
# real K-quant:
llama-quantize model-F16.gguf model-Q5_K_M.gguf Q5_K_M 20
# Laya (ggmlc only; NO real K-quants available):
python3 ggmlc/examples/laya/compile_laya.py --family english \
        --quantize {f32|f16|q8_0|q4_0|q4_k_m|ud_q4_k_m} --output out.gguf --device cpu
```

## 8. Required before any of this ships

1. **Re-measure tg tok/s on the production Xeon Gold 5320.** The Q8_0-is-fast result leans on
   AMX-INT8, which that CPU does not have. If Q8_0 loses its speed edge there, the MiniCPM5-2B
   recommendation weakens from "strictly dominant" to "accuracy-for-speed trade".
2. **Check RAM headroom.** MiniCPM5-2B +1.12 GB and Qwen3-4B +0.39 GB (Q5_K_M) / +1.78 GB (Q8_0)
   resident, on top of whatever `apps/server` and `laya serve` already hold.
3. **Coordinate any Laya change with `crates/laya-native`** (§6).

## 9. Open gaps

* No true K-quant is possible for Laya without extending `ggmlc`'s quantizer (§2). Unquantified how
  much a real Q6_K Laya would help — F16 already reaches 0.0006 mean deviation, so the headroom is
  small; the interesting question would be Q6_K's *speed*, not its accuracy.
* No F16 baseline was built for Qwen3-4B (its GGUF repo has none and the 8 GB safetensors download
  was not judged worth it); Q8_0 is used as the reference and is within ~0.1% of F16 on both models
  where both were measured.
* IQ-quants (`IQ4_XS`, `IQ3_M`) were not tested. They need an importance matrix from a calibration
  corpus to be meaningful, which is a larger job; given Q4_K_M already loses to Q8_0 on *speed*
  here, a smaller-still quant is unlikely to win on this hardware.
* `act_head` / the `choice:11+` temperature-policy conflict recorded in
  `pytorch-ground-truth-reference.md` is untouched by this loop; the E1-E3 rows above use the
  `ggmlc` raw-temperature semantics, matching what `laya serve` actually computes.

## 10. Reproducing this

Harness scripts are committed verbatim in `docs/benchmarks/quant-sweep-loop16/harness-*`.

```bash
kubectl config use-context fke-ncp-modas-stg-qc8ifaxe
kubectl create secret generic hf-token -n llms-lab --from-literal=token=<HF_TOKEN>
# pod: ghcr.io/ggml-org/llama.cpp:full, 24 CPU / 64Gi / 300Gi emptyDir, no GPU
apt-get install -y cmake build-essential python3-venv
git clone --recursive https://github.com/monatis/ggmlc && cmake -S ggmlc -B ggmlc/build \
    -DCMAKE_BUILD_TYPE=Release && cmake --build ggmlc/build -j20   # -> examples/laya/laya
# then harness-mkquants.sh, harness-runall.sh, harness-ppl.sh, harness-bench_laya.py
```

The pod and the `hf-token` secret were **deleted** afterwards and the namespace quota confirmed
released. The production server `103.146.166.46` was not contacted at any point, and
`crates/laya-native`, `apps/server` and `apps/cli` were not modified.
