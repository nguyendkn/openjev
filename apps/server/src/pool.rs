// Loop 14: the engine-worker *pool*. Replaces the single dedicated engine-worker thread with
// N independent workers so concurrent `/bench` requests can actually run in parallel on the
// 32-core box instead of queueing behind one serialized worker.
//
// Each worker is exactly what the old single worker was — its own OS thread, its own
// `HashMap<String, Engine>` cache, its own `EngineConfig`, its own G13 respawn supervisor —
// so a panic in one worker respawns only that worker (empty cache) and leaves the other N-1
// serving normally. `Engine` is `!Send` (raw `llama-cpp-2` FFI pointers, self-referential
// `LlamaContext<'static>`), so it stays thread-confined in all N workers, same as before.
use crate::app::{run_bench, ApiError, BenchReport, BenchRequest};
use engine::Engine;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot};

/// Default pool size, picked from Loop 14's real topology benchmark on the 32-core target
/// (qwen3-0.6b, 8 requests per cell, aggregate req/s):
/// `1x28` 0.475/0.468/0.469/0.460 at concurrency 1/2/4/8 (flat — fully serialized),
/// `2x14` 0.388/0.688/0.635/0.665, `4x7` 0.289/0.517/**0.903**/0.865, `8x4`
/// 0.196/0.347/0.645/0.068 (collapses at 8x4xc=8 — 32 engine threads oversubscribe the box).
/// `4 workers x 7 threads` is the peak (1.90x the single-worker baseline) and degrades
/// gracefully past its peak concurrency, so it is the default; `--workers`/`--threads` can
/// still override both halves at startup.
pub const DEFAULT_WORKERS: usize = 4;

/// One benchmark job handed to an engine-worker thread.
pub struct WorkerJob {
    pub req: BenchRequest,
    pub resp: oneshot::Sender<Result<BenchReport, ApiError>>,
}

/// Dispatcher-side handle for one worker thread.
struct WorkerHandle {
    tx: mpsc::Sender<WorkerJob>,
    /// Jobs currently sent-but-not-answered for this worker. Maintained by the async
    /// dispatcher (`Pool::dispatch`), not the worker thread — it's the queue-depth signal the
    /// routing policy balances on.
    inflight: Arc<AtomicUsize>,
    /// Model ids this worker currently has loaded in its own cache. Written by the worker
    /// thread (after a successful job) and cleared by its supervisor on respawn; read by the
    /// dispatcher as the model-affinity hint. A brief staleness here only costs an extra
    /// model load, never correctness.
    cached: Arc<Mutex<HashSet<String>>>,
}

/// The pool of engine workers plus the routing policy over them.
///
/// **Routing policy — least-inflight, model-affinity tie-break.** For each request the
/// dispatcher scores every worker by `(inflight_jobs, 0 if it already has this model else 1)`
/// and picks the minimum (lowest index wins a full tie). Consequences, both of which the loop
/// needs:
/// - *Idle pool* (the common case): every worker has `inflight == 0`, so the affinity term
///   decides and the request goes back to the worker that already has that model loaded.
///   A model therefore is NOT re-loaded into all N workers just because requests are spread
///   over time — `model_load_ms` stays 0 after the first load, and in the steady state at
///   most `min(N, distinct_models_ever_concurrent)` copies exist, not N copies per model.
/// - *Busy pool*: the affine worker's `inflight` is non-zero, so a second concurrent request
///   (even for the SAME model) goes to an idle worker instead of queueing. That is the actual
///   point of this loop — pure `hash(model) % N` routing would have bounded RAM perfectly but
///   kept same-model concurrency fully serialized, which is the dominant real request shape
///   here (3 models, one hot).
///
/// RAM cost of the looser policy is affordable and was the deciding factor: all three LLMs
/// together are ~4.7GB (0.7 + 1.5 + 2.5), so even the pathological "every worker holds every
/// model" case is ~19GB at N=4 against 62GB total, and `mmap` (on by default in
/// `llama-cpp-2`) makes the read-only weight pages shared in the OS page cache anyway — the
/// genuinely per-worker cost is the KV/compute buffers, not the weights.
pub struct Pool {
    workers: Vec<WorkerHandle>,
}

