//! Native Rust + raw-ggml implementation of the Laya decision model.
//!
//! STANDALONE / NOT YET WIRED INTO PRODUCTION. `crates/models::laya` (HTTP -> `laya serve`)
//! remains the only path used by `apps/*` today. Current standing vs the correctly-built
//! (`-O3 -march=native`) `ggmlc` that Loop 12 deployed to production: **95.1ms vs 92.7ms —
//! parity (~3% slower), not yet a win**. Loop 13+ keeps optimising this crate against that
//! baseline; wiring it in is gated on beating it, not on it being "good enough". See
//! `src/debug.rs`'s crate-status note and `docs/benchmarks/server-tuning-results.md` (Loop 12).
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

pub const BUCKETS: [i64; 4] = [64, 128, 256, 512];
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
    pub w: Weights,
    pub bpe: Bpe,
    slide_bias: Vec<f32>,
    c_sub: f32,
    c_mul: f32,
    c_mul1: f32,
    pub n_threads: i32,
}

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
        Ok(Self { w, bpe, slide_bias, c_sub, c_mul, c_mul1, graphs: HashMap::new(), n_threads })
    }

    pub fn mask_constants(&self) -> (f32, f32, f32) {
        (self.c_sub, self.c_mul, self.c_mul1)
    }

    /// Builds (and caches) the graph for `seq` if absent. Kept separate from the borrow of the
    /// graph itself so a failure here can't leave `self` in a half-moved state.
    fn ensure_graph(&mut self, seq: i64) -> Result<(), String> {
        if !self.graphs.contains_key(&seq) {
            let g = Graph::build(&self.w, seq, self.w.max_opts as i64)?;
            self.graphs.insert(seq, g);
        }
        Ok(())
    }

    /// Full pipeline for one question. `state` is the prompt text.
    pub fn score(&mut self, state: &str, q: &Question) -> Result<ScoreResult, String> {
        let t0 = std::time::Instant::now();
        let sp = SpecialIds { cls: self.w.cls_id, sep: self.w.sep_id, mask: self.w.mask_id };
        let built = build_sequence(&self.bpe, &sp, state, q, self.w.max_len, self.w.head_max_len);
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
        let seq = *BUCKETS.iter().find(|b| **b as usize >= n).ok_or("sequence exceeds 512")?;
        let qtype_id = q.qtype as i32;
        let (pad, kmax) = (self.w.pad_id, self.w.max_opts as i64);
        let (c_sub, c_mul, c_mul1) = (self.c_sub, self.c_mul, self.c_mul1);
        let n_threads = self.n_threads;

        self.ensure_graph(seq)?;
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
        let t1 = std::time::Instant::now();
        g.compute(n_threads)?;
        let forward_ms = t1.elapsed().as_secs_f64() * 1000.0;
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
