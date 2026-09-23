//! Native Rust + raw-ggml implementation of the Laya decision model.
//!
//! STANDALONE / NOT YET WIRED INTO PRODUCTION. `crates/models::laya` (HTTP -> `laya serve`)
//! remains the only path used by `apps/*` today.
//!
//! # Standing vs the C++ baseline after Loop 13: still parity, and probably structurally so
//!
//! Measured with `laya serve` and this crate interleaved over 6 alternating rounds on one box:
//! C++ median **87.6ms** (n=60) vs Rust median **88.4ms**. Loop 13 cut graph nodes 1430 -> 1076
//! (-25%) and arena use -32%, all bit-exact, and that moved wall-clock **not at all**.
//!
//! The reason is worth writing down so the next loop does not re-litigate it: `perf_span!` puts
//! **99.7% of request latency inside the single `ggml_graph_compute` call** — tokenization, mask
//! fill and softmax together are under 0.3ms of ~88ms — and that call runs the *same* ggml C
//! kernels `ggmlc`'s `laya` runs, over the same graph. There is no Rust-side overhead left to
//! delete. Beating the C++ path means changing what the kernels compute (quantization, padding,
//! kernel selection), not how they are called.
//!
//! # The 2x that IS taken: tighter sequence padding, on by default since 2026-09-23
//!
//! [`DEFAULT_SEQ_ALIGN`] pads to the next multiple of 16 instead of `laya serve`'s power-of-two
//! [`BUCKETS`], which roughly **halves** latency on typical short questions (reference question:
//! median 119.6ms -> 59.5ms). The user chose this deliberately over bit-parity: probabilities
//! drift by up to 2.6pp vs `laya serve`, the winning option never changes. Full reasoning and
//! measurements on [`DEFAULT_SEQ_ALIGN`] and in `docs/benchmarks/server-tuning-results.md`
//! (Loop 13), which also records the candidates that did not pay off.
//!
//! Consequence for the table above: the `-march=native` row reproduces `laya serve` exactly only
//! with `seq_align = 0`. At the default the probabilities are near, not equal.
//!
//! # Build requirement for numerical parity with `laya serve`
//!
//! Build with `RUSTFLAGS="-C target-cpu=native"`. `ggmlc` (which `laya serve` is built from)
//! compiles ggml with `GGML_NATIVE=ON` i.e. `-march=native`; `llama-cpp-sys-2`'s `build.rs` only
//! turns `GGML_NATIVE` on when Rust is also told `target-cpu=native`. The C sources are
//! byte-identical either way, but the different vectorisation of `ggml_vec_scale_f32` makes
//! `ggml_norm` disagree by 1 ULP on ~1.5% of elements, and Q8_0 activation re-quantisation
//! amplifies that into a ~1.7pp probability error by the output. Measured on the reference
//! question (see `tests/sequence_and_reference.rs`):
//!
//! | build | P(A) | P(B) | confidence |
//! |---|---|---|---|
//! | `laya serve` (reference) | 0.4140 | 0.5860 | 0.0215 |
//! | `-C target-cpu=native`   | 0.4140 | 0.5860 | 0.0215 |
//! | default (portable)       | 0.3973 | 0.6027 | 0.0306 |
//!
//! Both builds pick the same option; only the probabilities drift.
pub mod attn;
pub mod debug;
pub mod decode;
pub mod gguf;
pub mod graph;
pub mod head;
pub mod sequence;
pub mod tokenizer;

pub use decode::{argmax, choice_question, entropy_confidence, softmax};

use gguf::Weights;
use graph::Graph;
use llama_cpp_sys_2 as s;
use sequence::{build_sequence, Question, SpecialIds};
use std::collections::HashMap;
use tokenizer::Bpe;

/// The power-of-two length buckets production's `laya serve` rounds every request up to.
pub const BUCKETS: [i64; 4] = [64, 128, 256, 512];
/// Cap on distinct cached graphs. Each owns a flat multi-hundred-MB ggml arena, so once
/// [`LayaNative::seq_align`] makes the key space 32 values wide instead of 4 an unbounded map
/// becomes a slow memory leak. Least-recently-used is evicted.
const MAX_GRAPHS: usize = 6;
const SLIDE_N: usize = 513;

pub struct ScoreResult {
    pub logits: Vec<f32>,
    pub probabilities: Vec<f32>,
    pub choice: usize,
    pub confidence: f32,
    pub n_tokens: usize,
    pub seq: i64,
    pub encode_ms: f64,
    pub forward_ms: f64,
}

pub struct LayaNative {
    /// Declared before `w` on purpose: each `Graph` references weight tensors owned by `w`'s ggml
    /// context, and Rust drops fields in declaration order — so the graphs must die first.
    graphs: HashMap<i64, Graph>,
    /// Cached graph keys, least-recently-used first. Bounded by [`MAX_GRAPHS`].
    lru: Vec<i64>,
    pub w: Weights,
    pub bpe: Bpe,
    slide_bias: Vec<f32>,
    c_sub: f32,
    c_mul: f32,
    c_mul1: f32,
    pub n_threads: i32,
    /// Sequence-padding granularity. `0` reproduces `laya serve`'s [`BUCKETS`] exactly; any value
    /// `> 1` rounds up to the next multiple of it instead (D_13 candidate 5 — see [`Self::pad_seq`]).
    /// Defaults to [`DEFAULT_SEQ_ALIGN`]; set to `0` to get bit-parity with `laya serve` back.
    pub seq_align: i64,
}

