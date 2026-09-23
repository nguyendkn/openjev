//! GGUF loading for the Laya `ggmlc` export: pulls all 153 tensors + the KV metadata
//! (special token ids, budgets, temperatures, mask constants) into one Rust struct.
use llama_cpp_sys_2 as s;
use std::collections::HashMap;
use std::ffi::{CStr, CString};

pub struct Weights {
    pub gguf: *mut s::gguf_context,
    pub ctx: *mut s::ggml_context,
    pub tensors: HashMap<String, *mut s::ggml_tensor>,
    pub tokens: Vec<String>,
    pub merges: Vec<String>,
    pub cls_id: i32,
    pub sep_id: i32,
    pub mask_id: i32,
    pub pad_id: i32,
    pub unk_id: i32,
    pub max_len: usize,
    pub head_max_len: usize,
    pub max_opts: usize,
    pub temperature_by_options: String,
    /// `laya.temperature`, indexed by `QType` (choice, score, noul). Used when a question's
    /// `(qtype, size)` bucket is missing from `temperature_by_options`.
    pub temperature_by_qtype: [f32; 3],
}

/// `laya/common.py`'s `TEMP_MIN` / `TEMP_MAX` — the bounds upstream's `clamp_temperature()`
/// enforces on every fitted temperature. See [`Weights::temperature`].
pub const TEMP_MIN: f32 = 0.5;
pub const TEMP_MAX: f32 = 5.0;

/// Parses `laya.temperature`, which `laya`'s `fill_temperature` accepts as either a 3-element
/// JSON array or a single number broadcast to all three question types.
fn parse_temperatures(json: &str) -> [f32; 3] {
    const DEFAULT: [f32; 3] = [1.6369, 1.25143, 1.9834]; // laya's own hardcoded fallback
    let nums: Vec<f32> = json
        .trim_matches(|c: char| c == '[' || c == ']' || c.is_whitespace())
        .split(',')
        .filter_map(|v| v.trim().parse::<f32>().ok())
        .collect();
    match nums.len() {
        0 => DEFAULT,
        1 => [nums[0]; 3],
        _ => [nums[0], nums[1], *nums.get(2).unwrap_or(&nums[1])],
    }
}

/// G22: `gguf_init_from_file` hands back two separately-owned allocations — the gguf context and
/// (because `no_alloc == false`) a ggml context holding every tensor's data. `gguf_free` only
/// releases the first, so both are freed here.
impl Drop for Weights {
    fn drop(&mut self) {
        unsafe {
            s::gguf_free(self.gguf);
            s::ggml_free(self.ctx);
        }
    }
}

unsafe fn key_id(g: *mut s::gguf_context, k: &str) -> Option<i64> {
    let c = CString::new(k).unwrap();
    let id = s::gguf_find_key(g, c.as_ptr());
    (id >= 0).then_some(id)
}

unsafe fn kv_i32(g: *mut s::gguf_context, k: &str) -> Option<i32> {
    key_id(g, k).map(|i| s::gguf_get_val_i32(g, i))
}

unsafe fn kv_str(g: *mut s::gguf_context, k: &str) -> Option<String> {
    key_id(g, k).map(|i| CStr::from_ptr(s::gguf_get_val_str(g, i)).to_string_lossy().into_owned())
}

unsafe fn kv_arr_str(g: *mut s::gguf_context, k: &str) -> Vec<String> {
    let Some(i) = key_id(g, k) else { return Vec::new() };
    let n = s::gguf_get_arr_n(g, i);
    (0..n)
        .map(|j| CStr::from_ptr(s::gguf_get_arr_str(g, i, j)).to_string_lossy().into_owned())
        .collect()
}

impl Weights {
    /// Loads the GGUF with `no_alloc=false`, so tensor data lands in the returned ggml context.
    pub fn load(path: &str) -> Result<Self, String> {
        unsafe {
            let mut ctx: *mut s::ggml_context = std::ptr::null_mut();
            let params = s::gguf_init_params { no_alloc: false, ctx: &mut ctx };
            let cpath = CString::new(path).map_err(|e| e.to_string())?;
            let g = s::gguf_init_from_file(cpath.as_ptr(), params);
            if g.is_null() {
                return Err(format!("gguf_init_from_file failed for {path}"));
            }
            let n = s::gguf_get_n_tensors(g);
            let mut tensors = HashMap::with_capacity(n as usize);
            for i in 0..n {
                let name = CStr::from_ptr(s::gguf_get_tensor_name(g, i)).to_string_lossy().into_owned();
                let cn = CString::new(name.clone()).unwrap();
                let t = s::ggml_get_tensor(ctx, cn.as_ptr());
                if t.is_null() {
                    return Err(format!("tensor {name} listed in gguf but absent from ggml ctx"));
                }
                tensors.insert(name, t);
            }
            Ok(Weights {
                gguf: g,
                ctx,
                tensors,
                tokens: kv_arr_str(g, "tokenizer.ggml.tokens"),
                merges: kv_arr_str(g, "tokenizer.ggml.merges"),
                cls_id: kv_i32(g, "laya.cls_token_id").ok_or("missing laya.cls_token_id")?,
                sep_id: kv_i32(g, "laya.sep_token_id").ok_or("missing laya.sep_token_id")?,
                mask_id: kv_i32(g, "laya.mask_token_id").ok_or("missing laya.mask_token_id")?,
                pad_id: kv_i32(g, "laya.pad_token_id").ok_or("missing laya.pad_token_id")?,
                unk_id: kv_i32(g, "tokenizer.ggml.unknown_token_id").unwrap_or(50280),
                max_len: kv_i32(g, "laya.max_len").unwrap_or(512) as usize,
                head_max_len: kv_i32(g, "laya.head_max_len").unwrap_or(192) as usize,
                max_opts: kv_i32(g, "laya.max_opts").unwrap_or(16) as usize,
                temperature_by_options: kv_str(g, "laya.temperature_by_options").unwrap_or_default(),
                temperature_by_qtype: parse_temperatures(&kv_str(g, "laya.temperature").unwrap_or_default()),
            })
        }
    }

