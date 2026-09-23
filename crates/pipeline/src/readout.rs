// Constrained readout pipeline
use crate::error::PipelineError;
use engine::Engine;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct ReadoutResult {
    pub probs: HashMap<String, f32>,
    pub best_option: String,
}

/// Softmax restricted to `logits[i]` for each `i` in `restricted_indices`, in the same order
/// as `restricted_indices`. Never panics: out-of-range indices are treated as `-inf` (probability
/// 0), and an all-`-inf`/non-finite result falls back to a uniform distribution rather than
/// producing NaN.
///
/// This implements Decisions Locked #5 ("logits restricted to candidate-label token ids BEFORE
/// softmax, not full-vocab softmax then filter"): the softmax normalization only ever sees the
/// restricted set, so probabilities sum to 1 over the candidates alone.
pub fn constrained_softmax(logits: &[f32], restricted_indices: &[usize]) -> Vec<f32> {
    let _s = timing::perf_span!("pipeline::constrained_softmax");
    if restricted_indices.is_empty() {
        return Vec::new();
    }
    let restricted: Vec<f32> = restricted_indices
        .iter()
        .map(|&i| logits.get(i).copied().unwrap_or(f32::NEG_INFINITY))
        .collect();
    let max = restricted.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let exps: Vec<f32> = restricted
        .iter()
        .map(|&v| if max.is_finite() { (v - max).exp() } else { 0.0 })
        .collect();
    let sum: f32 = exps.iter().sum();
    if !sum.is_finite() || sum <= 0.0 {
        let n = restricted_indices.len() as f32;
        return vec![1.0 / n; restricted_indices.len()];
    }
    exps.iter().map(|&e| e / sum).collect()
}

/// Runs a constrained single-token readout (Decisions Locked: "1 forward pass, softmax
/// restricted to option-label token ids"): encodes `prompt`, decodes one forward pass to get
/// next-token logits, resolves each of `options`'s label to a token id, and restricts+softmaxes
/// over just those ids.
pub fn run_readout(
    engine: &mut Engine,
    prompt: &str,
    options: &[String],
) -> Result<ReadoutResult, PipelineError> {
    let mut _s = timing::perf_span!("pipeline::run_readout");
    _s.set("n_options", options.len().to_string());
    if options.is_empty() {
        return Err(PipelineError::InvalidOptions(
            "no options given".to_string(),
        ));
    }

    // Chat-template the prompt (Qwen3-0.6B is an Instruct model) and leave a hanging
    // "Answer: " continuation so the single next-token distribution is actually a label
    // choice, not raw free-text completion.
    let options_list = options.join(", ");
    let instruction = format!("{prompt}\nOptions: {options_list}\nAnswer with only the option letter.");
    let chat_prompt = engine
        .apply_chat_template(&instruction)
        .map_err(|e| PipelineError::Engine(e.to_string()))?;
    let chat_prompt = format!("{chat_prompt}Answer: ");

    let tokens = engine
        .tokenize(&chat_prompt)
        .map_err(|e| PipelineError::Engine(e.to_string()))?;
    let logits = engine
        .decode_prompt(&tokens)
        .map_err(|e| PipelineError::Engine(e.to_string()))?;

    let mut option_token_ids = Vec::with_capacity(options.len());
    for opt in options {
        let opt_tokens = engine
            .tokenize_no_bos(opt)
            .map_err(|e| PipelineError::Engine(e.to_string()))?;
        let label_token = opt_tokens.first().ok_or_else(|| {
            PipelineError::InvalidOptions(format!("option '{opt}' tokenized to nothing"))
        })?;
        option_token_ids.push(label_token.0 as usize);
    }

    let probs = constrained_softmax(&logits, &option_token_ids);

    let mut prob_map = HashMap::with_capacity(options.len());
    let mut best_option = options[0].clone();
    let mut best_prob = f32::NEG_INFINITY;
    for (opt, p) in options.iter().zip(probs.iter()) {
        prob_map.insert(opt.clone(), *p);
        if *p > best_prob {
            best_prob = *p;
            best_option = opt.clone();
        }
    }

    Ok(ReadoutResult {
        probs: prob_map,
        best_option,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uniform_when_all_neg_inf() {
        let logits = vec![0.0f32; 4];
        let probs = constrained_softmax(&logits, &[10, 20]); // out of range -> -inf both
        assert_eq!(probs.len(), 2);
        assert!((probs[0] - 0.5).abs() < 1e-6);
        assert!((probs[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn picks_higher_logit() {
        let logits = vec![1.0, 5.0, 2.0];
        let probs = constrained_softmax(&logits, &[0, 1, 2]);
        assert_eq!(probs.len(), 3);
        let sum: f32 = probs.iter().sum();
        assert!((sum - 1.0).abs() < 1e-5);
        assert!(probs[1] > probs[0] && probs[1] > probs[2]);
    }

    #[test]
    fn empty_indices_returns_empty() {
        assert!(constrained_softmax(&[1.0, 2.0], &[]).is_empty());
    }
}
