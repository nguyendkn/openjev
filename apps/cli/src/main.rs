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

    /// Accepted for forward-compat with the planned Laya scoring path — currently a NO-OP:
    /// Laya (`ggmlc-run`-backed classification scoring) is not implemented yet (Loop 3+).
    /// This flag does not change any current behavior.
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
    let generate = run_generate(&mut engine, &cli.prompt, &cli.options)?;
    timings.generation_ms = t0.elapsed().as_millis();

    // Laya (laya_model_load_ms / laya_inference_ms) is not implemented yet (Loop 3+); left at
    // Timings::default()'s 0. `--skip-laya` is currently a no-op for the same reason.

    let output = CliOutput {
        model: cli.model.clone(),
        prompt: cli.prompt.clone(),
        options: cli.options.clone(),
        timings,
        readout,
        generate,
    };
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}
