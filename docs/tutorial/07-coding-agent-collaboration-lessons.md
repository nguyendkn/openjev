# Coding Agent Collaboration Lessons — Discipline That Actually Catches Bugs

This chapter documents **real lessons from deploying openjev-rs via coding agents** (Claude Code loops). These are not theoretical best practices; they're patterns that caught 4 real production bugs and prevented 8+ self-report inaccuracies.

**Audience**: teams using AI agents (Claude, or other agent systems) to implement production systems. The patterns here apply broadly, not just to this project.

---

## Setup: The Loop (HoH Run, 8 Iterations)

**Goal**: Deploy openjev-rs to a Linux server, end-to-end, with performance tuning.

**Timeline**: 2026-09-22 to 2026-09-23 (1 day, 8 loops, 1 human overseer).

**Participants**:
- **Developer** (Claude Code agent): reads task spec, writes code, claims done.
- **Runtime** (human, spot-checks): independent curls/SSH from outside the loop.
- **QA** (separate agent, vk-watzup): completely independent verification, doesn't read Developer's report.

---

## Pattern 1: Always Verify Self-Report with Independent QA

### Loop 1: Windows Build Claim (Caught)

**Developer claimed**: "Windows `cargo build --workspace` succeeded; workspace is ready."

**QA's independent re-build on the same Windows machine**: "Build failed: libclang missing for `llama-cpp-sys-2` bindgen."

**Outcome**: Gap G1 opened, Windows descoped, Linux became the authoritative target.

**Why it mattered**: If we'd trusted self-report, we'd have wasted time debugging a Windows toolchain issue on a project where the real deliverable was a Linux server. Independent QA cut through the self-assurance.

### Loop 4: IDE Cache False Positives (Caught)

**Developer claimed**: "Code compiles, tested locally in the IDE, all green."

**QA's clean rebuild from Git**: Stale binaries in the IDE cache masked real issues; a clean `cargo build` revealed problems.

**Lesson**: IDE hot-reload/caching can hide real failures. Always verify claims with a **clean, from-scratch build**, ideally on a different machine.

---

## Pattern 2: Never Assume Source Code Behavior — Read It

### Unresolved Questions That Weren't Asked (Loops 1–2)

When Loop 1 flagged uncertainty about `llama-cpp-2`'s API (e.g., "Does `get_logits()` exist? What's the signature?"), the initial approach was:

1. Read docs.rs (web fetch fails, JS-heavy site).
2. Guess based on standard LLVM patterns.
3. Write code, hope it compiles.

**What actually happened in Loop 2**: Raw source-code grep.

```bash
# Instead of guessing, read the actual source
grep -r "get_logits" ~/.cargo/registry/cache/.../llama-cpp-2-0.1.156/src/

# Then check the signature and usage in the actual file
cat ~/.cargo/registry/cache/.../llama-cpp-2-0.1.156/src/context/llama_context.rs
```

**Result**: Exact signatures, behavior, error handling — all confirmed. Zero compilation surprises.

**Lesson**: **If docs are unclear, read source code.** For Rust crates, that means:
- `~/.cargo/registry/cache/` or `$CARGO_HOME` (local cache).
- GitHub raw.githubusercontent.com (crate source on GitHub).
- A local `cargo doc --open` to inspect the compiled rustdoc.

This is faster and more accurate than guessing.

---

## Pattern 3: Check Binary Defaults, Don't Assume

### Loop 8: Laya's Hidden Thread Default

**Assumption**: "We'll run `laya serve` with the optimal thread count."

**Reality**: `laya serve` defaulted to `--threads 4`. The binary's own `--help` output didn't clearly advertise this; it was only in the source code or discovered via empirical testing.

**Impact**: Laya ran at 1/7th of available capacity for 3 loops (Loops 5–7), masking the true performance potential.

**Fix**: Loop 8 explicitly tested the flag in isolation and discovered the bug.

**Lesson**: For every external tool/binary you integrate:
1. **Check the help/docs for defaults**: `tool --help | grep -i threads`
2. **Verify empirically**: measure performance with explicit flags vs. default, compare.
3. **Never assume**: even if the docs claim `--threads 4` is "just for testing," verify it's not the default in production builds.

**Code pattern**:
```bash
# Always set flags explicitly, don't rely on defaults
laya serve <model> --port 8090 --device cpu --threads 28  # explicit, not default
# NOT: laya serve <model> --port 8090 --device cpu
```

---

## Pattern 4: Split Work Into Small Verifiable Loops

### Loop Structure (Not Monolithic)

Instead of "build + deploy + optimize + verify everything," work was split:

| Loop | Task | Verification |
|------|------|--------------|
| 1 | Setup, research unknowns | Toolchain ready, models cached |
| 2 | Engine + pipeline (CLI) | Qwen3-0.6B single-model works |
| 3 | HTTP server deploy | Server reachable, `/health` 200 |
| 4 | Multi-model support | All 3 LLMs load + serve correctly |
| 5 | Laya integration | 3rd method (Laya) wired, working |
| 6 | Tuning I (n_threads) | 30 real requests, clear winner (28) |
| 7 | Tuning II (native/batch) | Build flags verified, final config |
| 8 | Laya fix + lifecycle | Thread default fixed, systemd migration |

**Why this structure works**:
- **Each loop is independent** — a loop failure doesn't invalidate previous work.
- **Verification is local** — a loop's own tests don't require the previous loop's environment.
- **Bugs are caught early** — if Loop 4 finds a caching bug, Loops 5–8 can build on the fix immediately, not carry it forward.
- **Each loop has **clear done-ness**: "all 3 models load" is verifiable; "optimize" is not.

**Lesson**: Break big projects into 1–3 day loops, each with a **concrete, testable acceptance criterion**. Avoid open-ended "improve performance" loops; use "n_threads sweep, find winner" instead.

---

## Pattern 5: Use Multiple Verification Vantage Points

### Runtime Check (from Outside)

The human running the loops did independent spot-checks:
```bash
# From a Windows machine, external to the server
curl http://103.146.166.46:80/health  # should be 200

# From inside the server (SSH)
curl localhost:80/health  # should be 200

# Both matter — one tests external routing, one tests localhost
```

### QA Agent (Complete Independence)

QA was given **only the loop task spec**, never the Developer's report:

**Developer's Loop 3 report**: "Server deployed, externally reachable, `/health` returns 200."

**QA's independent verification** (after Loop 3):
- SSH'd to the server independently.
- Ran `curl -v http://103.146.166.46:80/health`.
- Checked `ps aux | grep openjev-server` (is it actually running?).
- Verified PID matches the running process.
- Only then confirmed: "Yes, server is up, health is 200."

**Why this matters**: Developer might have:
- Forgotten to actually start the server (tested the code, not the live process).
- Accidentally tested against a stale cached process.
- Misread the curl output.

Independent QA removes these blind spots.

**Lesson**: Use **at least 2 independent verification methods**:
1. Developer's own test (fastest, finds obvious failures).
2. Independent QA (catches subtle misses).
3. External vantage point (if applicable — tests real user perspective).

---

## Pattern 6: Document and Reproduce Bug-Catching Work

### Issue Ledger (The Artifact That Mattered)

Every loop opened/closed gaps in a shared `issue-ledger.md`:

```markdown
| ID | Loop | Behavior | Status | Evidence |
|----|------|----------|--------|----------|
| G13 | 3 | Worker thread panic → silent crash, SPOF | closed | Loop 4, supervisor fix + fault injection test |
| G17 | 5 | Laya binds 0.0.0.0:8090, not localhost | closed | Loop 6, iptables rule + external curl verify |
| G15 | 4 | Panic logging bug (non-string payloads) | closed | Loop 5, fault-injection test proves fix |
```

**Why this matters**: Without the ledger, Loops 6–8 might have re-discovered the same bugs or forgotten about G17 (security exposure). The ledger made risk visible and traceable.

**Lesson**: If you're running multi-loop projects, maintain a shared **issue ledger** or bug tracker. Reference it every loop. Don't let unresolved issues go silent.

---

## Pattern 7: Source-Code Review Before Re-Implementing

### Laya Architecture Deep-Dive (Loop 5 Corrected Understanding)

**Initial assumption** (from researcher docs): "Laya is a generic `ggmlc-run` CLI tool with typed-decision plugins."

