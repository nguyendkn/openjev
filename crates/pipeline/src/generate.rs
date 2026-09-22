// Generation pipeline
use crate::error::PipelineError;
use engine::Engine;
use serde::Serialize;
use serde_json::Value;

pub const MAX_TOKENS: usize = 512;

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
pub fn run_generate(
    engine: &mut Engine,
    prompt: &str,
    options: &[String],
) -> Result<GenerateResult, PipelineError> {
    let options_list = options.join(", ");
    let full_prompt = format!(
        "{prompt}\nOptions: {options_list}\nRespond with ONLY a JSON object of the exact form \
         {{\"answer\": \"<one of the options above>\"}}."
    );
    let chat_prompt = engine
        .apply_chat_template(&full_prompt)
        .map_err(|e| PipelineError::Engine(e.to_string()))?;

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
