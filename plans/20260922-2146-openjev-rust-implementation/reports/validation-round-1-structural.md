### 4.A Structural — FAIL: 2 BLOCKER / 4 MINOR

**§4.A.1 Required sections (plan.md + phase-*.md)** — PASS. plan.md frontmatter (title/description/status/priority/effort/lane/branch/tags/created) present at plan.md:2-10. All 7 phase files (phase-00..06) have frontmatter (phase/name/status/depends_on) and all 13 required sections (Context Links, Overview, Key Insights, Requirements, Architecture, Related Code Files, Implementation Steps, Todo List, Success Criteria, Test Strategy & Quality Gate, Risk Assessment, Security Considerations, Next Steps) — verified present in each file.

**§4.A.2 Cross-reference mismatch (plan.md phase table ↔ phase-XX-*.md)** — PASS. plan.md:94-102 table lists phases 0-6, each has a matching `phase-0N-*.md` file (7/7), and plan.md:128-134 "Phase Files" list matches the 7 files on disk 1:1.

**§4.A.3 Unresolved placeholders (TODO/??/[fill in]/<placeholder>)** — PASS. Grepped whole plan dir for `TODO|\?\?|\[fill in\]|<placeholder>|TBD|XXX` — no matches.

**§4.A.4 Success Criteria bullets with no verifiable condition** — PASS with MINOR notes (see below); every Success Criteria bullet has at least an observable check (exit code, JSON shape, HTTP status, manual sanity value), none are bare assertions.

**§4.A.5 File-ownership overlap (Parallel Execution Matrix vs same-wave phases)** — **FAIL, 2 BLOCKER.**

- **BLOCKER 1** — `src/pipeline/mod.rs` is written by both Phase 2 and Phase 3, which plan.md's wave grouping runs in parallel. plan.md:110-120 Parallel Execution Matrix lists only `src/pipeline/readout.rs` for Phase 2 and `src/pipeline/generate.rs` for Phase 3 — `src/pipeline/mod.rs` is not listed as owned by either, yet phase-02-pipeline-constrained-readout.md:62-63 ("CREATE `src/pipeline/mod.rs` — module root, re-exports `readout`/`generate`...") and phase-03-pipeline-generation.md:67-70 ("MODIFY `src/pipeline/mod.rs` — add `pub mod generate;`... coordinate via plan's parallel-execution note that both phases touch `mod.rs`'s module declarations") both explicitly write to it in the same wave. This directly contradicts plan.md:122 ("Phases 2/3 touch disjoint files (no overlap)"). Violates §4.A.5.

- **BLOCKER 2** — `src/cli/mod.rs` and `src/main.rs` are written by both Phase 4 and Phase 5, run in parallel per plan.md's wave grouping. plan.md:110-120 matrix lists `src/cli/**` for Phase 4 and only `src/server/**` for Phase 5 (main.rs owned by neither). But phase-04-cli-bench.md:67-70 (creates `src/cli/mod.rs` incl. a `Serve` stub "implemented by Phase 5") and :72-73 (MODIFY `src/main.rs`) vs phase-05-http-api-serve.md:68-69 ("MODIFY `src/cli/mod.rs` — add `Serve(ServeArgs)`... or fill in if Phase 4 already added the variant stub") and :70-74 (MODIFY `src/main.rs`, changing `main` to `#[tokio::main] async fn`) — both phases write the same two files concurrently. Contradicts plan.md:123-124 ("Phases 4/5 touch disjoint files (no overlap). Safe to dispatch each wave's phases concurrently."). Violates §4.A.5.

**§4.A.6 Security Considerations section present in every phase (lane-independent requirement)** — PASS. All 7 phase files have a populated "Security Considerations" section (phase-00:114-116, phase-01:148-152, phase-02:121-122, phase-03:144-146, phase-04:133-136, phase-05:151-157, phase-06:176-179). Plan lane is `normal`, so ADR Rationale/high-risk requirement (item 6's other clause) is N/A.

**§4.A.7 Broken internal link/path** — PASS. All in-plan-dir relative references resolve: `research/researcher-01..04-*.md` (4/4 exist), each phase-to-phase reference (phase-00..06, 7/7 exist), `../plan.md` (exists). Out-of-plan-dir citation `docs/research/openjev-rust-research.md` (cited by plan.md:22, phase-00:12, phase-02:10, phase-03:10) resolves to a real file at the project root (`C:\Users\nguyendk\Documents\Projects\openjev\docs\research\openjev-rust-research.md`, confirmed on disk) — not broken.

---

**MINOR findings:**

1. plan.md is 135 lines vs the spec's "under 80 lines" soft target — not wildly over, kept as MINOR not BLOCKER. (plan.md, whole file)
2. phase-00-spike-verification.md:95 Success Criteria bullet 3 ("Phase 1 can start without re-deriving any of these facts") is a consequence statement, not an independently observable check — the other two bullets in the same section (:92-94) are concretely verifiable, so this one is a soft/MINOR gap only.
3. phase-02-pipeline-constrained-readout.md:91-93 Success Criteria — "should favor B strongly" has no defined numeric threshold for "strongly" (manual sanity check, explicitly labeled as such). MINOR.
4. phase-03-pipeline-generation.md:110-111 Success Criteria — "verify via a forced-long-output test case if feasible, or code inspection..." — conditional wording weakens verifiability but a fallback observable check (code inspection of the loop-counter condition) exists. MINOR.