/// Default sequence-padding granularity: pad to the next multiple of 16 instead of to `laya
/// serve`'s power-of-two [`BUCKETS`].
///
/// # This is a deliberate user decision (2026-09-23), not an accident — speed over bit-parity
///
/// Loop 13 measured the trade in both directions and the user chose speed via AskUserQuestion:
///
/// * **Gain:** ~**2x** on typical short questions. The 28-token reference question pads to 32
///   instead of 64, and encoder cost is near-linear in padded length — median 119.6ms -> 59.5ms,
///   min 89.0ms -> 46.7ms.
/// * **Cost:** results are no longer bit-identical to `laya serve`. A different token count
///   dispatches a different tinyBLAS GEMM tiling, hence a different float summation order, which
///   Q8_0 activation re-quantisation amplifies — the same 1-ULP mechanism already documented
///   above for `-march=native` and for `-O0`/`-O3`. Probabilities move by at most **2.6pp**
///   (worst case measured: `6_content_moderation` ban-user 0.5534 -> 0.5796).
/// * **Not at risk:** the winning option. All 10 project benchmark scenarios return the same
///   `choice` as before, and outputs differ *only* for questions whose padded length actually
///   changed.
///
/// 16 rather than 1 or 8: it keeps the graph-cache key space to 32 values (bounded further by
/// [`MAX_GRAPHS`]) and measured the same as align 8 or 32 within noise.
pub const DEFAULT_SEQ_ALIGN: i64 = 16;

impl LayaNative {
    pub fn load(path: &str, n_threads: i32) -> Result<Self, String> {
        let w = Weights::load(path)?;
        if w.tokens.is_empty() {
            return Err("GGUF carries no tokenizer.ggml.tokens".into());
        }
        let bpe = Bpe::new(&w.tokens, &w.merges, w.unk_id);
        let slide_bias = w.f32_data("slide_bias")?;
        if slide_bias.len() != SLIDE_N * SLIDE_N {
            return Err(format!("slide_bias has {} elements, expected {}", slide_bias.len(), SLIDE_N * SLIDE_N));
        }
        let c = |n: &str| -> Result<f32, String> { Ok(w.f32_data(n)?[0]) };
        let (c_sub, c_mul, c_mul1) = (c("const_sub_arg1")?, c("const_mul_arg1")?, c("const_mul_1_arg1")?);
        Ok(Self {
            w, bpe, slide_bias, c_sub, c_mul, c_mul1,
            graphs: HashMap::new(), lru: Vec::new(), n_threads, seq_align: DEFAULT_SEQ_ALIGN,
        })
    }

    /// Padded sequence length for `n` real tokens, or `None` if the model cannot hold it.
    ///
    /// D_13 candidate 5. The encoder cost is very close to linear in padded length (measured:
    /// 98 / 165 / 328 / 764 ms at seq 64 / 128 / 256 / 512), and `laya serve` rounds a 28-token
    /// question up to 64 — so more than half that request's compute is spent on pad tokens.
    /// Padded positions are driven to -inf in both attention masks and every other op in the
    /// graph is per-token, so a tighter pad cannot change *which option wins* (verified across
    /// all 10 benchmark scenarios); it does perturb the probabilities slightly, see
    /// [`DEFAULT_SEQ_ALIGN`]. `seq_align = 0` restores `laya serve`'s exact bucket behaviour.
    pub fn pad_seq(&self, n: usize) -> Option<i64> {
        if self.seq_align > 1 {
            let a = self.seq_align;
            let p = ((n as i64).max(a) + a - 1) / a * a;
            return (p <= 512).then_some(p);
        }
        BUCKETS.iter().copied().find(|b| *b as usize >= n)
    }

    pub fn mask_constants(&self) -> (f32, f32, f32) {
        (self.c_sub, self.c_mul, self.c_mul1)
    }

    /// Builds (and caches) the graph for `seq` if absent. Kept separate from the borrow of the
    /// graph itself so a failure here can't leave `self` in a half-moved state.
    fn ensure_graph(&mut self, seq: i64) -> Result<(), String> {
        if !self.graphs.contains_key(&seq) {
            let g = Graph::build(&self.w, seq, self.w.max_opts as i64)?;
            while self.graphs.len() >= MAX_GRAPHS {
                let victim = self.lru.remove(0);
                self.graphs.remove(&victim); // Drop frees that graph's whole arena
            }
            self.graphs.insert(seq, g);
        }
        self.lru.retain(|s| *s != seq);
        self.lru.push(seq);
        Ok(())
    }

