// D_11b Task 4: measures `PerfSpan`'s own overhead (enabled vs disabled), so a distorting
// profiler can be caught before being called "done". Run twice: once with `PERF_LOG` unset
// (disabled path) and once with `PERF_LOG=1` (enabled path, draining into the real background
// writer thread), and compare mean per-call cost. Not a substitute for the real end-to-end
// benchmark harness comparison (see the loop report), just an isolated microbenchmark of the
// span itself.
use std::time::Instant;

const ITERATIONS: u64 = 1_000_000;

fn main() {
    // Warm up the OnceLocks (env read + writer-thread spawn, if enabled) outside the timed loop.
    {
        let _s = timing::perf_span!("bench::warmup");
    }

    let t0 = Instant::now();
    for i in 0..ITERATIONS {
        let mut s = timing::perf_span!("bench::noop_span");
        s.set("iter", i.to_string());
    }
    let elapsed = t0.elapsed();

    let enabled = std::env::var("PERF_LOG").map(|v| v == "1").unwrap_or(false);
    println!(
        "PERF_LOG={} iterations={} total={:?} mean_ns_per_span={:.1}",
        enabled,
        ITERATIONS,
        elapsed,
        elapsed.as_nanos() as f64 / ITERATIONS as f64
    );
}
