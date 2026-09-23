// Fine-grained perf-logging infrastructure (D_11b Task 1).
//
// Design:
// - `PerfSpan::start(name)` captures `Instant::now()` and, on `Drop`, computes the elapsed
//   duration and does one cheap `mpsc::Sender::send` — no file I/O, no locking, no syscalls on
//   the hot path itself. All actual file I/O happens on a single dedicated background thread
//   ("perf-logger") that drains the channel.
// - Gated by the `PERF_LOG=1` env var, read exactly once into a `OnceLock<bool>`. When
//   disabled, `PerfSpan::start` skips `Instant::now()` entirely and returns a span whose `Drop`
//   is a single `if` check — near-zero cost, and the background thread is never even spawned
//   (lazily started on the first *enabled* span).
// - Output: `logs/perf/perf-<YYYYMMDD-HHmm>.jsonl`, one JSON object per line, rotating to a new
//   file whenever the wall-clock minute changes (checked against `Local::now()` fresh each time
//   a record is written on the background thread — NOT "1 minute since the first write").
// - Safe to call from many threads concurrently: `mpsc::Sender` is `Clone + Send + Sync`-free
//   (in std, `Sender` is `Send` but not `Sync`; each thread gets its own clone via
//   `sender().clone()`), so there is never a shared mutex guarding file I/O — only a single
//   consumer (the background thread) ever touches the `File`.
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;
use std::sync::mpsc::{self, Sender};
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

const LOG_DIR: &str = "logs/perf";

static PERF_LOG_ENABLED: OnceLock<bool> = OnceLock::new();
static WRITER_TX: OnceLock<Sender<PerfRecord>> = OnceLock::new();

/// Whether perf logging is turned on for this process, checked once via `PERF_LOG=1`.
fn enabled() -> bool {
    *PERF_LOG_ENABLED.get_or_init(|| {
        std::env::var("PERF_LOG")
            .map(|v| v == "1")
            .unwrap_or(false)
    })
}

/// Returns the process-wide sender to the background writer thread, spawning that thread on
/// first call. Only ever called when `enabled()` is true, so the thread never spawns in the
/// (default) disabled case.
fn writer_sender() -> Sender<PerfRecord> {
    WRITER_TX
        .get_or_init(|| {
            let (tx, rx) = mpsc::channel::<PerfRecord>();
            std::thread::Builder::new()
                .name("perf-logger".to_string())
                .spawn(move || writer_thread(rx))
                .expect("failed to spawn perf-logger background thread");
            tx
        })
        .clone()
}

#[derive(Serialize)]
struct PerfRecord {
    timestamp_micros: u128,
    span_name: &'static str,
    duration_micros: u64,
    thread_id: String,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    metadata: HashMap<&'static str, String>,
}

/// Background writer-thread loop: drains `rx`, rotating the open file to a new
/// `logs/perf/perf-<YYYYMMDD-HHmm>.jsonl` whenever the wall-clock minute bucket changes.
fn writer_thread(rx: mpsc::Receiver<PerfRecord>) {
    let mut current_bucket = String::new();
    let mut writer: Option<BufWriter<File>> = None;

    for record in rx.iter() {
        let bucket = chrono::Local::now().format("%Y%m%d-%H%M").to_string();
        if writer.is_none() || bucket != current_bucket {
            if let Some(mut w) = writer.take() {
                let _ = w.flush();
            }
            match open_bucket_file(&bucket) {
                Ok(f) => {
                    writer = Some(BufWriter::new(f));
                    current_bucket = bucket;
                }
                Err(e) => {
                    eprintln!("perf-logger: failed to open perf log file: {e}");
                    continue;
                }
            }
        }
        if let Some(w) = writer.as_mut() {
            match serde_json::to_string(&record) {
                Ok(line) => {
                    let _ = writeln!(w, "{line}");
                    // Flushed per-record: this is the background thread, not the hot path, and
                    // the whole point of this tool is that the coordinator can tail near-real-
                    // time logs while the benchmark harness is still running.
                    let _ = w.flush();
                }
                Err(e) => eprintln!("perf-logger: failed to serialize record: {e}"),
            }
        }
    }
    if let Some(mut w) = writer.take() {
        let _ = w.flush();
    }
}

fn open_bucket_file(bucket: &str) -> std::io::Result<File> {
    fs::create_dir_all(LOG_DIR)?;
    let path = Path::new(LOG_DIR).join(format!("perf-{bucket}.jsonl"));
    OpenOptions::new().create(true).append(true).open(path)
}

fn now_micros() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_micros())
        .unwrap_or(0)
}

fn thread_id_string() -> String {
    format!("{:?}", std::thread::current().id())
}

/// RAII perf-measurement guard. Create via [`PerfSpan::start`] (or the [`crate::perf_span!`]
/// macro); on `Drop`, records `{timestamp_micros, span_name, duration_micros, thread_id}` plus
/// any metadata attached via [`Self::set`] to the background perf-logger writer thread.
///
/// When perf logging is disabled (`PERF_LOG` unset/not `"1"`), `start()` skips `Instant::now()`
/// entirely and `Drop` is a single branch — safe to sprinkle liberally without measurably
/// affecting the paths it instruments (see D_11b's overhead measurement in the loop report).
pub struct PerfSpan {
    name: &'static str,
    start: Option<Instant>,
    metadata: HashMap<&'static str, String>,
}

impl PerfSpan {
    /// Starts a new span named `name` (convention: `"module::function_or_block_name"`).
    #[inline]
    pub fn start(name: &'static str) -> Self {
        if !enabled() {
            return Self {
                name,
                start: None,
                metadata: HashMap::new(),
            };
        }
        Self {
            name,
            start: Some(Instant::now()),
            metadata: HashMap::new(),
        }
    }

    /// Attaches a key-value metadata pair to this span's eventual log record (e.g.
    /// `span.set("model", "qwen3-0.6b")`). A no-op when perf logging is disabled.
    #[inline]
    pub fn set(&mut self, key: &'static str, value: impl Into<String>) -> &mut Self {
        if self.start.is_some() {
            self.metadata.insert(key, value.into());
        }
        self
    }
}

impl Drop for PerfSpan {
    #[inline]
    fn drop(&mut self) {
        let Some(start) = self.start else {
            return;
        };
        let duration_micros = start.elapsed().as_micros() as u64;
        let record = PerfRecord {
            timestamp_micros: now_micros(),
            span_name: self.name,
            duration_micros,
            thread_id: thread_id_string(),
            metadata: std::mem::take(&mut self.metadata),
        };
        // Cheap channel send; never blocks the caller on file I/O. Best-effort: a full/closed
        // channel silently drops the record rather than panicking the instrumented call site.
        let _ = writer_sender().send(record);
    }
}

/// Instruments a call site with near-zero overhead when perf logging is disabled.
///
/// ```ignore
/// let _s = timing::perf_span!("engine::decode_prompt");
/// // ... or with metadata:
/// let _s = timing::perf_span!("engine::decode_prompt", "model" => model_id.clone());
/// ```
#[macro_export]
macro_rules! perf_span {
    ($name:expr) => {
        $crate::logger::PerfSpan::start($name)
    };
    ($name:expr, $($key:expr => $val:expr),+ $(,)?) => {{
        let mut _perf_span = $crate::logger::PerfSpan::start($name);
        $( _perf_span.set($key, $val); )+
        _perf_span
    }};
}
