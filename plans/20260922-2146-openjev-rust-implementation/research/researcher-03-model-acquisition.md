# Research: Model Acquisition (GGUF sources, hf-hub crate, pinning, licensing)

Budget note: web search only, no HF org page browsed exhaustively. Several claims below are
search-summary-derived, not primary-source-read — flagged as UNVERIFIED where relevant.

## 1. GGUF repos/files for the 3 models

### Qwen3-0.6B — VERIFIED repos exist
- `unsloth/Qwen3-0.6B-GGUF` — https://huggingface.co/unsloth/Qwen3-0.6B-GGUF
- `bartowski/Qwen_Qwen3-0.6B-GGUF` — https://huggingface.co/bartowski/Qwen_Qwen3-0.6B-GGUF
- Q8_0 filename pattern: `Qwen_Qwen3-0.6B-Q8_0.gguf` (bartowski) / `Qwen3-0.6B-Q8_0.gguf` (unsloth).
  639MB for Q8_0 on a 0.6B model is plausible (~1 byte/param + overhead) — size not directly
  confirmed via fetch this session (UNVERIFIED exact byte count).
- Typical quant ladder available from these quantizers: Q2_K…Q8_0, incl. Q4_K_M, Q5_K_M, Q6_K, Q8_0.

### Qwen3-4B vs "Qwen3.5-4B" — resolves the naming question, partially
- BOTH exist as distinct HF repos:
  - `unsloth/Qwen3-4B-GGUF` (older gen) — https://huggingface.co/unsloth/Qwen3-4B-GGUF
  - `Qwen/Qwen3-4B-GGUF` (official) — https://huggingface.co/Qwen/Qwen3-4B-GGUF
  - `unsloth/Qwen3.5-4B-GGUF` (newer gen, has `Qwen3.5-4B-Q8_0.gguf`) —
    https://huggingface.co/unsloth/Qwen3.5-4B-GGUF
  - `bartowski/Qwen_Qwen3.5-4B-GGUF` — https://huggingface.co/bartowski/Qwen_Qwen3.5-4B-GGUF
- **Cannot confirm from search which one the original openjev.com site actually used** — both
  "Qwen3-4B" and "Qwen3.5-4B" are real, currently-published models. Recommend checking
  openjev's own GitHub repo (model IDs likely hardcoded) rather than inferring from name alone.

### MiniCPM4 2B vs "MiniCPM5 2B" — evidence favors MiniCPM5-2B being the real 2B checkpoint
- Search of the `openbmb` HF org surfaced: `openbmb/MiniCPM4-0.5B` (no GGUF found),
  `openbmb/MiniCPM4-8B-GGUF`, `openbmb/MiniCPM5-1B-GGUF`, `openbmb/MiniCPM5-2B-GGUF`.
  **No `MiniCPM4-2B` or `MiniCPM4-2B-GGUF` repo surfaced in search results.**
  - https://huggingface.co/openbmb/MiniCPM5-2B-GGUF
  - https://huggingface.co/openbmb/MiniCPM4-8B-GGUF
  - https://huggingface.co/openbmb/MiniCPM4-0.5B
- INFERENCE (not confirmed by directly browsing the full org listing): the MiniCPM4 generation
  shipped 0.5B/8B sizes; the 2B-class small model is a MiniCPM5-generation checkpoint. This
  supports the site copy ("MiniCPM5 2B") over the GitHub repo description ("MiniCPM4 2B") being
  the accurate name — but treat as inferred, not verified, until the openbmb org page or
  openjev's own source is checked directly.
- OpenBMB GGUF repos typically ship the standard llama.cpp quant ladder (Q4_K_M/Q5_K_M/Q8_0
  etc.) per prior OpenBMB release pattern; exact list for MiniCPM5-2B-GGUF not enumerated this
  session (UNVERIFIED).

## 2. `hf-hub` Rust crate API (docs.rs + GitHub README, fetched)

**Caveat on confidence**: both fetches (docs.rs/hf-hub and raw GitHub README) returned a
`HFClient`/`HFClientSync`/`.download_file().filename(...).send()` builder API with a `1.0.0`
version reference. This does **not** match the older, more commonly documented `hf-hub` API
(`Api`/`ApiBuilder`/`Repo::with_revision`/`.get(filename)`) that most existing tutorials and
crate usage in the wild (e.g. candle, llama-cpp-2 examples) reference. Two independent fetches
agreeing suggests the crate underwent a breaking rewrite to this builder style, but this is
**UNVERIFIED against crates.io version history** — confirm the actual currently-published
version and its real API before writing implementation code (do not trust this summary as
ground truth for signatures).

- Async by default (`HFClient`); blocking via `features = ["blocking"]` → `HFClientSync`,
  `HFRepositorySync`, which "mirror async method-for-method" using an internal tokio runtime.
