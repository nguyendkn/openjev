//! Native Rust + raw-ggml implementation of the Laya decision model.
//!
//! STANDALONE / NOT WIRED INTO PRODUCTION. `crates/models::laya` (HTTP -> `laya serve`) remains
//! the only path used by `apps/*`.
pub mod gguf;
pub mod graph;
pub mod sequence;
pub mod tokenizer;

use gguf::Weights;
use graph::Graph;
use llama_cpp_sys_2 as s;
use sequence::{build_sequence, QType, Question, SpecialIds};
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
    pub w: Weights,
    pub bpe: Bpe,
    slide_bias: Vec<f32>,
    c_sub: f32,
    c_mul: f32,
    c_mul1: f32,
    graphs: HashMap<i64, Graph>,
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

    fn graph_for(&mut self, seq: i64) -> Result<&Graph, String> {
        if !self.graphs.contains_key(&seq) {
            let g = Graph::build(&self.w, seq, self.w.max_opts as i64)?;
            self.graphs.insert(seq, g);
        }
        Ok(&self.graphs[&seq])
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
        let seq = *BUCKETS.iter().find(|b| **b as usize >= n).ok_or("sequence exceeds 512")?;
        let qtype_id = q.qtype as i32;
        let (pad, kmax) = (self.w.pad_id, self.w.max_opts as i64);
        let (c_sub, c_mul, c_mul1) = (self.c_sub, self.c_mul, self.c_mul1);
        let slide = std::mem::take(&mut self.slide_bias);
        let n_threads = self.n_threads;

        let g = self.graph_for(seq)?;
        let sq = seq as usize;
        unsafe {
            // input_ids (pad-filled) + attention_mask-derived additive masks
            let ids = s::ggml_get_data(g.inp_ids) as *mut i32;
            let pos = s::ggml_get_data(g.pos) as *mut i32;
            for i in 0..sq {
                *ids.add(i) = if i < n { built.ids[i] } else { pad };
                *pos.add(i) = i as i32;
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

pub fn softmax(logits: &[f32], temp: f32) -> Vec<f32> {
    let scaled: Vec<f32> = logits.iter().map(|l| l / temp).collect();
    let m = scaled.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let e: Vec<f32> = scaled.iter().map(|l| (l - m).exp()).collect();
    let sum: f32 = e.iter().sum();
    e.iter().map(|v| v / sum).collect()
}

pub fn argmax(p: &[f32]) -> usize {
    p.iter().enumerate().fold((0usize, f32::NEG_INFINITY), |(bi, bv), (i, v)| if *v > bv { (i, *v) } else { (bi, bv) }).0
}

/// `confidence_from_probs`: 1 - H(p)/log(k).
pub fn entropy_confidence(p: &[f32]) -> f32 {
    let k = p.len();
    if k < 2 {
        return 1.0;
    }
    let ent: f32 = -p.iter().map(|v| v * v.clamp(1e-12, 1.0).ln()).sum::<f32>();
    (1.0 - ent / (k as f32).ln()).clamp(0.0, 1.0)
}

pub fn choice_question(instructions: &str, options: &[String]) -> Question {
    Question {
        qtype: QType::Choice,
        instructions: instructions.to_string(),
        criteria: options.iter().map(|o| (o.clone(), None)).collect(),
    }
}
