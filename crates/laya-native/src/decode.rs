//! Answer decoding: the `softmax(logits / temperature)` -> argmax -> entropy-confidence tail
//! that `laya`'s `decode_answer` applies after the forward pass, plus a `Question` constructor.
use crate::sequence::{QType, Question};

pub fn softmax(logits: &[f32], temp: f32) -> Vec<f32> {
    let scaled: Vec<f32> = logits.iter().map(|l| l / temp).collect();
    let m = scaled.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    let e: Vec<f32> = scaled.iter().map(|l| (l - m).exp()).collect();
    let sum: f32 = e.iter().sum();
    e.iter().map(|v| v / sum).collect()
}

pub fn argmax(p: &[f32]) -> usize {
    p.iter().enumerate().fold((0usize, f32::NEG_INFINITY), |(bi, bv), (i, v)| if *v > bv { (i, *v) } else { (bi, bv) }).0
}

/// `confidence_from_probs`: 1 - H(p)/log(k).
pub fn entropy_confidence(p: &[f32]) -> f32 {
    let k = p.len();
    if k < 2 {
        return 1.0;
    }
    let ent: f32 = -p.iter().map(|v| v * v.clamp(1e-12, 1.0).ln()).sum::<f32>();
    (1.0 - ent / (k as f32).ln()).clamp(0.0, 1.0)
}

pub fn choice_question(instructions: &str, options: &[String]) -> Question {
    Question {
        qtype: QType::Choice,
        instructions: instructions.to_string(),
        criteria: options.iter().map(|o| (o.clone(), None)).collect(),
    }
}