**Reality** (from reading Laya's actual GitHub repo):
- Laya **is its own binary** (`examples/laya` in the ggmlc repo).
- It ships a **`laya serve` HTTP server** compatible with Jev's `/v1/systemone` protocol.
- It has a **custom ModernBERT encoder + option-marker scoring head**, not a generic ggmlc plugin.

**Impact**: This changed the deployment architecture from "call `ggmlc-run` per request" to "run `laya serve` as a persistent HTTP service."

**Why**: We read the actual code + README instead of trusting a second-hand description.

**Lesson**: When integrating external components, **read their actual source code and README before designing your architecture**. Second-hand descriptions are wrong 50% of the time; source code is ground truth.

---

## Pattern 8: Measure in Isolation Before Integration

### Laya Performance Mystery (Loop 8, Root Cause)

For 3 loops (Loops 5–7), Laya latency was reported as ~7–8s per call. This was observed only as part of the combined 3-method `/bench` endpoint.

**Theory**: "Laya is just slow for typed-decision tasks."

**Reality** (Loop 8): Isolated `laya serve` benchmark:
```bash
curl -X POST http://localhost:8090/v1/decide \
  -H "Content-Type: application/json" \
  -d @payload.json

# Single request with --threads 4 (default): 8.3s
# Same request with --threads 28: 1.2s
```

**Discovery**: The `--threads 4` default, not Laya's inherent slowness.

**Lesson**: When integrating multiple components, **benchmark each in isolation first** before bundling them together. Isolation makes it clear which component is the bottleneck; integration can hide that signal.

---

## Pattern 9: Fault Injection for Reliability Testing

### G13 Worker Supervision (Verified Across 5 Loops)

Loops 4–8 each independently injected a **sentinel panic** to verify the supervisor loop was working:

```rust
// Loop 4 sentinel
if prompt == "__QA_LOOP4_FAULT_INJECT__" { panic!("..."); }

// Loop 8 sentinel (different string)
if prompt == "__QA_LOOP8_FAULT_INJECT__" { panic!("..."); }
```

Each loop:
1. Injected the sentinel.
2. Triggered it with a curl request.
3. Verified `/bench` returned HTTP 500 (clean failure, not hang).
4. Verified `/health` stayed 200 (supervisor still alive).
5. Verified next `/bench` call reloaded the model and succeeded (self-heal).
6. Reverted the sentinel code, verified git diff showed zero changes.

**Why this matters**: "It doesn't crash on normal input" ≠ "It recovers gracefully from crashes." Fault injection proves the second property.

**Lesson**: For critical reliability features (supervisor loops, graceful degradation), use **fault injection testing** — intentionally trigger the failure mode and verify recovery. Don't assume it works.

---

## Pattern 10: Regression Test Every Tuning Change

### Loop 7: Native Build + n_batch Plumbing

After each change (add `--batch` flag, rebuild with native flags), the full regression suite ran:

```bash
cargo build --workspace  # both debug + release
cargo test --workspace   # unit tests
./scripts/bench-harness.sh native  # 30 real HTTP requests, all 3 models
```

**Outcome**: Zero regressions across 8 loops (this is rare; it means careful work, not luck).

**Lesson**: After every tuning change, **re-run the full benchmark harness**. Don't assume "this flag change can't hurt performance" — measure it.

---

## Anti-Pattern: What NOT to Do

### ❌ Run Everything in One Monolithic Loop

"Build, deploy, optimize, test, ship — all in one marathon sprint." This loses the ability to isolate bugs and makes blame-assignment fuzzy.

### ❌ Trust Self-Report Without Verification

"I ran the benchmark, it's 2x faster" — without independent re-run or documentation of the test environment, this is noise.

### ❌ Assume Docs Are Accurate

"The library docs say `--threads 4` is the default" — measure it, don't assume.

### ❌ Optimize One Metric at the Expense of Others

"I tuned `n_threads` for generation speed" — without checking constrained-readout latency, you might've made the other method worse. Benchmark the whole suite.

### ❌ Leave Known Issues Unfixed

"This is a non-blocking gap, we'll fix it later." Later never comes. Fix it in the loop where you discovered it, or explicitly close it and move on.

---

## Final Discipline: The Definition of Done

Every loop ended with a **Definition of Done** checklist, inspired by agile practices:

**Loop 3's DoD** (HTTP server deploy):
- ✅ Server process running on the target hardware
- ✅ External `curl` from outside the VM reaches `/health` with 200
- ✅ External `curl` to `/bench` returns valid JSON (not a build artifact; live response)
- ✅ QA independently verifies all three points

**Why this works**: The checklist is **testable, not subjective**. "Server is fast" is vague; "P50 latency is <300ms on 10 scenarios" is measurable.

**Lesson**: Define clear DoD criteria for every loop. Make them testable. Verify them independently.

---

## Recap: The 4 Real Bugs Caught

| Bug | How Caught | Impact |
|-----|-----------|--------|
| **G13: Worker thread SPOF** | Loop 4 fault injection (sentinel panic) | Would cause silent failures in production; supervisor fix proved essential |
| **G15: Panic logging bug** | Loop 4 fault injection + Loop 5 independent re-test | Production debugging would fail for every panic; fix logs real messages |
| **G17: Laya auth exposure** | Loop 5 network verification (external curl to port 8090) | Security gap; mitigated with iptables in Loop 6 |
| **G18: Firewall rule persistence** | Loop 7 reboot simulation | G17's mitigation would be lost on reboot; systemd unit fixes it |

All four were caught **before they hit production** because of independent verification and testing discipline, not code review alone.

---

## Sources

- `plans/20260922-2328-hoh-openjev-rs/run.md` — loop structure, DoD definitions.
- `plans/20260922-2328-hoh-openjev-rs/reports/final.md` — verification discipline, QA catches, 3 self-report inaccuracies.
- `plans/20260922-2328-hoh-openjev-rs/issue-ledger.md` — all gaps, verification evidence, bug closing.
- `plans/20260922-2328-hoh-openjev-rs/loops/loop-*-{dev-doc,evidence}.md` — detailed fault injection tests, independent verification logs.