    pub fn get(&self, name: &str) -> Result<*mut s::ggml_tensor, String> {
        self.tensors.get(name).copied().ok_or_else(|| format!("missing tensor: {name}"))
    }

    /// Fails loud on any shape/type drift vs. the Loop 9 inventory.
    pub fn expect(&self, name: &str, ne: &[i64], ty: s::ggml_type) -> Result<*mut s::ggml_tensor, String> {
        let t = self.get(name)?;
        unsafe {
            for (i, want) in ne.iter().enumerate() {
                let got = (*t).ne[i];
                if got != *want {
                    return Err(format!("{name}: ne[{i}]={got}, expected {want}"));
                }
            }
            if (*t).type_ != ty {
                return Err(format!("{name}: type={:?}, expected {:?}", (*t).type_, ty));
            }
        }
        Ok(t)
    }

    /// Reads an F32 tensor's contents out to a Vec (used for the baked mask constants).
    pub fn f32_data(&self, name: &str) -> Result<Vec<f32>, String> {
        let t = self.get(name)?;
        unsafe {
            if (*t).type_ != s::GGML_TYPE_F32 {
                return Err(format!("{name} is not F32"));
            }
            let n = s::ggml_nelements(t) as usize;
            Ok(std::slice::from_raw_parts(s::ggml_get_data_f32(t), n).to_vec())
        }
    }

    /// Temperature for a `(qtype, n_options)` bucket: look the bucket up in
    /// `laya.temperature_by_options`, fall back to the per-qtype `laya.temperature` entry (NOT
    /// 1.0) when the bucket is absent, then clamp to `[TEMP_MIN, TEMP_MAX]`.
    ///
    /// # Why the clamp is back (deliberate, and deliberately NOT what `ggmlc` does)
    ///
    /// Loop 11 removed this clamp to match `ggmlc`'s `max(t, 1e-3)`. Loop 15's PyTorch
    /// ground-truth run (`docs/benchmarks/pytorch-ground-truth-reference.md`) showed that is
    /// backwards with respect to the *original* model: upstream `laya/common.py` defines
    /// `TEMP_MIN = 0.5`, `TEMP_MAX = 5.0` and a `clamp_temperature()` that every bucket goes
    /// through, and emits a `RuntimeWarning` naming the bucket it rejects. Its source comment
    /// explains why, about the shipped `choice:11+` value of 0.1006:
    ///
    /// > multiplies [the logits] ~10x: a 0.24 top probability is published as 0.99, so a caller
    /// > gating on confidence is told a coin flip is a certainty. No honest calibration needs to
    /// > sharpen this hard, so refuse to apply one that does.
    ///
    /// `ggmlc` and upstream Python genuinely disagree here; this crate follows **upstream's
    /// design intent**, chosen by the user in Loop 13, accepting that it costs bit-for-bit
    /// agreement with `laya serve` on `choice:11+` questions. Blast radius is exactly that
    /// bucket: for `k <= 10` the two policies are identical, so all 10 project benchmark
    /// scenarios are untouched, and no `choice` flips even at k = 11/17/20 — only the published
    /// confidence changes (ground truth `E1_11opt_bucket_11plus`: 0.9642 clamped vs a dishonest
    /// 1.0000 raw).
    pub fn temperature(&self, qtype_name: &str, k: usize) -> f32 {
        let size = if k <= 2 { "2" } else if k <= 5 { "3-5" } else if k <= 10 { "6-10" } else { "11+" };
        let needle = format!("\"{qtype_name}:{size}\":");
        let default = match qtype_name {
            "score" => self.temperature_by_qtype[1],
            "noul" => self.temperature_by_qtype[2],
            _ => self.temperature_by_qtype[0],
        };
        let t = self
            .temperature_by_options
            .find(&needle)
            .and_then(|p| {
                let rest = &self.temperature_by_options[p + needle.len()..];
                let end = rest.find(|c| c == ',' || c == '}')?;
                rest[..end].trim().parse::<f32>().ok()
            })
            .unwrap_or(default);
        t.clamp(TEMP_MIN, TEMP_MAX)
    }
}
