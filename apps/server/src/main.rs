mod app;
mod pool;

use clap::Parser;

/// openjev-server: HTTP wrapper around the same readout/generate benchmark as `openjev-cli`,
/// reachable via `POST /bench` and `GET /health`. No auth (explicit user instruction).
#[derive(Parser, Debug)]
#[command(name = "openjev-server")]
struct Cli {
    /// Port to bind on 0.0.0.0 (external reachability, not just localhost).
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// Engine-worker threads in the pool (Loop 14). Each worker holds its own model cache and
    /// runs one request at a time, so this is the server's real concurrency limit; total CPU
    /// demand is `workers * threads`, which should stay at/below the core count. Defaults to
    /// `pool::DEFAULT_WORKERS` (4, the measured throughput winner — see that constant's docs
    /// for the full benchmark table).
    #[arg(long)]
    workers: Option<usize>,

    /// Threads used by the engine for both single-token and batch decode, **per worker**
    /// (passed through to every `EngineConfig` each pool worker loads). Defaults to
    /// `engine::EngineConfig::default().n_threads` when omitted (Loop 6: exposed as a startup
    /// flag so `n_threads` tuning iterations don't require recompiling a different default).
    /// Note since Loop 14: with `--workers N` this is per worker, not for the whole process —
    /// the tuned single-worker value (28) must be divided across the pool (e.g. 4 x 7).
    #[arg(long)]
    threads: Option<i32>,

    /// Max tokens processed per `decode()` call (llama.cpp's `n_batch`), passed through to
    /// every `EngineConfig` this server's workers load. Defaults to
    /// `engine::EngineConfig::default().n_batch` when omitted (Loop 7: exposed as a startup
    /// flag, same pattern as `--threads`, so `n_batch` tuning iterations don't require
    /// recompiling a different default).
    #[arg(long)]
    batch: Option<u32>,

    /// Number of model layers to offload to the GPU, passed through to every `EngineConfig`
    /// this server's workers load. Only has an effect on a `cuda`-feature build (`cargo build
    /// --features engine/cuda`) — a default (CPU-only) build links no GPU offload code path,
    /// so this is accepted but ignored there. Defaults to
    /// `engine::EngineConfig::default().n_gpu_layers` (`0`, no offload) when omitted, same
    /// pattern as `--threads`/`--batch`. See `EngineConfig::n_gpu_layers`'s doc for what a
    /// negative value means.
    #[arg(long)]
    gpu_layers: Option<i32>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let n_workers = cli.workers.unwrap_or(pool::DEFAULT_WORKERS);
    let n_threads = cli
        .threads
        .unwrap_or_else(|| engine::EngineConfig::default().n_threads);
    let n_batch = cli
        .batch
        .unwrap_or_else(|| engine::EngineConfig::default().n_batch);
    let n_gpu_layers = cli
        .gpu_layers
        .unwrap_or_else(|| engine::EngineConfig::default().n_gpu_layers);
    let state = app::AppState::new(n_workers, n_threads, n_batch, n_gpu_layers);
    let router = app::router(state);

    let addr = format!("0.0.0.0:{}", cli.port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: failed to bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    println!(
        "openjev-server listening on {addr} (engine pool: {n_workers} workers x {n_threads} \
         threads, n_batch={n_batch})"
    );
    if let Err(e) = axum::serve(listener, router).await {
        eprintln!("error: server exited: {e}");
        std::process::exit(1);
    }
}
