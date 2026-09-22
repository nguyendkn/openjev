pub mod error;

use error::EngineError;
use llama_cpp_2::context::params::LlamaContextParams;
use llama_cpp_2::context::LlamaContext;
use llama_cpp_2::llama_backend::LlamaBackend;
use llama_cpp_2::llama_batch::LlamaBatch;
use llama_cpp_2::model::params::LlamaModelParams;
use llama_cpp_2::model::{AddBos, LlamaChatMessage, LlamaModel, Special};
use llama_cpp_2::sampling::LlamaSampler;
use llama_cpp_2::token::LlamaToken;
use std::num::NonZeroU32;
use std::path::Path;
use std::sync::OnceLock;

static BACKEND: OnceLock<LlamaBackend> = OnceLock::new();

/// Returns the process-wide shared llama.cpp backend, initializing it on first call.
/// `LlamaBackend::init()` is a process-global singleton (an internal `AtomicBool` in
/// `llama-cpp-2` rejects a second `init()` call with `BackendAlreadyInitialized` while any
/// previous `LlamaBackend` value returned by an earlier `init()` is still alive — see
/// `llama-cpp-2`'s `llama_backend.rs`). `Engine::load` used to call `LlamaBackend::init()`
/// once per `Engine` and store the result as an owned field; that works for a single cached
/// model (`apps/cli`, one `Engine` per process) but breaks `apps/server`'s multi-model cache
/// (`HashMap<String, Engine>`): loading a second, distinct model while the first model's
/// `Engine` (and its `LlamaBackend`) is still alive in the cache fails here (reproduced via a
/// real `POST /bench` for `minicpm5-2b` on a server process that had already cached
/// `qwen3-0.6b`: `"llama backend init failed: BackendAlreadyInitialized"`). Sharing one
/// `'static` backend across all `Engine`s (leaked for the process's lifetime — never freed
/// until process exit, which is fine since the process keeps its model cache alive for its
/// whole life anyway) fixes this. Not guarded against concurrent first-call races: the only
/// two callers never call this concurrently (`apps/cli`: one `Engine`, one thread;
/// `apps/server`: one dedicated worker thread processes jobs sequentially — see `AppState`'s
/// doc comment in `apps/server/src/app.rs`).
fn shared_backend() -> Result<&'static LlamaBackend, EngineError> {
    if let Some(backend) = BACKEND.get() {
        return Ok(backend);
    }
    let backend = LlamaBackend::init().map_err(|e| EngineError::BackendInit(e.to_string()))?;
    Ok(BACKEND.get_or_init(|| backend))
}

/// Configuration for [`Engine::load`]. CPU-only: `llama-cpp-2` is used with its default
/// feature set (no `cuda`/`vulkan`/`rocm`/`opencl` cargo features enabled — see
/// `crates/engine/Cargo.toml`), and `n_gpu_layers` is pinned to `0` explicitly below so no
/// layers are offloaded even if a GPU backend happened to be compiled in.
#[derive(Debug, Clone, Copy)]
pub struct EngineConfig {
    /// Threads used for both single-token and batch decode. Default leaves headroom on a
    /// 32-core box (28 of 32).
    pub n_threads: i32,
    /// Context window size (tokens).
    pub n_ctx: u32,
    /// Max tokens processed per `decode()` call.
    pub n_batch: u32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            n_threads: 28,
            n_ctx: 4096,
            n_batch: 512,
        }
    }
}

/// A loaded GGUF model plus a persistent inference context, backed by `llama-cpp-2`.
///
/// `model` is heap-allocated (`Box`) so its address is stable even if `Engine` itself is
/// moved; `ctx` borrows from it (and from the process-wide `shared_backend()`) with a
/// lifetime unsafely widened to `'static` to make the two co-resident in one struct (a
/// standard self-referential-via-heap-indirection pattern for this crate). Field declaration
/// order (`ctx` before `model`) controls drop order, ensuring `ctx` is dropped before the
/// `model` it borrows from. The backend itself is not an owned field (see `shared_backend()`)
/// — it is a process-wide `'static` singleton shared by every `Engine`, so it needs no drop
/// ordering relative to `ctx`/`model` here.
pub struct Engine {
    ctx: LlamaContext<'static>,
    model: Box<LlamaModel>,
    n_vocab: i32,
}

