# Deployment Best Practices — Real Lessons from Production

This chapter consolidates **real production lessons** from the openjev-rs deployment loop (8 loops, 2026-09-23). These are not theoretical; they're bugs caught, optimizations measured, and architectural patterns proven under load on real hardware.

---

## CPU Tuning: n_threads, Native Build, Batch Size

### Finding 1: Thread Count Matters (n_threads Tuning, Loop 6)

Benchmarked 3 configurations on a 32-core Xeon:
- `threads=16` (conservative, half the box)
- `threads=28` (default, headroom for OS/other processes)
- `threads=32` (every core, no headroom)

| Model | 16 threads | 28 threads | 32 threads | Winner |
|-------|-----------|-----------|-----------|--------|
| Qwen3-0.6B (generation) | 5.6s | 4.9s | 5.0s | **28** |
| MiniCPM5-2B (generation) | 9.9s | 8.1s | 8.2s | **28** |
| Qwen3-4B (generation) | 18.0s | 14.9s | 15.5s | **28** |

**Why 28 beats 32**: Thread-pool sync overhead + context-switch cost don't scale linearly. When every core is saturated with no headroom for the OS scheduler (or concurrent processes like `laya serve`), small inference tasks (constrained-readout) regress **2–2.5x**. Allocation: keep 4 cores (12.5%) for OS/system services.

**Recommendation**: On N-core hardware, start with `n_threads = N × 0.875` (85% utilization). Tune ±2-4 cores based on your hardware + workload mix.

### Finding 2: Native CPU Build Cuts Constrained-Readout ~10–25% (Loop 7)

Rebuilt with `RUSTFLAGS="-C target-cpu=native"` on an Intel Xeon Gold 5320 (Ice Lake, AVX-512 capable):

| Model | Non-native | Native | Improvement |
|-------|-----------|--------|------------|
| Qwen3-0.6B (readout) | 125.7 ms | 112.2 ms | **-10.8%** |
| MiniCPM5-2B (readout) | 233.4 ms | 178.9 ms | **-23.4%** |
| Qwen3-4B (readout) | 389.7 ms | 291.0 ms | **-25.3%** |

**Mechanism**: `-march=native` reaches AVX-512 on this hardware; llama.cpp's ggml has hand-tuned SIMD kernels that exploit wider SIMD. Generation latency shows noise (mixed bag), but constrained-readout (single-token decode, tight loop) benefits consistently.

**How to enable** (in your build):
```bash
RUSTFLAGS="-C target-cpu=native" cargo build --release --workspace
```

Verify in the CMake output:
```
GGML_NATIVE:BOOL=ON
GGML_AVX512:BOOL=ON  # (if CPU supports it)
```

### Finding 3: n_batch Tuning Showed No Consistent Win (Loop 7)

Tested `n_batch=256`, `512` (default), and `1024`:

| Model | batch=256 | batch=512 | batch=1024 |
|-------|-----------|-----------|-----------|
| Qwen3-0.6B (generation) | 4.1s | 4.3s | 4.0s |
| MiniCPM5-2B (generation) | 8.5s | 8.5s | 8.3s |
| Qwen3-4B (generation) | 14.8s | 14.2s | 14.7s |

Deltas all within single-run noise (±5%). **Verdict**: Don't tune `n_batch` unless you're running long-sequence or batched inference. This workload (single-token generation steps, short context) doesn't stress the batch size.

---

### Finding 4: An Empty `CMAKE_BUILD_TYPE` Cost 13x (Loop 11–12) — The Biggest Single Win

The most expensive bug in this whole deployment was one unset CMake variable.

`laya serve` is built from an external C++ project (`ggmlc`). It was configured in Loop 5 with a
plain `cmake -S . -B build` — **no `-DCMAKE_BUILD_TYPE`**. CMake's default for that variable is
the empty string, and an empty build type means CMake appends **no optimization flags at all**
(neither `CMAKE_CXX_FLAGS_RELEASE` nor `_DEBUG` applies). The result is an effectively `-O0`
binary that looks completely normal: it builds without warnings, runs correctly, and produces
the right answers. It just runs 13x slower.

What made it invisible for six loops was that the *other* flag people did check looked right:

```
# Old production build — CMakeCache.txt
CMAKE_BUILD_TYPE:STRING=          # <- empty. Nobody looked at this.
GGML_NATIVE:BOOL=ON               # <- everyone checked THIS, and it was fine.
```

Loop 8 verified `GGML_NATIVE=ON` and `-march=native` on the real compiler command line, concluded
"build flags are ruled out," and spent the next three loops hunting the remaining gap in ggmlc's
algorithmic efficiency — eventually writing a whole native Rust reimplementation to escape it.
The actual compiler invocations tell the story instantly, if you look at the *whole* line and not
just the flag you came for:

