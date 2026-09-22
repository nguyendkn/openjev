# Testing Strategy Research — OpenJev-rs

Scope: split pure-logic vs heavy-model tests, mock LLM inference, property-based testing fit,
CI patterns for llama-cpp-2 native build + GGUF caching. Max-5-call budget; findings below mix
verified web facts with inferred/standard-practice knowledge (labeled).

## 1. Splitting pure-logic unit tests from heavy model integration tests

**Verified:** Cargo has two independent mechanisms and they are NOT equivalent:
- `cfg(feature = "integration")` — conditional **compilation**. Gating a test behind a Cargo
  feature removes it from the binary entirely when the feature is off, so it never even compiles
  (no llama-cpp-2/native-lib link cost) unless you run `cargo test --features integration`.
- `#[ignore]` — the test still compiles and is linked into the test binary, but is skipped at
  **runtime** unless invoked with `cargo test -- --ignored` (or `--include-ignored`). Doc:
  `cfg(not($cond))` removes the test; `cfg_attr($cond, ignore)` only marks it ignored — different
  effect on what appears in `cargo test` output.
  Source: [Different test scopes in Rust](https://blog.frankel.ch/different-test-scopes-rust/), [Cargo Book — Features](https://doc.rust-lang.org/cargo/reference/features.html), [Rust Book — Test Organization](https://doc.rust-lang.org/book/ch11-03-test-organization.html)

**Recommendation for OpenJev-rs (inferred from these facts + standard Rust practice):**
Because linking llama-cpp-2's native C/C++ build is itself costly (not just running the test),
prefer the **feature-gate** approach over `#[ignore]` for anything that needs the real inference
engine — this avoids paying the native-link cost on every `cargo test` in normal dev/CI loops.
Concretely:
- Put pure-logic tests (JSON strip/parse/validate, softmax/normalize math, timing struct
  (de)serialization) as ordinary `#[cfg(test)]` unit tests in each module — no feature gate, run
  on every commit, fast, no native dependency.
- Put model-dependent tests in a separate integration-test file/crate gated by
  `#[cfg(feature = "integration")]` (or a separate Cargo workspace member/test binary that only
  builds when that feature/member is explicitly requested), run only in a dedicated CI job or
  manually. A pre-RFC for native "test groups" exists (not stabilized as of this research), so
  feature-gating remains the standard idiom today. Source: [Pre-RFC: Test groups](https://internals.rust-lang.org/t/pre-rfc-test-groups/18591)
- `#[ignore]` is still useful *within* the integration-feature build for individual very slow
  cases (e.g., the largest 4B model) so devs can opt out further with `--ignored`.

## 2. Mocking the LLM engine for fast unit tests

**Verified:** `mockall` is the de facto standard Rust mocking crate for traits — "well over 100
million downloads," works via `#[automock]` for a single-impl trait or the `mock!` macro for
harder cases. Source: [mockall on crates.io](https://crates.io/crates/mockall), [mockall docs.rs](https://docs.rs/mockall), [asomers/mockall GitHub](https://github.com/asomers/mockall)

**Pattern (inferred, standard in the Rust LLM-tooling ecosystem):** Define a
`trait InferenceEngine { fn logits(&self, ...) -> Result<...>; fn generate(&self, ...) -> Result<String>; }`
(or split into two traits per your two pipelines — constrained-logit readout vs full-JSON
generation). Production code depends on `dyn InferenceEngine` / a generic `impl InferenceEngine`;
tests either:
- use `#[automock]` to generate `MockInferenceEngine` and script `.expect_logits().returning(...)`
  / `.expect_generate().returning(...)` per test case, or
- hand-roll a lightweight fake struct implementing the trait that returns fixed logit vectors or
  canned `<think>`-wrapped JSON strings — often simpler than mockall when you just need fixed
  canned responses rather than call-count/argument assertions.

One example noted in passing: the `llm-stack` crate ships test helpers (`mock_for()`) that queue
canned `ChatResponse`s for LLM call sites — evidence this pattern (trait + injected
mock/fake) is already idiomatic in the Rust LLM ecosystem, not something novel to invent here.
Source: [Rust Ecosystem for AI & LLMs](https://hackmd.io/@Hamze/Hy5LiRV1gg), [llm-stack on crates.io](https://crates.io/crates/llm-stack)

**Uncertain:** I did not verify llm-stack's exact API surface or whether it's a good dependency
fit for OpenJev-rs (likely overkill — a hand-rolled fake behind your own `InferenceEngine` trait
is simpler and has zero extra dependency surface for a tool this scoped). Treat as inspiration,
not a recommended dependency.

## 3. Property-based testing fit (proptest/quickcheck)

**Verified (general facts):** proptest generates randomized inputs and auto-shrinks failures to a
minimal repro; well suited to "code that manipulates data structures, parses input, or implements
algorithms with many edge cases," and explicitly good at finding numeric edge cases (overflow,
boundary values). Source: [proptest-rs/proptest GitHub](https://github.com/proptest-rs/proptest), [LogRocket: Property-based testing in Rust with Proptest](https://blog.logrocket.com/property-based-testing-in-rust-with-proptest/), [docs.rs/proptest](https://docs.rs/proptest)

**Assessment for OpenJev-rs (inferred, applying the above to your two math/parsing surfaces):**

*Softmax/normalize-over-subset-of-logits* — **good fit**. Properties to assert:
- Output values sum to 1.0 (within epsilon) for any non-empty finite `f32`/`f64` input vector.
- All outputs are in `[0, 1]`.
- Monotonicity: larger input logit ⇒ larger (or equal) output probability, relative ordering
  preserved.
- Numerical stability: no NaN/Inf for extreme inputs (very large/very small/negative logits) —
  proptest is exactly the tool for hunting this since it explores boundary magnitudes you
  wouldn't hand-write.
- Edge cases to cover explicitly (as regression unit tests, not just proptest, since proptest
  may not reliably generate size-1/size-0 cases): empty candidate set (should be an explicit
  error, not a panic/NaN), single-candidate set (probability must be exactly 1.0), all-equal
  logits (uniform distribution), one logit far larger than the rest (near-one-hot output).

*`<think>` tag stripping + JSON schema validation* — **moderate fit, mixed with example-based
tests**. proptest is strong for generating malformed/adversarial strings around the parser
(random garbage before/after tags, random valid-looking JSON with wrong types), but the specific
edge cases below are better as fixed example tests since they encode business rules, not general
properties:
- No `<think>` tags present at all (raw JSON only).
- Missing closing `</think>` tag (unterminated — must fail gracefully, not panic/hang).
- Nested/duplicate `<think>` tags.
- `<think>` tags with JSON content *inside* them that should be ignored/stripped along with the
  reasoning text.
- Malformed JSON after stripping (trailing commas, truncated at 512-token cutoff mid-object).
- Valid JSON that doesn't match the expected option/schema (extra/missing keys, wrong types,
  option value not in the allowed candidate set).
- Empty string / whitespace-only input after stripping.
A reasonable proptest property here: "for any string, stripping+parsing either returns `Ok` with
valid-schema JSON, or a structured `Err` — never panics." That fuzz-like property is the main
value-add; the semantic edge cases above should still be explicit unit tests for clarity.

**Unresolved:** whether `quickcheck` offers anything proptest lacks for this use case — not
researched; proptest is more actively maintained and commonly recommended in 2025/2026 Rust
sources found here, so default to it absent a specific reason otherwise.

## 4. CI patterns for llama-cpp-2's native build + GGUF model files

**Verified:**
- `sccache` (Mozilla) is a ccache-like compiler-cache tool that explicitly supports caching C/C++
  compilation as well as Rust, and integrates with GitHub Actions cache backend with minimal
  config via `Mozilla-Actions/sccache-action`, setting `SCCACHE_GHA_ENABLED=true` and
  `RUSTC_WRAPPER=sccache`. This directly addresses avoiding llama-cpp-2's native C/C++ rebuild
  cost on every CI run. Source: [Depot: Fast Rust Builds with sccache and GitHub Actions](https://depot.dev/blog/sccache-in-github-actions), [Mozilla-Actions/sccache-action](https://github.com/Mozilla-Actions/sccache-action), [sccache crates.io](https://crates.io/crates/sccache)
- A real production example (jan.ai) has a PR titled "ci: cache llama.cpp, MLX and Rust builds in
  S3" — confirms other projects using llama.cpp from CI hit this exact problem and solve it via
  external artifact caching (S3 in their case, not plain `actions/cache`). Source: [jan PR #8945](https://github.com/janhq/jan/pull/8945)
- Caveat: each GitHub repo's `actions/cache` total is capped at 10GB, which "fills quickly when
  saving whole copies of the target/ directory" — relevant since GGUF models (hundreds of MB to
  several GB) plus a cached `target/` could blow this budget fast. Source: [Depot sccache blog](https://depot.dev/blog/sccache-in-github-actions)

**Inferred (standard `actions/cache` usage, not specifically verified for this project):**
- `actions/cache` (GitHub's own action) is the standard way to persist arbitrary files (e.g. a
  `models/` directory of downloaded GGUF weights) across CI runs, keyed on a hash (e.g. of a
  models-manifest file listing filenames/URLs/checksums) so the cache is reused until the
  manifest changes. Given the 10GB/repo cap noted above, only cache the **smallest** GGUF model(s)
  actually needed for integration tests, not the full 0.6B–4B range.
- Given the size/cost tension, the more common pattern for CI (inferred, not found in a source
  during this research) is to **not** run true integration tests with real multi-GB models on
  every commit at all, but instead: (a) run pure-logic tests on every commit (see §1), (b) use a
  very small/tiny GGUF test model (e.g. a toy few-MB model or a heavily quantized ~0.5B model) —
  possibly a dedicated tiny fixture model — for a "smoke" integration job that does run in CI, and
  (c) reserve the full 0.6B-4B real-model matrix for a manual/nightly/scheduled workflow rather
  than per-PR CI, combined with `actions/cache` keyed by model checksum to avoid re-downloading
  between scheduled runs.
- This split (fast pure-logic CI gate + separate manual/scheduled heavy-model job) is consistent
  with §1's feature-gate recommendation: CI's default job builds/tests without the `integration`
  feature (no native link needed at all if llama-cpp-2 is only pulled in as an optional dep behind
  that feature); a second CI job or workflow explicitly enables `--features integration` and
  restores the sccache + model caches.

## Unresolved questions

- Whether llama-cpp-2 (the crate) requires the native llama.cpp submodule/source to be compiled
  every time regardless of sccache use, or whether it has a way to depend on a prebuilt/vendored
  binary — not checked against llama-cpp-2's own docs.rs/README in this pass (budget-limited).
- No concrete example found of `actions/cache` specifically caching GGUF model files in a public
  Rust project's CI (the jan.ai example uses S3, not `actions/cache`) — the GGUF-caching
  recommendation above is inferred from general `actions/cache` semantics, not observed practice.
- quickcheck vs proptest trade-off for this specific project not directly compared in sources
  found.
- Whether a tiny/fixture GGUF test model is publicly available and license-clean for CI use was
  not researched.
