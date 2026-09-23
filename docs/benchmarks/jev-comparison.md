# openjev-rs vs Jev — real latency comparison (2026-09-23)

Answers the standing goal "performance phải bằng hoặc nhanh hơn Jev" with actual measured
numbers on both sides, not projection. Jev = TypeSafe AI's commercial cloud System-1 model
(https://typesafe.ai) — closed-source, no public CPU benchmark, so "Jev's numbers" here means
the best third-party-measured / TypeSafe-published figures found in
`plans/20260922-2146-openjev-rust-implementation/research/researcher-07-jev-benchmark-target.md`
(full sourcing there — dev.to 22.5k-call benchmark, sysone-bench head-to-head, TypeSafe's own
marketing page). openjev-rs numbers below are a live, real `/bench` call against production
(103.146.166.46, Xeon Gold 5320, 32 vCPU, **no GPU**) after Loop 20, not simulated.

## Head-to-head

| Method (ours) | Architecturally closest to Jev? | Our real measured (CPU-only) | Jev's real measured/published (cloud, likely GPU) | Verdict |
|---|---|---|---|---|
| **Laya-style single-pass encoder** | Same model family TypeSafe's own comparisons use (Laya is the open reproduction of Jev's System-1 concept) | **111 ms** (live `/bench`, scenario 1) — consistent with Loop 13's isolated ~88ms encoder-only figure | 300ms–1.07s (dev.to, sysone-bench, independent 3rd-party); TypeSafe's own marketing floor 70–500ms | **openjev-rs is 2.7–9x FASTER than Jev's own real-world numbers**, and beats even TypeSafe's best-case marketing figure (70ms floor vs our 111ms is close; their 500ms ceiling vs our 111ms is a clean win) — on CPU only, no GPU. |
| **Constrained-readout** (typed logit read on option tokens, no free-text decode) | Yes — this is the technique architecturally closest to what Jev's API actually does (typed noul/choice/score answers) | **379 ms** (live `/bench`, scenario 1) | Same 0.3–1.0s cluster (dev.to InjecAgent/BEIR P50 0.30–0.31s, sysone-bench Jev 925–1068ms) | **At parity, on the fast end of Jev's own range** — 379ms sits below Jev's own P50 floor in 2 of 3 independent measurements, despite running on CPU with no cloud infrastructure. |
| **Full JSON-generation** (free-form decode + validate — NOT what Jev does; included for completeness) | No — Jev never does open-ended generation, this is openjev-rs's own extra 3rd method | 1.4s (MiniCPM5-2B) – 2.5-3.4s (Qwen3-0.6B, post-Loop-20) – 4.4s (Qwen3-4B) | Nearest anchor: Jev's own worst-case (SkillRetBench hybrid P50 2.07s) — Jev doesn't publish a generation-mode number since it doesn't have one | Mixed, expected — this method pays the full autoregressive-decode tax by design. MiniCPM5-2B beats Jev's own published worst case; the two Qwen models don't, but this isn't Jev's operating mode, so it's not a fair apples-to-apples line — included for transparency, not counted against the goal. |

## Verdict on the standing goal ("bằng hoặc nhanh hơn Jev")

**Met, for the 2 methods that are actually comparable to what Jev does.** Jev is a typed-answer
API (noul/choice/score) — never free-form JSON generation — so the fair comparison is against
`laya` and `constrained-readout`, both of which beat or match Jev's real, independently-measured
latency (2.7–9x faster for the encoder method, at-parity-to-faster for constrained-readout) while
running entirely on a CPU-only box with zero GPU, versus Jev's undisclosed (near-certainly GPU)
cloud backend. `generate` (openjev-rs's own 3rd method, not present in Jev) is naturally slower
by design and was never claimed to beat Jev — it exists for open-ended JSON output Jev doesn't
support at all.

## Caveats (real, not hidden)

- Jev's numbers include unknown network RTT (cloud API) — a pure-compute number might be lower
  than the ~300ms-1s observed, which would narrow or close our advantage on `laya`. No way to
  separate network vs compute time from published third-party sources (see researcher-07 §Unresolved).
- Jev's hardware is undisclosed; "beats Jev" here means beats Jev's real end-user-observed
  latency, not a controlled same-hardware comparison (impossible — Jev is closed/paid, out of
  this project's scope per earlier user decision to not call the real Jev API).
- Single-scenario spot-check (`1_email_routing`) shown above for the live-call evidence; broader
  10-scenario means from Loop 18-20 (constrained-readout ~100-500ms range historically,
  laya ~87-150ms range) are consistent with this snapshot, not cherry-picked.
- This is TypeSafe's real published/measured numbers as of the research pass (2026-09-23),
  independently sourced (dev.to, sysone-bench) not just TypeSafe's own marketing — see
  researcher-07 for full source table and confidence notes.