impl Pool {
    /// Spawns `n_workers` worker threads, each with its own supervisor, model cache and
    /// `EngineConfig { n_threads, n_batch, n_gpu_layers }`.
    pub fn new(n_workers: usize, n_threads: i32, n_batch: u32, n_gpu_layers: i32) -> Self {
        let n_workers = n_workers.max(1);
        let mut workers = Vec::with_capacity(n_workers);
        for worker_id in 0..n_workers {
            // Per-worker queue depth: the dispatcher already load-balances on `inflight`, so
            // this only needs to absorb bursts, not act as the main queue.
            let (tx, rx) = mpsc::channel::<WorkerJob>(32);
            let cached: Arc<Mutex<HashSet<String>>> = Arc::new(Mutex::new(HashSet::new()));
            let cached_worker = Arc::clone(&cached);
            std::thread::Builder::new()
                .name(format!("engine-worker-{worker_id}"))
                .spawn(move || {
                    supervisor_loop(worker_id, rx, cached_worker, n_threads, n_batch, n_gpu_layers)
                })
                .expect("spawn engine worker thread");
            workers.push(WorkerHandle {
                tx,
                inflight: Arc::new(AtomicUsize::new(0)),
                cached,
            });
        }
        Self { workers }
    }

    /// Picks a worker per the policy documented on [`Pool`].
    fn pick(&self, model: &str) -> usize {
        let mut best = 0usize;
        let mut best_key = (usize::MAX, 1u8);
        for (idx, w) in self.workers.iter().enumerate() {
            let inflight = w.inflight.load(Ordering::Acquire);
            let has_model = w
                .cached
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .contains(model);
            let key = (inflight, u8::from(!has_model));
            if key < best_key {
                best_key = key;
                best = idx;
            }
        }
        best
    }

    /// Routes one request to a worker and awaits its answer. Never blocks the async runtime:
    /// the blocking inference happens on the worker's own OS thread.
    pub async fn dispatch(&self, req: BenchRequest) -> Result<BenchReport, ApiError> {
        let idx = self.pick(&req.model);
        let worker = &self.workers[idx];
        worker.inflight.fetch_add(1, Ordering::AcqRel);
        // Decrements on every exit path (send failure, dropped response, success) so a failed
        // request can't permanently inflate this worker's apparent queue depth and exile it
        // from routing.
        let _guard = InflightGuard(Arc::clone(&worker.inflight));
        let (resp_tx, resp_rx) = oneshot::channel();
        worker
            .tx
            .send(WorkerJob { req, resp: resp_tx })
            .await
            .map_err(|_| {
                ApiError::Internal(format!("engine worker {idx} unavailable (channel closed)"))
            })?;
        resp_rx
            .await
            .map_err(|_| ApiError::Internal(format!("engine worker {idx} dropped the response")))?
    }
}

struct InflightGuard(Arc<AtomicUsize>);

impl Drop for InflightGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