```
# OLD (production, Loops 5-11)
CXX_FLAGS = -std=gnu++17 -fPIC -march=native
# NEW (Loop 12, -DCMAKE_BUILD_TYPE=Release)
CXX_FLAGS = -O3 -DNDEBUG -std=gnu++17 -fPIC -march=native
```

`-march=native` without `-O3` tells the compiler *which* instructions it may use while telling it
not to bother optimizing — the CPU-feature flag was doing almost nothing on its own.

Measured effect on production, external `curl` (nothing else changed — same source commit, same
Q8_0 model, same `--threads 28`, same systemd unit):

| | `laya_inference_ms` |
|---|---|
| Before (`CMAKE_BUILD_TYPE=""`) | 1202 / 1267 / 1910 ms |
| After (`CMAKE_BUILD_TYPE=Release`) | 113 / 113 / 129 / 131 / 158 ms |

**Lessons**:

1. **For any CMake project you build yourself, pass `-DCMAKE_BUILD_TYPE` explicitly.** There is no
   safe default. Unlike Cargo (`--release`), Meson (`buildtype=debugoptimized`), or Go, CMake's
   default is "no opinion", which in practice means unoptimized.
2. **Verify at the compiler-invocation level, not the cache-variable level.** Read the real
   `CXX_FLAGS` line, e.g. `grep CXX_FLAGS build/CMakeFiles/<target>.dir/flags.make` or
   `cmake --build . -- VERBOSE=1`. A cache variable being set is not proof the flag you care
   about reached `g++`, and — as here — checking one flag can create false confidence about all
   the others.
3. **Benchmark against a correctly-configured baseline before attributing a win to your own
   design.** Loops 9–11 measured a hand-written Rust engine at ~95ms against a ~1262ms C++
   incumbent and read that as a 13x architectural win. Rebuilt properly, the same C++ source hits
   ~92.7ms — the Rust engine is at *parity*, and essentially the entire "win" was the missing
   `-O3`. The rewrite is still worth continuing (see `crates/laya-native`), but on an honest
   baseline. **A performance comparison is only as trustworthy as the weakest-configured side of
   it**, and the side you didn't build yourself is the one to suspect.
