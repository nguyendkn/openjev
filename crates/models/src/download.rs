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
    },
    ModelEntry {
        id: "qwen3-4b",
        hf_repo_id: "Qwen/Qwen3-4B-GGUF",
        revision: "bc640142c66e1fdd12af0bd68f40445458f3869b",
        filename: "Qwen3-4B-Q4_K_M.gguf",
        license: "apache-2.0",
    },
    ModelEntry {
        id: "minicpm5-2b",
        hf_repo_id: "openbmb/MiniCPM5-2B-GGUF",
        revision: "2079a22f3beaa4e306449978533478fe0522f4b3",
        filename: "MiniCPM5-2B-Q4_K_M.gguf",
        license: "apache-2.0",
    },
    ModelEntry {
        id: "laya",
        hf_repo_id: "mys/laya-GGUF",
        revision: "713ae6f6e39fb54835e010485656e4484e5ec411",
        filename: "laya_english_ud_q4_k_m.gguf",
        license: "apache-2.0",
    },
];

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