/// G13 (now per-worker): owns worker `worker_id`'s *outer* loop and recovers it from a panic
/// instead of letting the thread (and the `mpsc::Receiver` it owns) die permanently.
///
/// Design choice — respawn-with-empty-cache, not per-job `catch_unwind` around a long-lived
/// `HashMap<String, Engine>`: `Engine` (`crates/engine`) is a self-referential struct built
/// with `unsafe` lifetime-widening (`LlamaContext<'static>` borrowing from a heap-boxed
/// `LlamaModel` via `std::mem::transmute`), wrapping raw `llama-cpp-2`/FFI state.
/// `std::panic::catch_unwind` only guarantees the *Rust* stack unwinds safely — it does NOT
/// guarantee that whatever `Engine`'s FFI calls (`ctx.decode`, which crosses into llama.cpp's
/// C layer and mutates its internal KV-cache/position bookkeeping) were doing at the moment
/// of the panic left that FFI-side state correct to keep using. There is no documented
/// `UnwindSafe`/panic-safety audit of `llama-cpp-2`'s internals to lean on, and getting this
/// wrong would fail silently (a corrupted `Engine` answering with subtly wrong results),
/// which is worse than the extra reload cost. So `engines` is created fresh *inside*
/// `worker_loop` on every (re)entry: on panic the entire `HashMap<String, Engine>` is dropped
/// during unwinding and `worker_loop` is immediately called again with a brand-new empty
/// cache — self-healing in-process, no operator restart, at the cost of one reload per model.
/// `rx` is untouched by the panic and safe to reuse across respawns; it is threaded through
/// via `AssertUnwindSafe` since `&mut Receiver` is not `UnwindSafe` by default.
///
/// Pool-specific parts (Loop 14): this supervisor is per worker, so a panic here respawns
/// ONLY this worker — the other N-1 workers never observe it, keep their caches and keep
/// serving (their `mpsc::Receiver`s and threads are entirely separate). The shared `cached`
/// affinity set is cleared on respawn so the dispatcher stops claiming this worker still
/// holds models that the dropped `HashMap` just took with it.
///
/// The panicking request itself still gets a clean error, not a hang: its `WorkerJob::resp`
/// (a `oneshot::Sender`) is a value local to the panicking `worker_loop` call and is dropped
/// during unwinding without ever calling `.send(..)`; `Pool::dispatch`'s `resp_rx.await` then
/// resolves to `Err`, mapped to `ApiError::Internal("engine worker N dropped the response")`
/// (HTTP 500).
fn supervisor_loop(
    worker_id: usize,
    mut rx: mpsc::Receiver<WorkerJob>,
    cached: Arc<Mutex<HashSet<String>>>,
    n_threads: i32,
    n_batch: u32,
    n_gpu_layers: i32,
) {
    loop {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            worker_loop(worker_id, &mut rx, &cached, n_threads, n_batch, n_gpu_layers)
        }));
        match outcome {
            Ok(()) => return, // rx closed normally (all Senders/AppState clones dropped)
            Err(payload) => {
                cached.lock().unwrap_or_else(|e| e.into_inner()).clear();
                let msg = panic_message(&*payload);
                eprintln!(
                    "engine worker {worker_id} panicked, respawning with an empty model cache \
                     (other workers unaffected): {msg}"
                );
                // loop continues: worker_loop is called again with a fresh HashMap below.
            }
        }
    }
}

/// Best-effort extraction of a panic's message for logging (the `catch_unwind` payload is
/// `Box<dyn Any + Send>`; the two conventional shapes are `&str` for `panic!("literal")` and
/// `String` for `panic!("{}", formatted)`).
fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<non-string panic payload>".to_string()
    }
}

/// One worker-thread lifetime's worth of job processing, from an empty model cache until the
/// channel closes or a panic unwinds out of here (see `supervisor_loop`).
fn worker_loop(
    worker_id: usize,
    rx: &mut mpsc::Receiver<WorkerJob>,
    cached: &Arc<Mutex<HashSet<String>>>,
    n_threads: i32,
    n_batch: u32,
    n_gpu_layers: i32,
) {
    let mut engines: HashMap<String, Engine> = HashMap::new();
    while let Some(job) = rx.blocking_recv() {
        let mut _s = timing::perf_span!("server::worker_loop::job");
        _s.set("model", job.req.model.clone());
        _s.set("worker", worker_id.to_string());
        let model = job.req.model.clone();
        let result = run_bench(&mut engines, job.req, n_threads, n_batch, n_gpu_layers);
        // Publish affinity only on success: on failure the model may never have made it into
        // `engines`, and over-claiming would send future requests to a worker that would then
        // pay the load cost anyway. Under-claiming costs at most one extra load.
        if result.is_ok() {
            cached
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert(model);
        }
        let _ = job.resp.send(result);
    }
}
