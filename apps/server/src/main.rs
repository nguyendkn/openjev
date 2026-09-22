mod app;

use clap::Parser;

/// openjev-server: HTTP wrapper around the same readout/generate benchmark as `openjev-cli`,
/// reachable via `POST /bench` and `GET /health`. No auth (explicit user instruction).
#[derive(Parser, Debug)]
#[command(name = "openjev-server")]
struct Cli {
    /// Port to bind on 0.0.0.0 (external reachability, not just localhost).
    #[arg(long, default_value_t = 8080)]
    port: u16,

    /// Threads used by the engine for both single-token and batch decode (passed through to
    /// every `EngineConfig` this server's worker loads). Defaults to
    /// `engine::EngineConfig::default().n_threads` when omitted (Loop 6: exposed as a startup
    /// flag so `n_threads` tuning iterations don't require recompiling a different default).
    #[arg(long)]
    threads: Option<i32>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let n_threads = cli
        .threads
        .unwrap_or_else(|| engine::EngineConfig::default().n_threads);
    let state = app::AppState::new(n_threads);
    let router = app::router(state);

    let addr = format!("0.0.0.0:{}", cli.port);
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: failed to bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    println!("openjev-server listening on {addr}");
    if let Err(e) = axum::serve(listener, router).await {
        eprintln!("error: server exited: {e}");
        std::process::exit(1);
    }
}