impl Engine {
    /// Loads a GGUF model from `model_path` and creates a persistent CPU-only inference
    /// context per `config`.
    pub fn load(model_path: &Path, config: EngineConfig) -> Result<Self, EngineError> {
        let backend = shared_backend()?;

        // CPU-only: n_gpu_layers = 0 explicitly (llama-cpp-2's own default is -1 = "auto
        // offload if a GPU backend is compiled in"; we never build with one, but pin this
        // anyway so intent is explicit and future Cargo.toml changes can't silently offload).
        let model_params = LlamaModelParams::default().with_n_gpu_layers(0);
        let model = LlamaModel::load_from_file(backend, model_path, &model_params)
            .map_err(|e| EngineError::ModelLoad(e.to_string()))?;
        let model = Box::new(model);
        let n_vocab = model.n_vocab();

        // SAFETY: `model` is heap-allocated via `Box` and stored below in `Self`; its
        // contents never move for the lifetime of `Engine` even if `Engine` itself moves
        // (moving a `Box` moves only the pointer, not the pointee). `ctx` is declared before
        // `model` in the struct so it is dropped first, before the borrow it holds would
        // become dangling.
        let model_ref: &'static LlamaModel =
            unsafe { std::mem::transmute::<&LlamaModel, &'static LlamaModel>(&model) };

        let ctx_params = LlamaContextParams::default()
            .with_n_ctx(NonZeroU32::new(config.n_ctx))
            .with_n_batch(config.n_batch)
            .with_n_threads(config.n_threads)
            .with_n_threads_batch(config.n_threads);
        let ctx = model_ref
            .new_context(backend, ctx_params)
            .map_err(|e| EngineError::ContextInit(e.to_string()))?;

        Ok(Self {
            ctx,
            model,
            n_vocab,
        })
    }

    /// Vocab size of the loaded model.
    pub fn n_vocab(&self) -> i32 {
        self.n_vocab
    }

    /// Tokenizes `text`, adding a BOS token. Used for full prompts fed to `decode_prompt`.
    pub fn tokenize(&self, text: &str) -> Result<Vec<LlamaToken>, EngineError> {
        self.model
            .str_to_token(text, AddBos::Always)
            .map_err(|e| EngineError::Tokenize(e.to_string()))
    }

    /// Tokenizes `text` without a BOS token. Used to resolve a short candidate-option label
    /// (e.g. `"A"`) to its raw token id for constrained readout, where BOS would just be
    /// leading noise.
    pub fn tokenize_no_bos(&self, text: &str) -> Result<Vec<LlamaToken>, EngineError> {
        self.model
            .str_to_token(text, AddBos::Never)
            .map_err(|e| EngineError::Tokenize(e.to_string()))
    }

    /// Applies the model's embedded chat template (ChatML-family for Qwen3:
    /// `<|im_start|>role\n...<|im_end|>`) to a single-turn user message, with a hand-built
    /// ChatML fallback if the GGUF has no chat template metadata.
    pub fn apply_chat_template(&self, user_content: &str) -> Result<String, EngineError> {
        match self.model.chat_template(None) {
            Ok(tmpl) => {
                let msg = LlamaChatMessage::new("user".to_string(), user_content.to_string())
                    .map_err(|e| EngineError::ChatTemplate(e.to_string()))?;
                self.model
                    .apply_chat_template(&tmpl, &[msg], true)
                    .map_err(|e| EngineError::ChatTemplate(e.to_string()))
            }
            Err(_) => Ok(format!(
                "<|im_start|>user\n{user_content}<|im_end|>\n<|im_start|>assistant\n"
            )),
        }
    }

    /// Feeds `tokens` through the model in one batch and returns the logits for the last
    /// position (i.e. the distribution over the next token). Only the last token's logits are
    /// requested from `llama_decode`, matching Decisions Locked's "restrict before softmax"
    /// intent one level down (full-vocab logits here; callers do the restriction).
    pub fn decode_prompt(&mut self, tokens: &[LlamaToken]) -> Result<Vec<f32>, EngineError> {
        if tokens.is_empty() {
            return Err(EngineError::BatchAdd("empty token sequence".to_string()));
        }
        let mut batch = LlamaBatch::new(tokens.len(), 1);
        let last = tokens.len() - 1;
        for (i, token) in tokens.iter().enumerate() {
            batch
                .add(*token, i as i32, &[0], i == last)
                .map_err(|e| EngineError::BatchAdd(e.to_string()))?;
        }
        self.ctx
            .decode(&mut batch)
            .map_err(|e| EngineError::Decode(e.to_string()))?;
        Ok(self.ctx.get_logits_ith(last as i32).to_vec())
    }

    /// Decodes a single already-generated token (used in the greedy generation loop) at
    /// `pos`, returning the next-token logits.
    pub fn decode_next(&mut self, token: LlamaToken, pos: i32) -> Result<Vec<f32>, EngineError> {
        let mut batch = LlamaBatch::new(1, 1);
        batch
            .add(token, pos, &[0], true)
            .map_err(|e| EngineError::BatchAdd(e.to_string()))?;
        self.ctx
            .decode(&mut batch)
            .map_err(|e| EngineError::Decode(e.to_string()))?;
        Ok(self.ctx.get_logits_ith(0).to_vec())
    }

    // --- Raw-token-id variants -------------------------------------------------------------
    // `LlamaToken` is a `llama-cpp-2` type; downstream crates (e.g. `pipeline`) don't depend
    // on `llama-cpp-2` directly, so the generation loop is exposed here in terms of plain
    // `i32` token ids instead, keeping `llama-cpp-2` an implementation detail of `engine`.

    /// Same as [`Self::decode_next`], keyed by raw token id.
    pub fn decode_next_id(&mut self, token_id: i32, pos: i32) -> Result<Vec<f32>, EngineError> {
        self.decode_next(LlamaToken::new(token_id), pos)
    }

    /// Same as [`Self::is_eog`], keyed by raw token id.
    pub fn is_eog_id(&self, token_id: i32) -> bool {
        self.is_eog(LlamaToken::new(token_id))
    }

    /// Same as [`Self::token_to_piece`], keyed by raw token id.
    pub fn token_to_piece_id(&self, token_id: i32) -> Result<String, EngineError> {
        self.token_to_piece(LlamaToken::new(token_id))
    }

    /// Greedy argmax over full-vocab `logits` (`LlamaSampler::greedy()`'s sample() also needs
    /// a live `LlamaContext`; this pure variant lets callers greedy-pick over restricted
    /// logit sets too, e.g. constrained readout).
    pub fn sample_greedy_from_logits(logits: &[f32]) -> usize {
        let mut best_idx = 0usize;
        let mut best_val = f32::NEG_INFINITY;
        for (i, &v) in logits.iter().enumerate() {
            if v > best_val {
                best_val = v;
                best_idx = i;
            }
        }
        best_idx
    }

    /// Greedy-samples the next token directly from the context's last-decoded logits using
    /// `llama-cpp-2`'s own `LlamaSampler::greedy()`.
    pub fn sample_greedy(&self) -> LlamaToken {
        let mut sampler = LlamaSampler::greedy();
        sampler.sample(&self.ctx, 0)
    }

    /// True if `token` is an end-of-generation token for this model.
    pub fn is_eog(&self, token: LlamaToken) -> bool {
        self.model.is_eog_token(token)
    }

    /// Converts a single token to its text piece (special tokens rendered as plaintext).
    #[allow(deprecated)]
    pub fn token_to_piece(&self, token: LlamaToken) -> Result<String, EngineError> {
        self.model
            .token_to_str(token, Special::Plaintext)
            .map_err(|e| EngineError::Tokenize(e.to_string()))
    }

    /// Clears the KV cache for all sequences. MUST be called between independent pipeline
    /// runs sharing this `Engine` (e.g. between `run_readout` and `run_generate`) so the
    /// second run's positions/attention are not contaminated by the first run's state.
    pub fn reset_context(&mut self) {
        self.ctx.clear_kv_cache();
    }
}
