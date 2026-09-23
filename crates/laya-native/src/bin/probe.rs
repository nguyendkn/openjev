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

    // G22 evidence: build and drop the largest bucket repeatedly. Without `Drop for Graph` each
    // iteration leaked its ~6.7 GB arena; with it, peak RSS must stay flat.
    if std::env::var("LAYA_LEAK_CHECK").is_ok() {
        let rss = || -> String {
            std::fs::read_to_string("/proc/self/status")
                .unwrap_or_default()
                .lines()
                .find(|l| l.starts_with("VmHWM") || l.starts_with("VmRSS"))
                .map(|l| l.to_string())
                .unwrap_or_default()
        };
        println!("\n== LEAK CHECK (build+drop seq=512 graph x5) ==\nbefore: {}", rss());
        for i in 0..5 {
            let g = Graph::build(&m.w, 512, m.w.max_opts as i64)?;
            g.compute(nth)?;
            drop(g);
            println!("iter {i}: {}", rss());
        }
        let hwm = std::fs::read_to_string("/proc/self/status").unwrap_or_default();
        println!("peak: {}", hwm.lines().find(|l| l.starts_with("VmHWM")).unwrap_or(""));
        return Ok(());
    }

    // Ad-hoc case runner for the Loop-11 benchmark-scenario comparison against `laya serve`.
    // `LAYA_CASE="<state>\t<instructions>\t<opt1>|<opt2>|..."` -> one line of probabilities.
    if let Ok(case) = std::env::var("LAYA_CASE") {
        let f: Vec<&str> = case.split('\t').collect();
        if f.len() != 3 {
            return Err("LAYA_CASE needs 3 tab-separated fields: state, instructions, opts".into());
        }
        let opts: Vec<String> = f[2].split('|').map(|s| s.to_string()).collect();
        let q = choice_question(f[1], &opts);
        let r = m.score(f[0], &q)?;
        let ps: Vec<String> = opts.iter().zip(&r.probabilities).map(|(o, p)| format!("{o}={p:.4}")).collect();
        println!("n_tokens={} seq={} k={} choice={} confidence={:.4} probs: {}",
            r.n_tokens, r.seq, opts.len(), opts[r.choice], r.confidence, ps.join(" "));
        return Ok(());
    }

    // Debug aid for the Loop-11 weight audit: stats for explicitly named F32 tensors.
    if std::env::var("LAYA_LIST_TENSORS").is_ok() {
        println!("\n== TENSOR INVENTORY ==");
        let mut names: Vec<&String> = m.w.tensors.keys().collect();
        names.sort();
        for n in names {
            let t = m.w.tensors[n];
            unsafe { println!("{n}\t{:?}\tne=[{},{},{},{}]", (*t).type_, (*t).ne[0], (*t).ne[1], (*t).ne[2], (*t).ne[3]); }
        }
    }
    if let Ok(names) = std::env::var("LAYA_TENSOR_STATS") {
        println!("\n== TENSOR STATS ==");
        for n in names.split(',').filter(|n| !n.is_empty()) {
            match m.w.f32_data(n) {
                Ok(v) => {
                    let sum: f64 = v.iter().map(|x| *x as f64).sum();
                    let sq: f64 = v.iter().map(|x| (*x as f64) * (*x as f64)).sum();
                    let c = v.len() as f64;
                    println!("{n}: n={} mean={:.9e} rms={:.9e} min={:.9e} max={:.9e} first4={:?}",
                        v.len(), sum / c, (sq / c).sqrt(),
                        v.iter().cloned().fold(f32::INFINITY, f32::min),
                        v.iter().cloned().fold(f32::NEG_INFINITY, f32::max),
                        &v[..v.len().min(4)]);
                    if let Ok(dir) = std::env::var("LAYA_TENSOR_DUMP_DIR") {
                        let b: Vec<u8> = v.iter().flat_map(|x| x.to_le_bytes()).collect();
                        std::fs::write(format!("{dir}/{n}.bin"), b).map_err(|e| e.to_string())?;
                    }
                }
                Err(e) => println!("{n}: {e}"),
            }
        }
    }

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
        let (used, reserved) = g.arena_usage();
        println!("seq={seq:<4} nodes={:<5} {ms:8.1} ms/forward   arena {:.0}/{:.0} MiB ({:.0}% used)",
            g.n_nodes(), used as f64 / 1048576.0, reserved as f64 / 1048576.0,
            100.0 * used as f64 / reserved as f64);
    }
    Ok(())
}
