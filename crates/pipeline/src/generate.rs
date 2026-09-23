// Generation pipeline
use crate::error::PipelineError;
use engine::Engine;
use serde::Serialize;
use serde_json::Value;

// Loop 18: was 512. Loop 16 measured this hard-capping Qwen3's `<think>` reasoning mid-block
// on ~40-50% of the 10 project scenarios for Qwen3-4B, which is what actually broke JSON
// validity (not quantization). The primary fix is suppressing `<think>` altogether (see the
// `<think>\n\n</think>\n\n` seed below in `run_generate`); this cap is now a safety net for
// whatever short reasoning still slips through post-suppression, not the primary defense, so
// it is raised modestly rather than to Loop 16's suggested 2048 (production Ice Lake measured
// ~11-12 tok/s generation for Qwen3-4B, far below the K8s AMX pod's 30+ tok/s -- pushing the
// cap that high would cost 60-90s+ of extra worst-case latency per request for little added
// benefit once thinking is suppressed).
//
// Loop 19: this cap is now ALSO the real ceiling for `suppress_think: false` models
// (currently only Qwen3-0.6B) generating natural, unsuppressed CoT -- if the model's
// reasoning runs past 768 tokens without ever emitting valid JSON, generation stops here with
// likely-invalid output. Real measured rate for Qwen3-0.6B under this policy: 7/10 valid JSON
// (matches this model's pre-Loop-18 baseline exactly -- see
// docs/benchmarks/generation-pipeline-tuning.md), 3 of the 10 scenarios genuinely run the
// full 768 tokens without landing on `{"answer": ...}`.
pub const MAX_TOKENS: usize = 768;

#[derive(Debug, Clone, Serialize)]
pub struct GenerateResult {
    /// Raw model output after `<think>...</think>` stripping, before JSON parsing.
    pub raw_text: String,
    /// Parsed+validated JSON, if `raw_text` contained a valid, schema-conforming object.
    pub parsed: Option<Value>,
    /// True iff `parsed` is `Some` (i.e. valid JSON matching the option schema).
    pub valid: bool,
    pub tokens_generated: usize,
}

/// Strips `<think>...</think>` reasoning blocks (Qwen3 "thinking" output) from `text`. Never
/// panics on adversarial/malformed input:
/// - No `<think>` at all: returns `text` unchanged.
/// - `<think>` with a matching `</think>`: removes that whole span (repeats for multiple
///   blocks).
/// - `<think>` with no matching `</think>` (truncated generation): drops everything from
///   `<think>` onward, since it's unterminated reasoning, not a final answer.
pub fn strip_think(text: &str) -> String {
    let _s = timing::perf_span!("pipeline::strip_think");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        match rest.find("<think>") {
            None => {
                out.push_str(rest);
                break;
            }
            Some(start) => {
                out.push_str(&rest[..start]);
                let after_open = &rest[start + "<think>".len()..];
                match after_open.find("</think>") {
                    Some(end) => {
                        rest = &after_open[end + "</think>".len()..];
                    }
                    None => {
                        // Unterminated: drop the rest, nothing usable follows.
                        break;
                    }
                }
            }
        }
    }
    out.trim().to_string()
}

/// Extracts the first balanced `{...}` JSON object substring from `text` (models often wrap
/// JSON in prose or code fences) and validates it against the option schema: a JSON object
/// with an `"answer"` string field whose (trimmed) value matches one of `options`. Never
/// panics on adversarial input — any parse/shape mismatch yields `None`.
pub fn parse_and_validate(text: &str, options: &[String]) -> Option<Value> {
    let _s = timing::perf_span!("pipeline::parse_and_validate");
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end < start {
        return None;
    }
    let candidate = &text[start..=end];
    let value: Value = serde_json::from_str(candidate).ok()?;
    let obj = value.as_object()?;
    let answer = obj.get("answer")?.as_str()?;
    let answer_trimmed = answer.trim();
    if options.iter().any(|o| o.trim() == answer_trimmed) {
        Some(value)
    } else {
        None
    }
}

