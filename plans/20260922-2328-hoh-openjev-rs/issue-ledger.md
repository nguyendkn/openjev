# Issue Ledger — HoH run hoh-openjev-rs

| ID | Loop opened | Behavior | Status | Loop closed | Reopened | Evidence |
|----|-------------|----------|--------|-------------|----------|----------|
| G1 | 1 | Windows `cargo build --workspace` fails: libclang missing for `llama-cpp-sys-2` bindgen | open | — | — | loop-01-evidence.md § Runtime Check |
| G2 | 1 | Unresolved #1/#2 (model repo IDs): fallback IDs work but openjev GitHub source never cited | open | — | — | loop-01-evidence.md § Unresolved Gaps |
| G3 | 1 | Unresolved #3 (hf-hub API): stub only, no real API usage/confirmation | closed | 2 | — | loop-02-evidence.md § Verified Behaviors (G3); crates/models/src/download.rs real HFClientSync/download_file usage, grep-confirmed against hf-hub-1.0.0 source |
| G4 | 1 | Unresolved #4 (llama-cpp-2 logits/KV-cache API): inferred, not confirmed against source/cargo doc | closed | 2 | — | loop-02-evidence.md § Verified Behaviors (G4); crates/engine/src/lib.rs real llama-cpp-2 0.1.156 usage, grep-confirmed exact signatures against resolved source |
| G5 | 1 | Unresolved #5 (model licensing): pending, no per-repo HF license field checked | closed | 2 | — | loop-02-evidence.md § Verified Behaviors (G5); curl HF API for all 4 repos, cardData.license=apache-2.0 + sha matches pinned revision, all 4 |
| G6 | 1 | Unresolved #6 (ggmlc-run typed-decision tensor-binding semantics for Laya): tensor names known, encoding/output semantics unconfirmed | open | — | — | loop-01-evidence.md § Unresolved Gaps; `ggmlc-run info` output — out of scope Loop 2 (Laya deferred to Loop 3+) |
| G7 | 1 | Unresolved #7 (ggmlc-run Windows build): never attempted | open | — | — | loop-01-evidence.md § Unresolved Gaps — out of scope Loop 2 |
| G8 | 1 | Unresolved #8 (Laya CPU latency sanity check): not measured | open | — | — | loop-01-evidence.md § Unresolved Gaps — out of scope Loop 2 |
| G9 | 2 | docs/codebase-summary.md not updated for Loop 2: engine/models still marked [STUB], llama-cpp-2/hf-hub items still marked UNVERIFIED/Inferred despite real implementation now present | open | — | — | loop-02-evidence.md § Unresolved Gaps; docs/codebase-summary.md:16-17,215-219 |
| G10 | 2 | crates/pipeline/src/generate.rs strip_think/parse_and_validate have no committed unit tests (only constrained_softmax in readout.rs does); "never panic" claim verified only via QA's uncommitted ad hoc tests | open | — | — | loop-02-evidence.md § Unresolved Gaps; § Verified Behaviors (15 temp adversarial tests) |
| G11 | 2 | crates/engine/src/lib.rs Engine::load unsafe lifetime-widening (transmute to 'static over Box<LlamaModel>) is correct today but not compiler-enforced — risk if struct fields reordered or model taken by value in a future change | open | — | — | loop-02-evidence.md § Unresolved Gaps; crates/engine/src/lib.rs:69-75 |
