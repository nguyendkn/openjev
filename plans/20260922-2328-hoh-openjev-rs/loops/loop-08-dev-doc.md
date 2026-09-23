---
loop: 8
status: pending
preservation_constraints:
  - "openjev-server (port 80) and laya serve (port 8090) both stay live and reachable throughout"
  - "All 3 LLM models + Laya still correct via apps/cli and apps/server"
  - "reset_context / G13 respawn-supervisor / Laya graceful degradation / G17+G18 firewall all still functional"
  - "cargo build --workspace (debug+release) and cargo test --workspace still pass"
  - "n_threads=28, native build, n_batch=512 (Loop 6-7's winning LLM config) unchanged unless this loop finds solid evidence otherwise"
---

## Objective
Persist+prove the Laya `--threads` fix (found live: 4→28 threads = 5x speedup, 8350ms→1665ms); push further toward Jev's speed class.

**MAJOR FINDING (main agent, live on the server, already applied)**: `laya serve` was deployed
in every prior loop (5-7) WITHOUT a `--threads` flag. The `laya` binary's own `--help` states
`--threads <N>  CPU workers  (default: 4)` for BOTH `decide` and `serve` subcommands — so Laya
was running on just 4 of the server's 32 cores this whole time. Manually restarting with
`--threads 28` (matching the LLM server's proven-optimal thread count) cut a single measured
request from **8350ms to 1665ms — a 5x improvement** (tested twice: 28→1665ms, 32→1625ms,
16→2511ms, confirming 28 is near-optimal, consistent with the LLM tuning pattern from Loop 6).
This was a real, previously-undiscovered deployment bug (not caught in Loops 5-7 because those
loops focused on correctness/functionality, not performance — Laya's speed was never
benchmarked in isolation, only as part of the combined 3-method `/bench` timing where it was
lumped in as "just how slow Laya is").

This loop must: (a) make the fix PERMANENT (not just a manual SSH restart that regresses on
next reboot/redeploy), (b) get statistically solid before/after numbers via the real harness
(not just 1-2 manual curl calls), (c) push further if there's more headroom, (d) research
Jev's actual target latency numbers (parallel research task, results may land mid-loop — check
`plans/20260922-2146-openjev-rust-implementation/research/researcher-07-jev-benchmark-target.md`
if present) and assess how close we now are.

## Tasks
1. **Persist the fix**: find every place the `laya serve` startup command is documented/scripted
   (README.md, docs/project-overview-pdr.md, docs/codebase-summary.md, the systemd unit from
   G18 if it also starts the process — check `/etc/systemd/system/openjev-laya-firewall.service`,
   it currently only manages the iptables rule, NOT process startup, per Loop 7's own scope
   note — consider whether it SHOULD also manage `laya serve`'s lifecycle now that we know a
   wrong flag causes a silent 5x perf regression that's easy to miss). Add `--threads 28` to
   every documented/scripted invocation. If a startup script doesn't exist yet (just manual
   `setsid nohup ...` commands typed ad-hoc across loops), consider creating one
   (`scripts/start-laya-serve.sh` or similar) so future restarts can't accidentally regress this.
2. **Re-run the benchmark harness** (`scripts/bench-harness.sh`) with `laya serve --threads 28`
   for all 3 LLM models × 10 scenarios (30 requests) — get real aggregated
   `laya_inference_ms` mean/median/max, compare directly against Loop 7's `native-summary.json`
   baseline (which had the OLD threads=4 Laya numbers, ~6.5-7.5s mean). Save as
   `docs/benchmarks/laya-threads28-{raw.jsonl,summary.json}`.
3. **Check for more Laya headroom**: the quantization used is `UD_Q4_K_M` (smallest/fastest per
   the model card's own framing) — verify this is actually still the fastest CPU quant by
   testing whether `Q8_0` or `F16` might paradoxically be faster on CPU (quantized formats can
   have slower CPU dequant paths without well-optimized kernels; test at least one alternative
   quant if time permits — download it, run the same single-request timing test, compare; if
   `UD_Q4_K_M` wins, keep it and document the check; if not, this could be another real win).
4. **Re-verify constrained-readout is genuinely competitive**: Loop 7's numbers showed
   112-291ms mean across the 3 models — already within Jev's claimed 70-500ms range. Confirm
   this holds with the fresh harness run (Task 2 already captures this), and note it explicitly
   in the results doc as "already Jev-competitive" — don't let this get lost among the Laya
   fix's bigger news.
5. **Honestly reframe JSON-generation vs Jev**: `generate.rs`'s pipeline does full
   autoregressive token-by-token JSON generation — this is architecturally NOT what Jev does
   (Jev/Laya never generate tokens, they score in one pass). Add a clear note to
   `docs/benchmarks/server-tuning-results.md` and/or `README.md` stating this explicitly: the
   generation method is `openjev-rs`'s OWN 2nd comparison arm (inherited from the original
   SemIf/OpenJev methodology, predating Laya's addition), not a Jev-equivalent — it will never
   match Jev's speed class by design, and that's expected, not a bug to chase.
6. **Full regression pass**: all 3 LLM models × 3 methods, CLI + external curl, G13
   fault-injection re-test (yet again — 5th consecutive loop, cheap insurance), Laya-outage
   graceful degrade re-test.
7. **Update `docs/benchmarks/server-tuning-results.md`** with a clear "Loop 8: Laya thread-count
   fix" section — before/after numbers, root cause, the quant-check result, and an updated
   final summary table comparing ALL of: Jev's claimed range, constrained-readout (ours),
   Laya (ours, before and after this loop's fix), generation (ours, explicitly marked
   not-comparable).

## Preservation
See frontmatter.

## Validation Requirements
- Given the persisted fix, When `laya serve`'s actual running process is inspected (`ps aux`)
  after this loop, Then it shows `--threads 28` (or whatever final value Task 3 settles on),
  not the default 4.
- Given the re-run harness, When compared to Loop 7's `native-summary.json` Laya numbers, Then
  `laya_inference_ms` mean/median drops by roughly the same order of magnitude as the manual
  spot-check (8350ms→1665ms, ~5x) — a real, harness-measured improvement, not just 1 lucky
  sample.
- Given the loop ends, When `curl http://103.146.166.46:80/health` and a full `/bench` call
  (all 3 methods) are made, Then the server is live and correct, and the response's
  `laya_inference_ms` field reflects the new fast number.

## Out-of-scope
- Rewriting `pipeline::generate` to somehow match Jev's speed — architecturally impossible by
  design (token-by-token generation vs single-pass scoring), explicitly documented as such
  instead.
- Further n_threads/batch/build-flag tuning for the LLM pipelines — Loop 6-7 already converged
  on those, not this loop's focus unless something regressed.
- G10/G12/G14/G16 — unchanged, not this loop's focus.
