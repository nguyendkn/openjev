// Model registry and download logic
//
// `hf-hub` API confirmed by reading the resolved source on the Linux server
// (~/.cargo/registry/src/index.crates.io-*/hf-hub-1.0.0/):
// - `HFClientSync::new()` — src/blocking.rs:157 (reads HF token/endpoint from env, spawns a
//   background tokio runtime for blocking calls).
// - `HFClientSync::model(owner, name) -> HFRepositorySync<RepoTypeModel>` — src/blocking.rs:177.
// - `HFRepositorySync<T>::download_file().filename(..).revision(..).send() -> HFResult<PathBuf>`
//   — src/repository/download.rs:1332 (blocking `bon`-builder wrapper around the async
//   `HFRepository::download_file` at src/repository/download.rs:1142). Cache-aware: when
//   `local_dir` is left unset it resolves into the standard `~/.cache/huggingface/hub/`
//   layout and returns the existing cached path without a network request if already present
//   (doc comment on download_file, src/repository/download.rs:1120-1136).
//
// Revisions below are pinned to the exact commit SHA `GET
// https://huggingface.co/api/models/<repo>` returned for `.sha` (equivalently, the `hf-hub`
// resolved snapshot dir name) at the time this was written, confirmed against the already
// -cached snapshot directory names on the server (`find ~/.cache/huggingface/hub -name
// snapshots` -> matching SHA-named subdirs). Licenses are the `.cardData.license` value from
// the same API call (all four repos report `apache-2.0`; the top-level `.license` field is
// `null` for all four, so `cardData.license` is the field that actually carries it).

use crate::error::ModelsError;
use hf_hub::HFClientSync;
use std::path::PathBuf;

/// A single entry in the static model registry.
#[derive(Debug, Clone)]
pub struct ModelEntry {
    /// Short id used on the CLI (`--model <id>`).
    pub id: &'static str,
    /// `owner/name` Hugging Face repo id.
    pub hf_repo_id: &'static str,
    /// Git revision pinned to the resolved commit SHA (not a mutable branch like `main`).
    pub revision: &'static str,
    /// GGUF filename within the repo.
    pub filename: &'static str,
    /// License, from the HF Hub API's `cardData.license` field.
    pub license: &'static str,
    /// Loop 19 per-model generation policy: whether `pipeline::run_generate` force-closes
    /// the model's `<think>` reasoning block by seeding `<think>\n\n</think>\n\n` right
    /// after the chat-templated prompt (Loop 18's fix). `true` for Qwen3-4B and
    /// MiniCPM5-2B, where Loop 18 measured this as a proven net win (10/10 valid JSON,
    /// improved correctness). `false` for Qwen3-0.6B: Loop 18 found this model's
    /// correctness depends on its visible CoT (forcing it closed dropped correct-answer
    /// count 5/9 -> 3/9), so Loop 19 left it on natural CoT. Unused for the `laya` entry
    /// (dead registry path, see its own doc comment below) -- set to `false` there as a
    /// harmless default.
    pub suppress_think: bool,
    /// Loop 20: bounded/partial-CoT token budget, only meaningful when `suppress_think` is
    /// `false` (currently only `qwen3-0.6b`). `None` = unbounded natural CoT within
    /// `pipeline::MAX_TOKENS` (Loop 19's behavior). `Some(n)` = let the model reason
    /// naturally inside its own `<think>...</think>` block for up to `n` generated tokens;
    /// if it hasn't closed `</think>` by then, `pipeline::run_generate` force-injects
    /// `</think>\n\n` (the same literal Loop 18 used to suppress thinking entirely, just
    /// applied mid-generation instead of at the prompt) to move the model into its answer.
    /// Calibrated empirically against the 10 project benchmark scenarios -- see
    /// `docs/benchmarks/generation-pipeline-tuning.md`'s Loop 20 section for the full
    /// budget-sweep table and the reasoning behind the chosen value. A registry field (not
    /// a hardcoded constant) so a future loop can retune without touching generation logic.
    pub think_budget: Option<usize>,
}

