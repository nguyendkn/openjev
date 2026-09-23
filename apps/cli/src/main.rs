use clap::Parser;
use serde::Serialize;
use std::path::PathBuf;
use std::time::Instant;

use engine::{Engine, EngineConfig};
use pipeline::{run_generate, run_readout, GenerateResult, ReadoutResult};
use timing::Timings;

/// openjev-cli: constrained-readout vs generation benchmark for a single MCQ prompt against a
/// locally-loaded GGUF model (CPU-only, `llama-cpp-2`).
#[derive(Parser, Debug)]
#[command(name = "openjev-cli")]
struct Cli {
    /// Registry model id (see `crates/models/src/download.rs::REGISTRY`), e.g. "qwen3-0.6b".
    #[arg(long)]
    model: String,

    /// The MCQ prompt text.
    #[arg(long)]
    prompt: String,

    /// Comma-separated option labels, e.g. "A,B".
    #[arg(long, value_delimiter = ',')]
    options: Vec<String>,

    /// Output format. Only "json" is currently supported.
    #[arg(long, default_value = "json")]
    format: String,

    /// When set, skips the Laya comparison method (calling the separately-running `laya serve`
    /// process) — `laya` stays `null` in the output and both `laya_*` timing fields stay 0.
    /// When absent (default), Laya IS called; if `laya serve` is unreachable the request still
    /// succeeds (readout/generate are unaffected) but `laya` is `null` and a warning is printed
    /// to stderr.
    #[arg(long)]
    skip_laya: bool,
}

#[derive(Serialize)]
struct CliOutput {
    model: String,
    prompt: String,
    options: Vec<String>,
    timings: Timings,
    readout: ReadoutResult,
    generate: GenerateResult,
    laya: Option<models::LayaScoreResult>,
}

fn main() {
    let cli = Cli::parse();

    if cli.format != "json" {
        eprintln!("error: only --format json is currently supported");
        std::process::exit(2);
    }
    if cli.options.is_empty() {
        eprintln!("error: --options must list at least one option, e.g. --options A,B");
        std::process::exit(2);
    }

    if let Err(e) = run(&cli) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

fn run(cli: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let mut timings = Timings::default();

    // 1. models::ensure_downloaded + Engine::load, timed together as model_load_ms.
    let t0 = Instant::now();
    let entry = models::find(&cli.model)?;
    let model_path: PathBuf = models::ensure_downloaded(entry)?;
    let mut engine = Engine::load(&model_path, EngineConfig::default())?;
    timings.model_load_ms = t0.elapsed().as_millis();

    // 2. Warmup: one throwaway decode + reset so the first real pass isn't paying one-time
    // allocation/paging costs baked into `constrained_readout_ms`.
    let t0 = Instant::now();
    let warmup_tokens = engine.tokenize("Hello")?;
    engine.decode_prompt(&warmup_tokens)?;
    engine.reset_context();
    timings.warmup_ms = t0.elapsed().as_millis();

    // 3. Tokenize timing (standalone measurement of the tokenize step on the real prompt;
    // run_readout below re-tokenizes internally as part of its own work).
    let t0 = Instant::now();
    let _ = engine.tokenize(&cli.prompt)?;
    timings.tokenize_ms = t0.elapsed().as_millis();

    // 4. Constrained readout.
    let t0 = Instant::now();
    let readout = run_readout(&mut engine, &cli.prompt, &cli.options)?;
    timings.constrained_readout_ms = t0.elapsed().as_millis();

    // 5. Reset KV-cache between the two independent pipeline runs so `run_generate` isn't
    // contaminated by `run_readout`'s decoded state.
    engine.reset_context();

    // 6. Greedy JSON generation.
    let t0 = Instant::now();
    let generate = run_generate(
        &mut engine,
        &cli.prompt,
        &cli.options,
        entry.suppress_think,
        entry.think_budget,
    )?;
    timings.generation_ms = t0.elapsed().as_millis();

    // 7. Laya (3rd comparison method): calls the separately-running `laya serve` process.
    // `laya_model_load_ms` stays 0 (the model loads once at `laya serve` startup, not
    // per-request — see `pipeline::laya::run_laya`'s doc comment). Unreachable/erroring Laya
    // degrades gracefully: readout/generate above already succeeded, so we don't fail the whole
    // run — `laya` stays `None` and a warning goes to stderr instead.
    let laya = if cli.skip_laya {
        None
    } else {
        let t0 = Instant::now();
        let result = pipeline::run_laya(&cli.prompt, &cli.options);
        timings.laya_inference_ms = t0.elapsed().as_millis();
        match result {
            Ok(laya) => Some(laya),
            Err(e) => {
                eprintln!("warning: laya unavailable, continuing without it: {e}");
                None
            }
        }
    };

    let output = CliOutput {
        model: cli.model.clone(),
        prompt: cli.prompt.clone(),
        options: cli.options.clone(),
        timings,
        readout,
        generate,
        laya,
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