    /// Full pipeline for one question. `state` is the prompt text.
    pub fn score(&mut self, state: &str, q: &Question) -> Result<ScoreResult, String> {
        let _sp_all = timing::perf_span!("laya_native::score");
        let t0 = std::time::Instant::now();
        let sp_enc = timing::perf_span!("laya_native::score::encode");
        let sp = SpecialIds { cls: self.w.cls_id, sep: self.w.sep_id, mask: self.w.mask_id };
        let built = build_sequence(&self.bpe, &sp, state, q, self.w.max_len, self.w.head_max_len);
        drop(sp_enc);
        let encode_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let n = built.ids.len();
        let k = built.markers.len();
        if k == 0 {
            return Err("no option markers survived truncation".into());
        }
        // G21: the `logits` tensor is allocated with exactly `max_opts` slots. Reading `k > max_opts`
        // of them is an out-of-bounds raw-pointer read. Reject loudly — a silent truncation would
        // answer a >16-option question wrong with no error signal at all.
        if k > self.w.max_opts {
            return Err(format!(
                "question has {k} options but this model supports at most {} (laya.max_opts); \
                 refusing to score rather than silently dropping options",
                self.w.max_opts
            ));
        }
        let seq = self.pad_seq(n).ok_or("sequence exceeds 512")?;
        let qtype_id = q.qtype as i32;
        let (pad, kmax) = (self.w.pad_id, self.w.max_opts as i64);
        let (c_sub, c_mul, c_mul1) = (self.c_sub, self.c_mul, self.c_mul1);
        let n_threads = self.n_threads;

        {
            let _sp = timing::perf_span!("laya_native::score::ensure_graph", "seq" => seq.to_string());
            self.ensure_graph(seq)?;
        }
        // Second, independent bound: trust the tensor the read actually targets, not just the
        // GGUF metadata that was supposed to have sized it. Checked before `slide_bias` is moved
        // out, so every error path below leaves `self` intact.
        let n_slots = unsafe { s::ggml_nelements(self.graphs[&seq].logits) } as usize;
        if k > n_slots {
            return Err(format!("{k} options exceeds the {n_slots}-slot logits tensor"));
        }
        let slide = std::mem::take(&mut self.slide_bias);
        let g = &self.graphs[&seq];
        let sq = seq as usize;
        let sp_fill = timing::perf_span!("laya_native::score::fill_inputs", "seq" => seq.to_string());
        unsafe {
            // input_ids (pad-filled) + attention_mask-derived additive masks
            let ids = s::ggml_get_data(g.inp_ids) as *mut i32;
            for i in 0..sq {
                *ids.add(i) = if i < n { built.ids[i] } else { pad };
            }
            let att: Vec<f32> = (0..sq).map(|i| if i < n { 1.0 } else { 0.0 }).collect();
            let mg = s::ggml_get_data(g.mask_g) as *mut f32;
            let ml = s::ggml_get_data(g.mask_l) as *mut f32;
            for qi in 0..sq {
                for ki in 0..sq {
                    let pad_term = (att[ki] - c_sub) * c_mul;
                    *mg.add(qi * sq + ki) = att[qi] * att[ki] * c_mul1 + pad_term;
                    *ml.add(qi * sq + ki) = slide[qi * SLIDE_N + ki] + pad_term;
                }
            }
            *(s::ggml_get_data(g.qtype) as *mut i32) = qtype_id;
            let mp = s::ggml_get_data(g.mpos) as *mut i32;
            for i in 0..kmax as usize {
                *mp.add(i) = if i < k { built.markers[i] } else { 0 };
            }
        }
        drop(sp_fill);
        let t1 = std::time::Instant::now();
        g.compute(n_threads)?;
        let forward_ms = t1.elapsed().as_secs_f64() * 1000.0;
        let _sp_dec = timing::perf_span!("laya_native::score::decode");
        let raw: Vec<f32> = unsafe {
            let p = s::ggml_get_data_f32(g.logits);
            (0..k).map(|i| *p.add(i)).collect()
        };
        self.slide_bias = slide;
        // Debug hook for the Loop-11 layer-by-layer diff against `ggmlc-dbg`. Off unless asked.
        if let Ok(p) = std::env::var("LAYA_RS_DUMP") {
            self.graphs[&seq].dump_intermediates(&p)?;
        }

        let temp = self.w.temperature(q.qtype.name(), k);
        let probabilities = softmax(&raw, temp);
        let choice = argmax(&probabilities);
        Ok(ScoreResult {
            logits: raw,
            confidence: entropy_confidence(&probabilities),
            probabilities,
            choice,
            n_tokens: n,
            seq,
            encode_ms,
            forward_ms,
        })
    }

    /// Last encoder hidden state `[D, seq]` from the most recent `score` call.
    pub fn last_hidden(&self, seq: i64) -> Option<Vec<f32>> {
        let g = self.graphs.get(&seq)?;
        unsafe {
            let n = s::ggml_nelements(g.hidden) as usize;
            Some(std::slice::from_raw_parts(s::ggml_get_data_f32(g.hidden), n).to_vec())
        }
    }

    pub fn graph_nodes(&self, seq: i64) -> Option<i32> {
        self.graphs.get(&seq).map(|g| g.n_nodes())
    }
}
