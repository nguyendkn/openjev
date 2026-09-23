pub mod logger;

pub use logger::PerfSpan;

use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug)]
pub struct Timings {
    pub model_load_ms: u128,
    pub warmup_ms: u128,
    pub tokenize_ms: u128,
    pub constrained_readout_ms: u128,
    pub generation_ms: u128,
    pub laya_model_load_ms: u128,
    pub laya_inference_ms: u128,
}

impl Default for Timings {
    fn default() -> Self {
        Self {
            model_load_ms: 0,
            warmup_ms: 0,
            tokenize_ms: 0,
            constrained_readout_ms: 0,
            generation_ms: 0,
            laya_model_load_ms: 0,
            laya_inference_ms: 0,
        }
    }
}