/// Greedy JSON generation (Decisions Locked #6): builds a chat-templated prompt asking the
/// model to answer with `{"answer": "<option>"}`, greedy-samples up to `MAX_TOKENS` or an
/// end-of-generation token, strips `<think>...</think>`, then parses+validates the result.
///
/// `suppress_think` selects the Loop 19 per-model generation policy (from
/// `models::ModelEntry::suppress_think`, plumbed through by the caller):
/// - `true` (Qwen3-4B, MiniCPM5-2B): Loop 18's behavior, unchanged -- seed a closed, empty
///   `<think>\n\n</think>\n\n` right after the chat-templated prompt so the model never emits
///   visible reasoning at all. Proven net win (10/10 valid JSON, improved correctness) for
///   both models; kept as-is.
/// - `false` (Qwen3-0.6B): Loop 19's fix -- let the model reason naturally (no seed), since
///   Loop 18 found this model's correctness depends on visible CoT (forcing it closed dropped
///   correct-answer count 5/9 -> 3/9). Recovers correctness to 5/9 (matching the pre-Loop-18
///   baseline) at the honest cost of JSON validity also reverting to that same pre-Loop-18
///   baseline (7/10, down from Loop 18's 10/10 for this model) -- see
///   `docs/benchmarks/generation-pipeline-tuning.md` for why grammar-constrained decoding
///   (which would have recovered the JSON-validity win without this trade-off) was prototyped
///   and rejected: it crashes this project's vendored llama.cpp build.
pub fn run_generate(
    engine: &mut Engine,
    prompt: &str,
    options: &[String],
    suppress_think: bool,
) -> Result<GenerateResult, PipelineError> {
    let mut _s = timing::perf_span!("pipeline::run_generate");
    _s.set("n_options", options.len().to_string());
    _s.set("suppress_think", suppress_think.to_string());
    let options_list = options.join(", ");
    let full_prompt = format!(
        "{prompt}\nOptions: {options_list}\nRespond with ONLY a JSON object of the exact form \
         {{\"answer\": \"<one of the options above>\"}}."
    );
    let chat_prompt = engine
        .apply_chat_template(&full_prompt)
        .map_err(|e| PipelineError::Engine(e.to_string()))?;
    // Loop 18 MAX_TOKENS-truncation fix, now conditional on the per-model policy (Loop 19).
    // `engine::apply_chat_template` renders via llama.cpp's built-in `llama_chat_apply_template`
    // (a fixed-format C++ matcher over the messages, confirmed by reading
    // `llama-cpp-2::model::apply_chat_template`'s source), NOT a real Jinja engine -- so the
    // GGUF's embedded chat template's `{% if enable_thinking is false %}{{- '<think>\n\n</think>
    // \n\n' }}{% endif %}` branch (confirmed present, byte for byte identical, in both
    // Qwen3-4B's and MiniCPM5-2B's `tokenizer.chat_template` GGUF metadata) never executes
    // here; there is no `enable_thinking` kwarg to pass through this binding. Appending that
    // exact literal text ourselves right after the assistant turn reproduces what the real
    // template would render for `enable_thinking=False` -- this is the documented community
    // workaround for chat-template engines that can't run arbitrary Jinja. It is inert prompt
    // -conditioning text (never generated, so it costs zero tokens) that tells the model its
    // reasoning block is already closed/empty, which in practice suppresses
    // `<think>...</think>` generation entirely instead of just capping it. Only applied when
    // `suppress_think` is true (Qwen3-4B, MiniCPM5-2B); Qwen3-0.6B (`suppress_think: false`)
    // gets the unmodified chat-templated prompt and reasons naturally.
    let chat_prompt = if suppress_think {
        format!("{chat_prompt}<think>\n\n</think>\n\n")
    } else {
        chat_prompt
    };

    let tokens = engine
        .tokenize(&chat_prompt)
        .map_err(|e| PipelineError::Engine(e.to_string()))?;
    let mut logits = engine
        .decode_prompt(&tokens)
        .map_err(|e| PipelineError::Engine(e.to_string()))?;

    let mut generated_text = String::new();
    let mut pos = tokens.len() as i32;
    let mut tokens_generated = 0usize;

    for _ in 0..MAX_TOKENS {
        let next_id = Engine::sample_greedy_from_logits(&logits) as i32;
        if engine.is_eog_id(next_id) {
            break;
        }
        let piece = engine
            .token_to_piece_id(next_id)
            .map_err(|e| PipelineError::Engine(e.to_string()))?;
        generated_text.push_str(&piece);
        tokens_generated += 1;

        logits = engine
            .decode_next_id(next_id, pos)
            .map_err(|e| PipelineError::Engine(e.to_string()))?;
        pos += 1;
    }

    let raw_text = strip_think(&generated_text);
    let parsed = parse_and_validate(&raw_text, options);
    let valid = parsed.is_some();

    Ok(GenerateResult {
        raw_text,
        parsed,
        valid,
        tokens_generated,
    })
}
