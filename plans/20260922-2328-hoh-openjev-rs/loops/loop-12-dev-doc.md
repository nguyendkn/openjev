---
loop: 12
status: pending
preservation_constraints:
  - "openjev-server (port 80) stays live and correct throughout — only laya serve's binary changes this loop"
  - "All 3 LLM models unaffected"
  - "reset_context / G13 respawn-supervisor / graceful Laya-outage degradation all still functional"
  - "G17/G18 (firewall blocking external :8090, systemd persistence) unaffected"
  - "cargo build --workspace (debug+release) and cargo test --workspace still pass"
---

## Objective
Rebuild+deploy production `laya`/`ggmlc` with proper optimization flags (the REAL fix for the
speed goal); decide `crates/laya-native`'s fate based on real evidence; final documentation.

**Pivotal finding from Loop 11** (independently re-verified by the Runtime): production's
`laya serve` was built with `CMAKE_BUILD_TYPE=""` (empty = unoptimized, effectively `-O0`) since
Loop 5 — nobody had checked this until Loop 11 compared against a properly-optimized (`-O3`)
rebuild of the exact same `ggmlc` source and found it ALSO gets ~92.7ms (vs production's
~1262ms), matching `crates/laya-native`'s own 95.1ms almost exactly. **The native Rust rewrite
does not outperform correctly-built C++.** Nearly the entire "10-13x" speedup story from Loops
9-11 was a production build misconfiguration, not an architectural advantage of hand-rolled
code. This is a simpler, lower-risk fix than a production cutover to experimental Rust code —
take it.

## Tasks
1. **Rebuild `ggmlc` in the PRODUCTION tree with proper optimization**: reconfigure/rebuild
   `/tmp/ggmlc/build` (or a fresh build dir swapped in atomically) with `-DCMAKE_BUILD_TYPE=Release`
   (or explicit `-O3`, whichever `ggmlc`'s CMakeLists actually honors — verify, don't assume)
   AND `-DGGML_NATIVE=ON` (Loop 11 found this also matters for numerical consistency, not just
   speed — keep it aligned with `crates/engine`'s existing `-C target-cpu=native` build for
   consistency across the whole project). Produce a new `laya` binary.
2. **Deploy carefully**: this changes the actual binary the `openjev-laya-serve.service` systemd
   unit runs. Back up the old binary path first. Swap in the new one, restart the service,
   verify `/health` and a real `/v1/decide` call before considering this done. If anything goes
   wrong, the backup lets you roll back immediately — don't leave Laya down.
3. **Re-verify correctness on the NEW production binary**: re-run the same 7+ benchmark
   scenarios Loop 11 used, confirm the new production binary's answers match what Loop 11
   already validated for the `-O3` rebuild (should be identical, since it's the same
   optimization level) — don't just trust it carries over, check it for real on the actual
   swapped-in binary.
4. **Re-run the full benchmark harness** (`scripts/bench-harness.sh`) against the corrected
   production Laya, get final real numbers, update `docs/benchmarks/server-tuning-results.md`
   with a clear final section: root cause (unoptimized build), the fix, before/after numbers,
   and the honest verdict on `crates/laya-native` (does not outperform correctly-built C++;
   kept in the repo as a validated reference implementation / learning artifact, NOT wired into
   production — state this explicitly as a deliberate decision with the evidence behind it, not
   as giving up).
5. **`crates/laya-native` cleanup**: remove the extensive debug-only env-gated scaffolding
   (`LAYA_RS_DUMP`/`RS_FULL`/`PERTURB`/`INJECT_HIDDEN`/`LEAK_CHECK`/`CASE`/`TENSOR_*`) that Loop
   11 correctly left in for its own debugging, OR keep it but clearly document each one's
   purpose in a module-level doc comment — Runtime's call: KEEP it (it's genuinely useful
   reference material given how hard this was to get right, and it's inert / env-gated / never
   touched by production code), but add a top-of-file comment in `debug.rs` explaining this
   crate's status: "validated reference implementation, not used in production; see
   server-tuning-results.md for why."
6. **Update `docs/tutorial/`** (particularly `05-benchmark-and-performance.md` and
   `06-deployment-best-practices.md`) with this finding — it's a genuinely important lesson
   ("check your build flags before concluding an architecture is slow" / "always benchmark
   against a correctly-configured baseline before attributing a win to your own cleverness").
7. **Final full regression + production health check**: all 3 LLM models × 3 methods (readout,
   generate, laya) via both `apps/cli` and external curl; G13 fault-injection re-test (yet
   again — cheap insurance); Laya-outage graceful degrade re-test; confirm G17 (external :8090
   still blocked) and G18 (firewall persists) unaffected by the binary swap.

## Preservation
See frontmatter.

## Validation Requirements
- Given the rebuilt `laya` binary deployed, When `curl -X POST http://103.146.166.46:80/bench`
  is called externally (not SSH), Then `laya_inference_ms` reflects the ~90-100ms class latency
  (not the old ~1200-1700ms), and the answer is correct.
- Given the 7+ benchmark scenarios, When re-run against the NEW production binary, Then results
  match Loop 11's `-O3` findings (not the old `-O0` production findings) — report any
  discrepancy honestly if one appears.
- Given a rollback scenario (simulate: is the backup binary+swap procedure actually documented
  and tested, not just assumed), Then confirm you could revert if needed.
- Given the full regression pass, Then G13/Laya-outage/G17/G18 all still verified working, not
  assumed unaffected.

## Out-of-scope
- Wiring `crates/laya-native` into `apps/server`/`apps/cli` — explicitly decided against this
  loop, given it doesn't outperform the fixed C++ build. Don't do it "since we built it anyway."
- Further micro-optimization beyond the `-O3`/`-march=native` fix — diminishing returns given
  we're now at parity with the hand-rolled Rust implementation's own measured ceiling.
- `act_head` implementation in `laya-native` — stays unimplemented, the crate is a reference,
  not a production target.
