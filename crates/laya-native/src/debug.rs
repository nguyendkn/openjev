//! Env-gated research hooks used to diff this crate against an instrumented `ggmlc` build,
//! layer by layer. Nothing here runs unless the corresponding variable is set — no log spam,
//! no cost on the normal path.
//!
//! # Crate status (as of Loop 13) — parity, and the profile says that is structural
//!
//! Loop 11 found production's `laya serve` had been built with an empty `CMAKE_BUILD_TYPE`
//! (no `-O3`) since Loop 5. Loop 12 rebuilt and deployed `ggmlc` correctly
//! (`-DCMAKE_BUILD_TYPE=Release -DGGML_NATIVE=ON`), which moved production from ~1262ms to the
//! ~90-160ms class and left this crate at parity (95.1 vs 92.7ms).
//!
//! Loop 13 profiled rather than guessed, cut graph nodes 1430 -> 1076 and arena use -32% (all
//! bit-exact), and still measures **parity: 88.4ms vs 87.6ms median**, interleaved. The profile
//! explains why — 99.7% of the latency is inside one `ggml_graph_compute` running the same C
//! kernels as `ggmlc`. See the crate-level docs in `lib.rs` and
//! `docs/benchmarks/server-tuning-results.md` (Loop 13). Wiring into `apps/*` stays gated.
//!
//! Every hook below is deliberately **kept**, not cleaned up: getting the graph numerically
//! identical to `ggmlc` was hard, and these are the tools that made it possible — they are the
//! first thing the next optimisation pass will need. See
//! `docs/benchmarks/server-tuning-results.md` (Loop 12) for the full numbers behind this.
//!
//! * `LAYA_RS_DUMP=<path>` — after `score()`, write every landmark intermediate's shape/stats.
//! * `LAYA_RS_FULL=<names>` — additionally write those tensors' raw F32 bytes to `<path>.<name>.bin`.
//! * `LAYA_PERTURB=<eps>` — scale the residual stream by `1+eps` right after the embedding norm,
//!   to measure how much a given relative perturbation moves the final probabilities.
//! * `LAYA_INJECT_HIDDEN=<path>` — replace the encoder output with a `[D, seq]` F32 blob, so the
//!   head+scorer can be validated against a reference encoder output in isolation.
use crate::graph::{Graph, D};
use llama_cpp_sys_2 as s;

/// `LAYA_PERTURB` hook. Returns `x` unchanged when the variable is unset.
pub unsafe fn maybe_perturb(ctx: *mut s::ggml_context, x: *mut s::ggml_tensor) -> Result<*mut s::ggml_tensor, String> {
    match std::env::var("LAYA_PERTURB") {
        Ok(e) => {
            let e: f32 = e.parse().map_err(|_| "LAYA_PERTURB must be a float")?;
            Ok(s::ggml_scale(ctx, x, 1.0 + e))
        }
        Err(_) => Ok(x),
    }
}

/// `LAYA_INJECT_HIDDEN` hook. The replacement is a leaf tensor, so `ggml_graph_compute` never
/// overwrites it. Returns `x` unchanged when the variable is unset.
pub unsafe fn maybe_inject_hidden(
    ctx: *mut s::ggml_context,
    x: *mut s::ggml_tensor,
    seq: i64,
) -> Result<*mut s::ggml_tensor, String> {
    let Ok(p) = std::env::var("LAYA_INJECT_HIDDEN") else { return Ok(x) };
    let raw = std::fs::read(&p).map_err(|e| format!("{p}: {e}"))?;
    let want = (D * seq) as usize * 4;
    if raw.len() != want {
        return Err(format!("{p}: {} bytes, expected {want}", raw.len()));
    }
    let t = s::ggml_new_tensor_2d(ctx, s::GGML_TYPE_F32, D, seq);
    std::ptr::copy_nonoverlapping(raw.as_ptr(), s::ggml_get_data(t) as *mut u8, want);
    Ok(t)
}

impl Graph {
    /// Writes the landmark intermediates to `path` in the same TSV shape the instrumented
    /// `ggmlc-dbg` build emits, so the two can be diffed directly.
    pub fn dump_intermediates(&self, path: &str) -> Result<(), String> {
        use std::io::Write;
        let full = std::env::var("LAYA_RS_FULL").unwrap_or_default();
        let mut f = std::fs::File::create(path).map_err(|e| e.to_string())?;
        writeln!(f, "# name\tne\tcount\tmean\trms\tmin\tmax\tfirst8").map_err(|e| e.to_string())?;
        for (name, t) in &self.dbg {
            unsafe {
                let ne = s::ggml_nelements(*t) as usize;
                if (**t).type_ != s::GGML_TYPE_F32 {
                    writeln!(f, "{name}\tSKIPTYPE {:?}", (**t).type_).map_err(|e| e.to_string())?;
                    continue;
                }
                let v = std::slice::from_raw_parts(s::ggml_get_data_f32(*t), ne);
                let (mut sum, mut sq) = (0f64, 0f64);
                let (mut mn, mut mx) = (f32::INFINITY, f32::NEG_INFINITY);
                for x in v {
                    sum += *x as f64;
                    sq += (*x as f64) * (*x as f64);
                    mn = mn.min(*x);
                    mx = mx.max(*x);
                }
                if full.split(',').any(|w| w == name) {
                    let bytes: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
                    std::fs::write(format!("{path}.{name}.bin"), bytes).map_err(|e| e.to_string())?;
                }
                let n = ne as f64;
                let head: Vec<String> = v.iter().take(8).map(|x| format!("{x:.9}")).collect();
                writeln!(f, "{name}\t{},{},{},{}\t{ne}\t{:.9e}\t{:.9e}\t{:.9e}\t{:.9e}\t{}",
                    (**t).ne[0], (**t).ne[1], (**t).ne[2], (**t).ne[3],
                    sum / n, (sq / n).sqrt(), mn, mx, head.join(" "))
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}
