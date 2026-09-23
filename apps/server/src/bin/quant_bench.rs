// THROWAWAY benchmark binary (Loop 18, Tasks 3/4): measures real prompt-processing and
// token-generation throughput for an explicit GGUF file on THIS box, mirroring `llama-bench
// -p <pp> -n <tg> -r <reps>`'s methodology so Loop 16's K8s (Sapphire Rapids, AMX-INT8) numbers
// can be directly re-measured on production (Xeon Gold 5320 Ice Lake, no AMX) through our own
// engine/runtime instead of trusting the K8s ranking. Not production code -- it exists purely
// to produce the real numbers Loop 18's dev-doc requires before any quant-swap deploy decision.
//
// Unlike `run_generate` (which stops at EOG or MAX_TOKENS), the generation loop here always
// forces exactly `n_gen` decode steps regardless of EOG, same as `llama-bench`'s `tg` test, so
// results are comparable across quant variants/models without being skewed by how quickly a
// given file naturally stops.
use engine::{Engine, EngineConfig};
use std::path::PathBuf;
use std::time::Instant;

const PROMPT: &str = "You are a careful, concise decision-making assistant. Given a short \
scenario and a list of options, you choose exactly one option and explain your reasoning in \
one or two sentences before giving a final structured answer. Consider the context carefully, \
weigh the available evidence, and avoid unnecessary hedging. Scenario: a production incident \
has been reported with elevated latency and an increased error rate on the checkout API.";

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: quant_bench <gguf_path> [n_gen=128] [reps=3] [n_threads=28]");
        std::process::exit(2);
    }
    let path = PathBuf::from(&args[1]);
    let n_gen: usize = args.get(2).and_then(|v| v.parse().ok()).unwrap_or(128);
    let reps: usize = args.get(3).and_then(|v| v.parse().ok()).unwrap_or(3);
    let n_threads: i32 = args.get(4).and_then(|v| v.parse().ok()).unwrap_or(28);

    eprintln!(
        "loading {path:?} (n_threads={n_threads})... size={}MB",
        std::fs::metadata(&path).map(|m| m.len() / 1_000_000).unwrap_or(0)
    );
    let t0 = Instant::now();
    let mut engine = Engine::load(
        &path,
        EngineConfig {
            n_threads,
            ..EngineConfig::default()
        },
    )
    .expect("engine load");
    eprintln!("loaded in {:.1}s", t0.elapsed().as_secs_f64());

    println!("path,rep,prompt_tokens,pp_ms,pp_tok_s,gen_tokens,tg_ms,tg_tok_s");
    for rep in 0..reps {
        engine.reset_context();
        let tokens = engine.tokenize(PROMPT).expect("tokenize");
        let n_prompt = tokens.len();

        let t0 = Instant::now();
        let mut logits = engine.decode_prompt(&tokens).expect("decode_prompt");
        let pp_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let mut pos = n_prompt as i32;
        let t0 = Instant::now();
        for _ in 0..n_gen {
            let next_id = Engine::sample_greedy_from_logits(&logits) as i32;
            // Forced generation: do NOT stop at EOG, so every rep does exactly n_gen decode
            // steps (matches llama-bench's `tg` test; avoids one file "winning" on speed just
            // by finishing sooner).
            logits = engine.decode_next_id(next_id, pos).expect("decode_next");
            pos += 1;
        }
        let tg_ms = t0.elapsed().as_secs_f64() * 1000.0;

        let pp_tok_s = n_prompt as f64 / (pp_ms / 1000.0);
        let tg_tok_s = n_gen as f64 / (tg_ms / 1000.0);
        println!(
            "{},{rep},{n_prompt},{pp_ms:.1},{pp_tok_s:.2},{n_gen},{tg_ms:.1},{tg_tok_s:.2}",
            path.display()
        );
    }
}
