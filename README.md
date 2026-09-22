# OpenJev-RS

**Status: Phase 0 Spike (skeleton-only, not yet functional)**

A Rust port of [OpenJev/SemIf](https://github.com/workszop/openjev)—a CPU-only benchmark tool comparing three methods of answering multiple-choice questions with local LLMs. Measures latency per inference stage and captures relative calibration differences across scoring paths. Dual deployment: command-line one-shot runner (`openjev-cli`) and HTTP API server (`openjev-server`/axum) on a shared library stack.

---

## Three Scoring Methods

OpenJev-RS compares these independent inference paths on the same MCQ dataset:

1. **Constrained Single-Token Readout** — One forward pass, logits restricted to option-label token IDs (before softmax), fast baseline. Typical latency: <50ms per decision.

2. **JSON Generation** — Greedy decoding (max 512 tokens), strip reasoning tags (`<think>...</think>`), parse and validate JSON schema, flexible multi-token reasoning. Typical latency: <500ms per decision.

3. **Laya Single-Pass Encoder** — External subprocess (`ggmlc-run` CLI) running ModernBERT for calibrated probability estimates. Typical latency: <150ms per decision (CPU; GPU ~25ms).

Each runs against all three models (Qwen3-0.6B, MiniCPM-2B, Qwen3-4B), producing a 3×3 latency matrix.

---

## Quick Start

### Prerequisites
- Rust 1.70+ (toolchain pinned in `rust-toolchain.toml`)
- 10+ GB free disk (model cache)
- Linux (recommended target) or Windows (development supported)

### Build All Crates
```bash
cargo build --workspace
```

### Run CLI (One-Shot Benchmark)
```bash
cargo run -p cli -- --help
```
or via Makefile:
```bash
make bench
```

### Run HTTP Server
```bash
cargo run -p server -- --help
```
or:
```bash
make serve
```

### Run Tests
```bash
cargo test --workspace
```

See `Makefile` for additional targets (`fmt`, `clippy`, `ci` suite).

---

## Project Status

**Current phase:** Phase 0 (spike verification) — ongoing.

**What exists:**
- Workspace skeleton: 6-crate structure (timing, models, engine, pipeline, cli, server) all declared in `Cargo.toml`.
- `crates/timing`: Complete `Timings` struct (27 lines) capturing 7-stage wall-clock measurements.
- Phase 0 verification: llama-cpp-2 API shape, hf-hub behavior, model repository IDs, and Laya subprocess integration all under validation (see `docs/project-overview-pdr.md` §8 for 8 open items).

**What's NOT working yet:**
- Model download and loading (Phase 1).
- Inference pipelines: readout, generation, Laya (Phases 2–3).
- CLI and server binaries are stubs (Phases 4–5).
- Tests and deployment (Phases 6–7).

**Do NOT expect** to run benchmarks yet. Current binaries print placeholder output.

---

## Documentation

Four authoritative docs guide development and architecture:

- **[`docs/project-overview-pdr.md`](docs/project-overview-pdr.md)** — Problem statement, goals, functional + non-functional requirements, 7-phase roadmap with acceptance criteria, and Phase 0 spike items (required blockers before Phase 1).

- **[`docs/codebase-summary.md`](docs/codebase-summary.md)** — Workspace layout, per-crate state (status table), dependency graph, build/run commands, and what's NOT implemented yet.

- **[`docs/code-standards.md`](docs/code-standards.md)** — Locked engineering rules: error handling patterns, API design conventions, testing strategy (≥90% line, ≥75% branch coverage), CPU-only constraint, and Cargo discipline.

- **[`docs/system-architecture.md`](docs/system-architecture.md)** — End-to-end pipeline flows for all three scoring methods, request/response shapes (CLI + HTTP), external integrations (Hugging Face hf-hub, subprocess ggmlc-run), timing breakdown per stage, and deployment topology (dev/prod).

Start with `project-overview-pdr.md` to understand the scope and Phase 0 blockers.

---

## 7-Phase Roadmap

| Phase | Title | Goal | Est. Duration |
|-------|-------|------|---|
| **0** | Spike Verification | Resolve 8 unresolved items (model IDs, API shapes, licensing) via primary source. | ~2 days |
| **1** | Engine / Models / Timing | Load models via llama-cpp-2, download + cache via hf-hub, populate `Timings` struct. | ~3 days |
| **2** | Pipeline — Readout + Laya | Constrained-softmax over options, Laya shell-out subprocess wrapper. | Parallel with Phase 3 |
| **3** | Pipeline — Generation | Greedy 512-token decode, strip `<think>` tags, JSON schema validation. | Parallel with Phase 2 |
| **4** | CLI Binary | Parse args, orchestrate 3 pipelines, JSON/text output, exit codes. | Parallel with Phase 5 |
| **5** | HTTP Server | axum routes (`POST /bench`, `GET /health`), Arc<Mutex> serialization, spawn_blocking. | Parallel with Phase 4 |
| **6** | Tests + CI | Unit + integration coverage ≥90%/≥75%, GitHub Actions (Windows dev, Linux prod). | Parallel with Phase 7 |
| **7** | E2E + Tuning | Deploy to Linux server, tune n_threads + batch_size, measure baseline latencies, document results. | Parallel with Phase 6 |

**Critical path:** Phase 0 → Phase 1 → {Phase 2–3} → {Phase 4–5} → {Phase 6–7}.

See `plans/20260922-2146-openjev-rust-implementation/plan.md` for per-phase details.

---

## Development

**Format & lint:**
```bash
cargo fmt --all
cargo clippy --workspace -- -D warnings
```

**Pre-commit check (CI suite):**
```bash
make ci
```

**Code standards:** See [`docs/code-standards.md`](docs/code-standards.md) § 1–10 for naming, error handling, testing rules, and locked MUST/MUST NOT constraints (CPU-only, no mocks, structured errors).

---

## Next Steps

1. **Phase 0 completion:** Verify 8 open items and close Phase 0 spike before Phase 1 starts (blocking).
2. **Phase 1 implementation:** Engine + models setup, real GGUF loading, hf-hub integration.
3. **Feedback:** Methodology, architecture, or design concerns? Open an issue or PR.

---

## License

Apache-2.0 (pending verification per model repo licenses; see `docs/project-overview-pdr.md` § Open Risks § 5).

**Attribution:** Rust implementation by Claude Haiku 4.5. Original OpenJev/SemIf browser tool: [workszop/openjev](https://github.com/workszop/openjev).