- Revision/commit pinning: not shown explicitly in either fetched snippet, but `HFError`
  reportedly includes a `RevisionNotFound` variant, implying a revision parameter exists
  somewhere in the repo/model builder chain — exact method name UNCONFIRMED this session.
- Cache location: **conflicting info between the two fetches** — docs.rs summary said
  `HF_HOME`/`HF_HUB_CACHE` env vars, default `~/.cache/huggingface` (matches Python
  `huggingface_hub` convention); GitHub README fetch said default is `.cache/huggingface/hub`
  relative to CWD, overridable via `.cache_dir(...)`. Do not rely on either without checking the
  actual source — flag for direct crate inspection (`cargo doc --open` or crates.io) before
  relying on a specific default path in code.
- Integrity verification: downloads are described as "content-addressed under HF_HUB_CACHE with
  on-disk locking" (dedup by content hash), but **no explicit sha256/etag verification API was
  found in either fetch** — treat checksum verification as the caller's responsibility (see §3).

Sources: https://docs.rs/hf-hub , https://github.com/huggingface/hf-hub (README.md),
https://crates.io/crates/hf-hub (listed, not fetched — recommend checking version page directly).

## 3. Best practice: pinning + integrity verification (general knowledge, not searched this session)

- **Pin by commit SHA, not tag/branch.** HF Hub revisions are git refs; `main`/tags can move or
  be force-updated by the repo owner. A 40-char commit SHA is immutable. Store it in project
  config alongside the repo id and filename.
- **Checksum verification**: HF's model API (`GET /api/models/{repo_id}?blobs=true` or the
  per-file LFS pointer) exposes a `sha256` for LFS-tracked files (GGUF files are always LFS).
  Fetch and record this sha256 once when pinning the revision, then after download compute
  sha256 locally (e.g. via the `sha2` crate) and compare before loading the file into
  `llama-cpp-2`. This is standard practice inferred from HF's public API shape — **not verified
  via a live fetch this session**; confirm exact endpoint/field name before implementing.
- Practical flow: pin `(repo_id, revision_sha, filename, expected_sha256)` as a static table in
  the Rust source or a checked-in manifest; download via hf-hub with `revision_sha`; verify
  sha256 post-download; fail closed (refuse to load) on mismatch.

## 4. Licensing (partially verified via search)

- **Qwen3 family**: Apache-2.0. Search results consistently describe unsloth/Qwen team GGUF
  repos as Apache-2.0. — https://huggingface.co/unsloth/Qwen3-4B-GGUF (search summary)
- **MiniCPM4 / MiniCPM5 family**: Apache-2.0 per OpenBMB's stated licensing pattern (search
  summary referencing OpenBMB model cards) — not read directly from a MiniCPM4/5-2B-GGUF model
  card this session (UNVERIFIED at the specific-repo level, but Apache-2.0 is OpenBMB's
  consistent stated license across the MiniCPM line per GitHub repo:
  https://github.com/openbmb/minicpm).
- **GGUF requantizations (bartowski/unsloth)**: standard community practice is that a
  requantization inherits the base model's license and does not impose an additional one; the
  quantizer repos typically just tag the same license in their HF metadata. This is general
  knowledge, not confirmed per-repo this session.
- Net: if both base licenses are indeed Apache-2.0, redistributing/bundling GGUF weights in an
  open-source Rust project should be permissible (attribution/notice requirements per Apache-2.0
  apply — e.g. keep a NOTICE referencing Qwen/OpenBMB). **Recommend a direct read of each
  specific GGUF repo's license field before shipping**, since quantizer repos occasionally omit
  or mis-tag license metadata even when the base model's license is permissive.

## Unresolved questions
1. Which exact Qwen 4B variant did openjev.com actually use — "Qwen3-4B" or "Qwen3.5-4B"? Both
   are real HF repos; need to check openjev's own source/GitHub, not inferable from search.
2. Does an actual `MiniCPM4-2B` (non-GGUF or GGUF) repo exist anywhere on HF that this search
   simply missed? Org page was not browsed exhaustively — recommend a direct visit to
   https://huggingface.co/openbmb to enumerate all models before finalizing the model list.
3. What is the true current `hf-hub` crate API and version (crates.io shows the version; the two
   fetches here gave a `HFClient` builder API under version `1.0.0` that conflicts with commonly
   known older `Api`/`ApiBuilder` usage) — needs direct crates.io/docs.rs confirmation, ideally
   by pinning an exact version and reading its rendered docs or source before coding against it.
4. Exact default cache path for `hf-hub` (`~/.cache/huggingface` vs CWD-relative
   `.cache/huggingface/hub`) — conflicting fetch results, needs direct source check.
5. Exact HF API field/endpoint for a file's sha256 (to compare against post-download hash) —
   not verified live this session.
6. Full quant ladder + exact file sizes for `MiniCPM5-2B-GGUF` (or MiniCPM4-2B if it turns out to
   exist) and for the chosen Qwen3(.5)-4B repo — not enumerated this session.
