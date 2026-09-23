// THROWAWAY benchmark binary (Loop 14, task 1): measures aggregate throughput and latency
// percentiles for worker-pool topologies (N workers x T threads each) WITHOUT going through
// HTTP, so the numbers reflect engine/CPU behaviour only. Not production code — it exists to
// pick the (N, T) the real pool in `app.rs` ships with.
//
// Each worker owns its own `Engine` (same `!Send` confinement as the server's worker thread)
// and runs the exact per-request body `run_bench` runs (reset -> tokenize -> readout -> reset
// -> generate), minus Laya (a separate process, not part of the CPU-topology question).
use engine::{Engine, EngineConfig};
use pipeline::{run_generate, run_readout};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Instant;

const PROMPT: &str = "Which is larger, 3 or 5? Answer A for 3, B for 5.";
// Kept deliberately small + spaced out: this probe saturates the CPU by design and shares the
// box with the LIVE `openjev-server`, so each cell is a short burst followed by an idle gap
// that lets production serve real requests instead of starving behind the benchmark.
const DEFAULT_REQS_PER_CELL: usize = 8;
/// Idle seconds between cells, so production gets CPU back between bursts.
const COOLDOWN_SECS: u64 = 8;

struct Job {
    done: mpsc::Sender<u128>,
}

fn opts() -> Vec<String> {
    vec!["A".to_string(), "B".to_string()]
}

fn spawn_worker(
    model_path: PathBuf,
    n_threads: i32,
) -> (mpsc::Sender<Job>, std::thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<Job>();
    let handle = std::thread::spawn(move || {
        let cfg = EngineConfig {
            n_threads,
            ..EngineConfig::default()
        };
        let mut engine = Engine::load(&model_path, cfg).expect("engine load");
        // Same warmup the server does, so first measured request isn't skewed.
        let warm = engine.tokenize("Hello").expect("tokenize warmup");
        engine.decode_prompt(&warm).expect("decode warmup");
        engine.reset_context();

        let options = opts();
        while let Ok(job) = rx.recv() {
            let t0 = Instant::now();
            engine.reset_context();
            let _ = engine.tokenize(PROMPT).expect("tokenize");
            let _ = run_readout(&mut engine, PROMPT, &options).expect("readout");
            engine.reset_context();
            let _ = run_generate(&mut engine, PROMPT, &options).expect("generate");
            let _ = job.done.send(t0.elapsed().as_millis());
        }
    });
    (tx, handle)
}

/// Nearest-rank percentile over `sorted` (ms).
fn pct(sorted: &[u128], p: f64) -> u128 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((p / 100.0) * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn main() {
    let model_id = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "qwen3-0.6b".to_string());
    let entry = models::find(&model_id).expect("model id");
    let model_path = models::ensure_downloaded(entry).expect("model download");

    // One (N workers, threads per worker) topology per invocation — run separately so the
    // operator can health-check production between topologies instead of pinning every core
    // for one long uninterrupted sweep. N*T is kept near the 28-32 core budget.
    let n_workers: usize = std::env::args()
        .nth(2)
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);
    let n_threads: i32 = std::env::args()
        .nth(3)
        .and_then(|v| v.parse().ok())
        .unwrap_or(28);
    let reqs_per_cell: usize = std::env::args()
        .nth(4)
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_REQS_PER_CELL);
    let topologies: [(usize, i32); 1] = [(n_workers, n_threads)];
    let concurrencies = [1usize, 2, 4, 8];

    println!("model={model_id} reqs_per_cell={reqs_per_cell}");
    println!("topology,workers,threads_per_worker,concurrency,wall_s,throughput_rps,p50_ms,p95_ms,mean_ms");

    for (n_workers, n_threads) in topologies {
        let load_t0 = Instant::now();
        let mut senders = Vec::with_capacity(n_workers);
        let mut handles = Vec::with_capacity(n_workers);
        for _ in 0..n_workers {
            let (tx, h) = spawn_worker(model_path.clone(), n_threads);
            senders.push(tx);
            handles.push(h);
        }
        // Force every worker to finish loading+warmup before timing anything: send one
        // throwaway request to each and wait for all of them.
        let (warm_tx, warm_rx) = mpsc::channel::<u128>();
        for s in &senders {
            s.send(Job {
                done: warm_tx.clone(),
            })
            .expect("warm send");
        }
        drop(warm_tx);
        for _ in 0..n_workers {
            warm_rx.recv().expect("warm recv");
        }
        eprintln!(
            "[{n_workers}x{n_threads}] pool ready in {:.1}s",
            load_t0.elapsed().as_secs_f64()
        );

        for &c in &concurrencies {
            // Cooldown BEFORE each measured cell: gives the live server a quiet window and
            // also means the cell starts from a cold-ish scheduler state each time.
            std::thread::sleep(std::time::Duration::from_secs(COOLDOWN_SECS));
            let (done_tx, done_rx) = mpsc::channel::<u128>();
            let mut sent = 0usize;
            let mut received = 0usize;
            let mut rr = 0usize;
            let mut lat: Vec<u128> = Vec::with_capacity(reqs_per_cell);
            let start = Instant::now();
            while sent < c.min(reqs_per_cell) {
                senders[rr % n_workers]
                    .send(Job {
                        done: done_tx.clone(),
                    })
                    .expect("send");
                rr += 1;
                sent += 1;
            }
            while received < reqs_per_cell {
                let ms = done_rx.recv().expect("recv");
                lat.push(ms);
                received += 1;
                if sent < reqs_per_cell {
                    senders[rr % n_workers]
                        .send(Job {
                            done: done_tx.clone(),
                        })
                        .expect("send");
                    rr += 1;
                    sent += 1;
                }
            }
            let wall = start.elapsed().as_secs_f64();
            lat.sort_unstable();
            let mean = lat.iter().sum::<u128>() as f64 / lat.len() as f64;
            println!(
                "{n_workers}x{n_threads},{n_workers},{n_threads},{c},{:.2},{:.3},{},{},{:.0}",
                wall,
                reqs_per_cell as f64 / wall,
                pct(&lat, 50.0),
                pct(&lat, 95.0),
                mean
            );
        }

        drop(senders);
        for h in handles {
            let _ = h.join();
        }
    }
}
