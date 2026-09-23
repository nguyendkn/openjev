//! Throwaway Loop-10 validation binary: verifies the GGUF inventory + the baked RoPE/mask
//! constants against the assumptions the graph is built on, runs the reference question
//! end-to-end, then benchmarks the forward pass. Not a production entry point.
use laya_native::{choice_question, graph::*, softmax, LayaNative, BUCKETS};
use llama_cpp_sys_2 as s;

const MODEL: &str = "/root/.cache/laya-models/laya_english_q8_0.gguf";

fn main() -> Result<(), String> {
    let path = std::env::args().nth(1).unwrap_or_else(|| MODEL.to_string());
    let nth: i32 = std::env::args().nth(2).and_then(|v| v.parse().ok()).unwrap_or(28);
    let t0 = std::time::Instant::now();
    let mut m = LayaNative::load(&path, nth)?;
    println!("== LOAD ==\nloaded {} tensors, {} vocab, {} merges in {:.2}s",
        m.w.tensors.len(), m.w.tokens.len(), m.w.merges.len(), t0.elapsed().as_secs_f64());
    println!("special ids: cls={} sep={} mask={} pad={} unk={}", m.w.cls_id, m.w.sep_id, m.w.mask_id, m.w.pad_id, m.w.unk_id);
    println!("max_len={} head_max_len={} max_opts={}", m.w.max_len, m.w.head_max_len, m.w.max_opts);

    // ---- assumption checks -------------------------------------------------------------
    let (c_sub, c_mul, c_mul1) = m.mask_constants();
    println!("\n== BAKED MASK CONSTANTS ==\nconst_sub={c_sub} const_mul={c_mul} const_mul_1={c_mul1}");

    println!("\n== ROPE THETA CHECK (baked cos tables vs theta^(-j/32)) ==");
    for (name, theta) in [("cos_full", THETA_GLOBAL), ("cos_slide", THETA_LOCAL)] {
        let t = m.w.f32_data(name)?;
        let mut worst = 0f64;
        for p in [1usize, 7, 100, 512] {
            for j in 0..32usize {
                let inv = (theta as f64).powf(-(j as f64) / 32.0);
                let want = (p as f64 * inv).cos();
                let got = t[p * 64 + j] as f64;
                worst = worst.max((want - got).abs());
                // the second half of the 64 entries must duplicate the first (cat(freqs,freqs))
                worst = worst.max((t[p * 64 + j] - t[p * 64 + j + 32]).abs() as f64);
            }
        }
        println!("{name}: theta={theta} max|baked - recomputed| = {worst:.3e}");
    }

    println!("\n== SLIDE_BIAS (sliding-window mask, 513x513) ==");
    {
        let sb = m.w.f32_data("slide_bias")?;
        let at = |q: usize, k: usize| sb[q * 513 + k];
        let row = 100usize;
        let open: Vec<usize> = (0..513).filter(|k| at(row, *k) == 0.0).collect();
        println!("row {row}: zero(open) cols {}..={} (count {})", open[0], open[open.len() - 1], open.len());
        println!("row {row}: blocked value = {}", at(row, 0));
        let asym = (0..513).step_by(37).flat_map(|q| (0..513).step_by(41).map(move |k| (q, k)))
            .filter(|(q, k)| at(*q, *k) != at(*k, *q)).count();
        println!("asymmetric sampled entries: {asym} (0 => orientation-independent)");
        println!("corner rows: row0 open {:?}, row512 open {:?}",
            (0..513).filter(|k| at(0, *k) == 0.0).count(),
            (0..513).filter(|k| at(512, *k) == 0.0).count());
    }

    // ---- reference case ----------------------------------------------------------------
    let state = "The capital of France is: A) London B) Paris";
    let opts = vec!["A".to_string(), "B".to_string()];
    let q = choice_question("Pick the correct option.", &opts);
    println!("\n== TOKENIZATION ==");
    for probe in ["choice question: Pick the correct option.", " A", " B", state] {
        println!("{:>48} -> {:?}", format!("{probe:?}"), m.bpe.encode(probe));
    }

    {
        let sp = laya_native::sequence::SpecialIds { cls: m.w.cls_id, sep: m.w.sep_id, mask: m.w.mask_id };
        let b = laya_native::sequence::build_sequence(&m.bpe, &sp, state, &q, m.w.max_len, m.w.head_max_len);
        println!("built ids ({}) = {:?}\nmarkers = {:?}", b.ids.len(), b.ids, b.markers);
    }
    let r = m.score(state, &q)?;
    println!("\n== FORWARD (reference case) ==");
    println!("n_tokens={} padded_seq={} graph_nodes={} encode={:.2}ms forward={:.1}ms",
        r.n_tokens, r.seq, m.graph_nodes(r.seq).unwrap_or(-1), r.encode_ms, r.forward_ms);
    let temp = m.w.temperature("choice", opts.len());
    println!("raw logits = {:?}   temperature = {temp}", r.logits);
    println!("probabilities(T)  = A={:.4} B={:.4}", r.probabilities[0], r.probabilities[1]);
    let p1 = softmax(&r.logits, 1.0);
    println!("probabilities(T=1)= A={:.4} B={:.4}", p1[0], p1[1]);
    println!("choice = {}  confidence = {:.4}", opts[r.choice], r.confidence);
    println!("REFERENCE          A=0.414 B=0.586 choice=B confidence=0.0215");

    let mut warm = Vec::new();
    for _ in 0..5 {
        let rr = m.score(state, &q)?;
        warm.push(rr.encode_ms + rr.forward_ms);
    }
    println!("warm end-to-end (tokenize+forward, 5 reps): {:?} ms", warm.iter().map(|v| (v * 10.0).round() / 10.0).collect::<Vec<_>>());

    if let Some(h) = m.last_hidden(r.seq) {
        let norm = (h[..1024].iter().map(|v| v * v).sum::<f32>() / 1024.0).sqrt();
        println!("last_hidden[CLS] rms={norm:.4} first8={:?}", &h[..8]);
    }

    // ---- benchmark ---------------------------------------------------------------------
    println!("\n== BENCHMARK (real weights, correctness-checked graph, threads={nth}) ==");
    for seq in BUCKETS {
        let g = Graph::build(&m.w, seq, m.w.max_opts as i64)?;
        unsafe {
            for i in 0..seq as usize {
                *(s::ggml_get_data(g.inp_ids) as *mut i32).add(i) = 100;
                *(s::ggml_get_data(g.pos) as *mut i32).add(i) = i as i32;
            }
            for i in 0..(seq * seq) as usize {
                *(s::ggml_get_data(g.mask_g) as *mut f32).add(i) = 0.0;
                *(s::ggml_get_data(g.mask_l) as *mut f32).add(i) = 0.0;
            }
            *(s::ggml_get_data(g.qtype) as *mut i32) = 0;
            for i in 0..m.w.max_opts {
                *(s::ggml_get_data(g.mpos) as *mut i32).add(i) = 0;
            }
        }
        g.compute(nth)?; // warm
        let reps = if seq <= 128 { 10 } else { 4 };
        let t = std::time::Instant::now();
        for _ in 0..reps {
            g.compute(nth)?;
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0 / reps as f64;
        println!("seq={seq:<4} nodes={:<5} {ms:8.1} ms/forward", g.n_nodes());
    }
    Ok(())
}