4. **A 10x+ unexplained gap is a configuration smell, not an algorithmic finding.** When a
   32-core Xeon loses to a 4-core i3 by 11x (Loop 8's unresolved anomaly), the prior should be
   "something in my setup is wrong," not "the upstream implementation is inefficient." Loop 8
   wrote the gap down as an honest open question — which was the right call at the time — but the
   size of the anomaly was itself the strongest available clue, and it pointed at the build.

---

## Watch Out For: Default Flags in External Binaries

### Lesson: Laya's Default --threads Was 4 (Loop 8)

**The bug**: `laya serve <model> --port 8090 --device cpu` **defaults to `--threads 4`** (not configurable, hardcoded in the binary help).

On a 32-core server, Laya ran on 4 threads for 3 entire loops without anyone noticing:
- Loop 5: `laya_inference_ms` ~8.3s (seemed slow, thought Laya was just slow)
- Loop 8: Same payload with `--threads 28`: **1.2s** (**~5.8x faster**)

**Lesson**: **Never trust the claimed default of an external binary. Always verify:**
```bash
# Check what your binary actually supports
laya serve --help | grep -i thread
# or
laya serve --help | grep -i "cpu\|threads"

# Always explicitly set the flag
laya serve <model> --port 8090 --device cpu --threads 28
```

### Why It Wasn't Caught Earlier

The Laya call was bundled inside openjev-rs's `/bench` endpoint, which reports aggregate 3-method timing. Laya's slow latency looked like "Laya is inherently slow" rather than "misconfigured Laya". **Lesson**: Benchmark components **in isolation** before integrating them; catches configuration bugs early.

---

## Graceful Degradation When Components Fail

### Pattern: Laya Outage Should NOT Break the LLM Methods

**Architectural requirement**: Laya is a separate HTTP service (`laya serve` running on `localhost:8090`). If it crashes, `/bench` should:
1. Return a valid response with `laya: null`, `laya_inference_ms: ~10ms` (fast fail).
2. Include a degrade log message.
3. Continue serving the other two methods (constrained-readout, generation).

**Implemented in openjev-rs** (`crates/models/src/laya.rs`):
```rust
// Pseudo-code
match http_client.post("http://127.0.0.1:8090/v1/decide").send() {
    Ok(response) => { /* parse & return */ },
    Err(e) => {
        eprintln!("laya unavailable, continuing without it: {}", e);
        return LayaAnswer { best_option: None, inference_ms: 10 };
    }
}
```

**Verified in production** (Loop 5, 7, 8):
- Kill Laya: `/bench` → HTTP 200, `laya: null`.
- Restart Laya: next `/bench` → `laya` scores correctly again.
- No openjev-server restart needed.

**Lesson**: Multi-component systems should **fail open** (degrade gracefully) rather than **fail closed** (fail the whole request). Your SLA is the slowest dependency; if one dependency is flaky, isolate it.

---

## Security: Expose Only What You Need

### Gap: Laya Server Bound 0.0.0.0:8090, Not 127.0.0.1:8090 (G17, Loop 5)

**The issue**: The `laya` binary's `serve` subcommand has **no `--host` / `--bind` flag**. It always binds `0.0.0.0:8090` (all interfaces), regardless of claims in documentation.

**Impact**: If external firewall was misconfigured or removed, `/v1/decide` becomes a public, unauthenticated, unrestricted inference endpoint. DoS/resource abuse/model-probing exposure.

**Mitigations**:
1. **Host-level firewall** (Loop 6–7): `iptables -A INPUT -p tcp --dport 8090 -i lo -j ACCEPT && iptables -A INPUT -p tcp --dport 8090 -j DROP` (loopback-only via INPUT chain rules).
2. **Persistence** (Loop 7): Systemd unit that re-applies the rules on boot: `openjev-laya-firewall.service`.
3. **Documentation** (Loop 6): Updated README to describe the real mechanism, not the false claim ("bound to 127.0.0.1").

**Lesson**: **Always verify actual network exposure**, not what docs claim. Use `ss -ltnp` (Linux) or `netstat -an` (universal) to check what's actually listening on what interface. Test from external machines.

---

## Avoiding Multi-Model Caching Bugs

### Bug: BackendAlreadyInitialized (Loop 4, Closed)

**Symptom**: Loading a second LLM model in the same process failed with `BackendAlreadyInitialized` error.

**Root cause**: `llama-cpp-2` wraps llama.cpp's global backend state. You can only call `LlamaBackend::init()` once per process. Second model load tried to init again → panic.

**Fix**: `Arc<OnceLock<LlamaBackend>>` singleton, shared across all model loads:
```rust
// Global backend, init once
lazy_static! {
    static ref BACKEND: Arc<LlamaBackend> = Arc::new(
        LlamaBackend::init().expect("backend init failed")
    );
}

// Each model load re-uses it
let model = LlamaModel::load_from_file(&*BACKEND, model_path, ...)?;
```

**Lesson**: Check whether external C/C++ libraries you're binding have **global mutable state**. Read their docs and source code; test multi-instance scenarios explicitly.

---

## Worker Thread Supervision (G13, Loops 4–8)

### Problem: No Panic Supervision

The openjev-server's HTTP handler spins up a tokio-blocking worker thread to call `run_bench()`:

```rust
// Before fix: if run_bench panics, the thread dies silently
tokio::task::spawn_blocking(move || {
    run_bench(/*...*/)
}).await?
```

If `run_bench` panicked, the thread exited; subsequent `/bench` requests would 500 until the whole process restarted. **SPOF** (single point of failure).

### Solution: Supervisor Loop + Respawn

```rust
loop {
    // Worker thread
    if let Err(_) = run_bench(...) {
        eprintln!("worker panicked, respawning...");
        cache.clear(); // fresh model load next request
    }
    // Keep running, don't exit
}
```

Catches panics, clears cached model (ensures fresh state), waits for next request. Verified via fault injection (Loop 4–8) with sentinel panics.

**Lesson**: Long-lived services need **supervision**. Panics should not crash threads silently; they should be caught, logged, and recovered. Systemd's `Restart=on-failure` works for process-level crashes; supervisor loops handle intra-process thread deaths.

---

## HTTP Server Lifecycle Management (Loop 8)

### Migrating from Ad-Hoc nohup to Systemd

Originally (Loops 5–7):
```bash
setsid nohup ./target/release/openjev-server --port 80 --threads 28 > /tmp/openjev-server.log 2>&1 &
setsid nohup laya serve <model> --port 8090 --device cpu --threads 4 > /tmp/laya.log 2>&1 &
```

**Problems**:
- Flags buried in the shell command (easy to forget `--threads 28`).
- No restart on crash (until manual intervention).
- No boot-time startup (requires manual restart after reboot).

### Solution: Systemd Units (Loop 8)

```ini
# /etc/systemd/system/openjev-server.service
[Unit]
Description=OpenJev Rust Server
After=network.target

[Service]
Type=simple
ExecStart=/root/openjev-rs/target/release/openjev-server --port 80 --threads 28
Restart=on-failure
RestartSec=5s
User=root

[Install]
WantedBy=multi-user.target
```

**Benefits**:
- Flags visible in the service file (reviewable, versionable).
- `Restart=on-failure`: automatic recovery on crash.
- `systemctl enable`: boot-time startup.
- `systemctl status`: health check.
- `journalctl -u openjev-server`: logs.

Deployed both `openjev-server.service` and `openjev-laya-serve.service` (Loop 8); both enabled and verified.

**Lesson**: For production deployments, use init-system management (systemd, supervisor, etc.), not ad-hoc backgrounded processes. Makes ops/debugging/automation much cleaner.

---

## Measuring: Independent Verification Matters

### QA Caught Self-Report Inaccuracies (Final Report)

Loop 1: Developer reported "Windows build works," but independent QA rebuild showed it fails (missing libclang).

Loops 4–5: IDE's cache showed "code works," but a clean rebuild exposed stale binaries.

**Pattern**: A developer's own test run (especially in an IDE with hot-reload/caching) is not the same as an independent build from scratch. Always have **someone else** or **a CI system** verify critical claims.

**Lesson**: For performance claims especially:
- **Never trust self-reported numbers** ("I ran the benchmark and got 2x faster").
- **Have an independent re-run** (different machine, different time, clean build).
- **Log raw data** (timestamps, hardware specs, git commit) so results are reproducible.

---

## Final Production Config (Verified Across 8 Loops)

On **Ubuntu 32-core Xeon** (103.146.166.46):

```bash
# LLM inference engine (constrained-readout + generation)
./target/release/openjev-server --port 80 --threads 28

# Laya scoring (separate service)
laya serve /root/.cache/laya-models/laya_english_q8_0.gguf \
  --port 8090 --device cpu --threads 28

# Both managed by systemd
systemctl status openjev-server.service
systemctl status openjev-laya-serve.service

# Firewall (loopback-only for Laya)
iptables -L INPUT -n
# (applied via systemd unit openjev-laya-firewall.service, persisted across reboot)

# Rust-side build flags (verified in CMakeCache.txt of llama-cpp-sys-2's vendored build)
GGML_NATIVE=ON  # -march=native
GGML_OPENMP=ON  # threading backend
GGML_AVX512=ON  # on this hardware
GGML_BLAS=OFF   # CPU only

# C++-side (ggmlc / laya serve) — Loop 12 fix, the one that mattered most
cmake -S /tmp/ggmlc -B /tmp/ggmlc/build-release \
  -DCMAKE_BUILD_TYPE=Release \   # <-- NOT optional; empty default = -O0 = 13x slower
  -DGGML_NATIVE=ON
cmake --build /tmp/ggmlc/build-release --target laya -j 24
# verify the flags actually reached the compiler, don't trust the cache variable:
grep CXX_FLAGS /tmp/ggmlc/build-release/CMakeFiles/ggml_lib.dir/flags.make
# -> -O3 -DNDEBUG -std=gnu++17 -fPIC -march=native
```

**Binary swap procedure** (back up first; a 2-second restart, fully reversible):

```bash
# deploy
systemctl stop openjev-laya-serve.service
cp -a /tmp/ggmlc/build/examples/laya/laya /root/laya-binary-backups/laya-<tag>.bak   # BACKUP
cp -a <new-binary> <prod-path>.new && mv -f <prod-path>.new <prod-path>              # atomic rename
systemctl start openjev-laya-serve.service
curl -sf http://127.0.0.1:8090/health   # gate on this before declaring success

# rollback = the same steps with the .bak as the source
```

Keep both directions as *scripts* (`deploy-o3.sh` / `rollback.sh`), and **actually run the
rollback once** before you need it. Loop 12 did: rolled production back to the old binary,
confirmed the old md5 and the old (pre-`-O3`) probability vector came back, then re-deployed. An
untested rollback is a hope, not a plan.

**Performance (Loop 12 harness, 30 requests, external `curl` verified):**
- Constrained-readout: 112–291 ms (models 0.6B–4B)
- Generation: 4.3–15.6s (models 0.6B–4B)
- Laya: **~90–160 ms** (constant across models; was 1.2–1.4s before the `CMAKE_BUILD_TYPE` fix)
- Uptime: 8+ days, zero unplanned restarts (only intentional fault injection, systemd control
  tests, and Loop 12's ~2s binary-swap + rollback-drill windows)

---

## Sources

- `docs/benchmarks/server-tuning-results.md` — Loop 6 (n_threads), Loop 7 (native build, n_batch), Loop 8 (Laya thread fix, quant tuning).
- `plans/20260922-2328-hoh-openjev-rs/issue-ledger.md` — G13 (worker supervision), G17 (Laya auth), G18 (firewall persistence), all verified fixes.
- `plans/20260922-2328-hoh-openjev-rs/run.md` — final production config, DoD sign-off.
- `plans/20260922-2328-hoh-openjev-rs/reports/final.md` — verification discipline, QA catches.