/// Static registry of known models. Only `qwen3-0.6b` is exercised (downloaded + run) this
/// loop; the other three entries carry real, verified data but are not exercised — see
/// D_2 Out-of-scope.
pub const REGISTRY: &[ModelEntry] = &[
    ModelEntry {
        id: "qwen3-0.6b",
        hf_repo_id: "Qwen/Qwen3-0.6B-GGUF",
        revision: "23749fefcc72300e3a2ad315e1317431b06b590a",
        filename: "Qwen3-0.6B-Q8_0.gguf",
        license: "apache-2.0",
        suppress_think: false,
        // Loop 20: see the field doc comment above + generation-pipeline-tuning.md for the
        // calibration sweep that produced this value.
        think_budget: THINK_BUDGET_QWEN3_0_6B,
    },
    ModelEntry {
        id: "qwen3-4b",
        hf_repo_id: "Qwen/Qwen3-4B-GGUF",
        revision: "bc640142c66e1fdd12af0bd68f40445458f3869b",
        // Loop 18 quant swap: was "Qwen3-4B-Q4_K_M.gguf". Loop 16 found the official Q4_K_M
        // file has a real quality defect (+15.24% perplexity vs Q8_0, abnormal for Q4_K_M,
        // likely missing an imatrix at quantize time). Q5_K_M recovers essentially all of it
        // (+0.01% PPL). Re-validated on THIS box (Xeon Gold 5320, no AMX-INT8, not Loop 16's
        // K8s pod): real added `generation_ms` cost ~670ms/request (825ms->1495ms mean over
        // the 10 project scenarios, CLI, after Loop 18's MAX_TOKENS/think-suppression fix --
        // see docs/benchmarks/quantization-sweep-results.md's Loop 18 addendum), zero answer
        // changes across all 10 scenarios. Deployed: the defect fix justifies the cost and
        // absolute latency stays well under 2s.
        filename: "Qwen3-4B-Q5_K_M.gguf",
        license: "apache-2.0",
        suppress_think: true,
        think_budget: None,
    },
    ModelEntry {
        id: "minicpm5-2b",
        hf_repo_id: "openbmb/MiniCPM5-2B-GGUF",
        revision: "2079a22f3beaa4e306449978533478fe0522f4b3",
        // Loop 18: Loop 16 recommended Q8_0 here as "strictly dominant" (better PPL AND
        // faster) based on a K8s pod with AMX-INT8. Re-validated on THIS box (Xeon Gold 5320,
        // no AMX-INT8): Q8_0 is NOT faster -- it is ~31% slower generation / ~23% slower
        // prompt-processing than this Q4_K_M file (clean 5-rep quant_bench, n_threads=16;
        // see docs/benchmarks/quantization-sweep-results.md's Loop 18 addendum), directly
        // contradicting the K8s speed claim (AMX-INT8 specifically accelerates Q8_0 integer
        // kernels; without it Q8_0's larger per-block dequant work is memory-bound and slower
        // than Q4_K_M here). The PPL win is real but small (+5.28%->+0.06%) and does not
        // justify a ~30% latency regression per D_18's own "reconsider if speed regresses"
        // guidance. NOT swapped -- stays Q4_K_M.
        filename: "MiniCPM5-2B-Q4_K_M.gguf",
        license: "apache-2.0",
        suppress_think: true,
        think_budget: None,
    },
    ModelEntry {
        id: "laya",
        hf_repo_id: "mys/laya-GGUF",
        revision: "713ae6f6e39fb54835e010485656e4484e5ec411",
        // Loop 18 fix: was "laya_english_ud_q4_k_m.gguf", which named the one variant Loop 16
        // measured as WORST (flips a compliance/jailbreak answer vs PyTorch ground truth) and
        // did not match what `laya serve` actually loads in production
        // (`scripts/start-laya-serve.sh`'s pinned default is `laya_english_q8_0.gguf`).
        // Confirmed dead code today: `models::laya`/`pipeline::run_laya` talk to the separately
        // -running `laya serve` HTTP process directly and never call `models::find("laya")` or
        // `ensure_downloaded`, so this entry was never on the live Laya path -- but it would
        // mislead anyone who later wires the registry up (or reads it as documentation of what
        // ships), so corrected to match reality rather than removed.
        filename: "laya_english_q8_0.gguf",
        license: "apache-2.0",
        suppress_think: false,
        think_budget: None,
    },
];

// Loop 20: the calibrated Qwen3-0.6B think-budget value, factored out to a single named
// constant so the registry table above stays scannable and the chosen value + its provenance
// are documented in exactly one place (see docs/benchmarks/generation-pipeline-tuning.md for
// the full sweep this was picked from).
const THINK_BUDGET_QWEN3_0_6B: Option<usize> = Some(150); // Loop 20 calibrated winner (10/10 valid, 6/9 correct, 2486ms mean -- see docs/benchmarks/generation-pipeline-tuning.md).

/// Looks up a registry entry by its short CLI id.
pub fn find(id: &str) -> Result<&'static ModelEntry, ModelsError> {
    REGISTRY
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| ModelsError::UnknownModel(id.to_string()))
}

/// Ensures `entry`'s GGUF is present in the local HF cache, downloading it if necessary, and
/// returns the local file path. Cache-aware: a second call for an already-downloaded model
/// returns immediately without a network request (see `hf-hub`'s own doc comment cited above).
///
/// Checksum verification: `hf-hub`'s blocking `download_file` doesn't expose a convenient
/// etag/sha256 accessor on this call path (it's used internally for cache validation but not
/// returned to the caller) — documented gap, not blocking per D_2 Task 4.
pub fn ensure_downloaded(entry: &ModelEntry) -> Result<PathBuf, ModelsError> {
    let mut _s = timing::perf_span!("models::ensure_downloaded");
    _s.set("model_id", entry.id.to_string());
    let client = HFClientSync::new().map_err(|e| ModelsError::ClientInit(e.to_string()))?;
    let (owner, name) = entry
        .hf_repo_id
        .split_once('/')
        .ok_or_else(|| ModelsError::UnknownModel(entry.hf_repo_id.to_string()))?;
    let repo = client.model(owner, name);
    repo.download_file()
        .filename(entry.filename)
        .revision(entry.revision)
        .send()
        .map_err(|e| ModelsError::Download(e.to_string()))
}
