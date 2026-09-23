//! The hand-rolled ggml graph: 28-layer ModernBERT-large encoder (+ the 2-layer decision head
//! and scorer, kept here ONLY so this loop has an end-to-end correctness signal — Loop 11 owns
//! the real head). Built once for a given padded sequence length and reused across requests;
//! per request only the input tensor DATA is rewritten.
//!
//! Op order transcribed node-by-node from the GGUF's own `ggmlc.graph_spec` (1364 nodes), not
//! from the HF reference: layer 0 has NO attention pre-norm, every other LayerNorm is
//! affine-free (its gamma is folded into the following Linear), attention alternates
//! global (layers 0,3,...,27, RoPE theta=160000) / sliding-window (RoPE theta=10000).
use crate::gguf::Weights;
use llama_cpp_sys_2 as s;

pub const D: i64 = 1024;
pub const NH: i64 = 16;
pub const HD: i64 = 64;
pub const WI: i64 = 5248;
pub const FF: i64 = 2624;
pub const LAYERS: usize = 28;
pub const HEAD_LAYERS: usize = 2;
pub const EPS: f32 = 1e-5;
pub const ROPE_NEOX: i32 = 2;
pub const THETA_GLOBAL: f32 = 160000.0;
pub const THETA_LOCAL: f32 = 10000.0;

pub struct Graph {
    pub ctx: *mut s::ggml_context,
    pub gf: *mut s::ggml_cgraph,
    pub seq: i64,
    pub kmax: i64,
    pub inp_ids: *mut s::ggml_tensor,
    pub pos: *mut s::ggml_tensor,
    pub mask_g: *mut s::ggml_tensor,
    pub mask_l: *mut s::ggml_tensor,
    pub qtype: *mut s::ggml_tensor,
    pub mpos: *mut s::ggml_tensor,
    pub hidden: *mut s::ggml_tensor,
    pub logits: *mut s::ggml_tensor,
}

unsafe fn split_head(ctx: *mut s::ggml_context, qkv: *mut s::ggml_tensor, seq: i64, part: i64) -> *mut s::ggml_tensor {
    // qkv is [3D, seq] F32 contiguous -> a [HD, NH, seq] view of one of the three slices.
    s::ggml_cont(
        ctx,
        s::ggml_view_3d(ctx, qkv, HD, NH, seq, (HD * 4) as usize, (3 * D * 4) as usize, (part * D * 4) as usize),
    )
}

/// q/k/v -> softmax(QK^T * 1/8 + mask) V, returned as [D, seq].
///
/// Deliberately `ggml_flash_attn_ext` with K/V cast to F16, because that is exactly what the
/// `ggmlc` runtime does for head_dim=64 (`runtime/src/executor.cpp`, `is_fattn_supported`), and
/// this loop is chasing bit-level agreement with that reference, not maximum accuracy.
unsafe fn sdpa(
    ctx: *mut s::ggml_context,
    q: *mut s::ggml_tensor,
    k: *mut s::ggml_tensor,
    v: *mut s::ggml_tensor,
    mask: *mut s::ggml_tensor,
    seq: i64,
) -> *mut s::ggml_tensor {
    let q = s::ggml_cont(ctx, s::ggml_permute(ctx, q, 0, 2, 1, 3)); // [HD, seq, NH]
    let k = s::ggml_cast(ctx, s::ggml_cont(ctx, s::ggml_permute(ctx, k, 0, 2, 1, 3)), s::GGML_TYPE_F16);
    let v = s::ggml_cast(ctx, s::ggml_cont(ctx, s::ggml_permute(ctx, v, 0, 2, 1, 3)), s::GGML_TYPE_F16);
    let o = s::ggml_flash_attn_ext(ctx, q, k, v, mask, 1.0 / (HD as f32).sqrt(), 0.0, 0.0);
    s::ggml_reshape_2d(ctx, o, D, seq) // [HD, NH, seq] -> [D, seq]
}

impl Graph {
    pub fn build(w: &Weights, seq: i64, kmax: i64) -> Result<Graph, String> {
        // Rough upper bound on the arena: every intermediate stays materialised so the graph can
        // be recomputed in place. Deliberately simple (correctness-first); a gallocr pass would
        // cut this a lot.
        let per_layer = 124_416i64 * seq + 128 * seq * seq;
        let mem = (per_layer * 32 * 2) as usize + 512 * 1024 * 1024;
        unsafe {
            let ctx = s::ggml_init(s::ggml_init_params {
                mem_size: mem,
                mem_buffer: std::ptr::null_mut(),
                no_alloc: false,
            });
            if ctx.is_null() {
                return Err("ggml_init failed".into());
            }
            let gf = s::ggml_new_graph_custom(ctx, 8192, false);

            let inp_ids = s::ggml_new_tensor_1d(ctx, s::GGML_TYPE_I32, seq);
            let pos = s::ggml_new_tensor_1d(ctx, s::GGML_TYPE_I32, seq);
            let mask_g = s::ggml_new_tensor_2d(ctx, s::GGML_TYPE_F32, seq, seq);
            let mask_l = s::ggml_new_tensor_2d(ctx, s::GGML_TYPE_F32, seq, seq);
            let qtype = s::ggml_new_tensor_1d(ctx, s::GGML_TYPE_I32, 1);
            let mpos = s::ggml_new_tensor_1d(ctx, s::GGML_TYPE_I32, kmax);
            // ggml_flash_attn_ext wants an F16 mask; ggmlc casts the same F32 masks the graph
            // builds, so cast once here and share across all layers.
            let (mask_g16, mask_l16) = (s::ggml_cast(ctx, mask_g, s::GGML_TYPE_F16), s::ggml_cast(ctx, mask_l, s::GGML_TYPE_F16));

            // --- embeddings + embedding LayerNorm (the only encoder norm that keeps its gamma)
            let tok = w.expect("tok_emb.weight", &[D, 50368], s::GGML_TYPE_Q8_0)?;
            let mut x = s::ggml_get_rows(ctx, tok, inp_ids); // [D, seq]
            x = s::ggml_mul(ctx, s::ggml_norm(ctx, x, EPS), w.expect("emb_norm_w", &[D], s::GGML_TYPE_F32)?);

            for l in 0..LAYERS {
                let global = l % 3 == 0;
                let (mask, theta) = if global { (mask_g16, THETA_GLOBAL) } else { (mask_l16, THETA_LOCAL) };
                // layer 0's attn_norm is Identity in ModernBERT (graph_spec node 18 reads the
                // embedding norm output directly).
                let h = if l == 0 { x } else { s::ggml_norm(ctx, x, EPS) };
                let qkv = s::ggml_mul_mat(ctx, w.expect(&format!("qkv.{l}.weight"), &[D, 3 * D], s::GGML_TYPE_Q8_0)?, h);
                let rope = |t: *mut s::ggml_tensor| {
                    s::ggml_rope_ext(ctx, t, pos, std::ptr::null_mut(), HD as i32, ROPE_NEOX, 512, theta, 1.0, 0.0, 1.0, 32.0, 1.0)
                };
                let q = rope(split_head(ctx, qkv, seq, 0));
                let k = rope(split_head(ctx, qkv, seq, 1));
                let v = split_head(ctx, qkv, seq, 2);
                let a = sdpa(ctx, q, k, v, mask, seq);
                let a = s::ggml_mul_mat(ctx, w.expect(&format!("wo.{l}.weight"), &[D, D], s::GGML_TYPE_Q8_0)?, a);
                x = s::ggml_add(ctx, x, a);

                let h = s::ggml_norm(ctx, x, EPS);
                let wi = s::ggml_mul_mat(ctx, w.expect(&format!("mlp_wi.{l}.weight"), &[D, WI], s::GGML_TYPE_Q8_0)?, h);
                let gate = s::ggml_cont(ctx, s::ggml_view_2d(ctx, wi, FF, seq, (WI * 4) as usize, 0));
                let up = s::ggml_cont(ctx, s::ggml_view_2d(ctx, wi, FF, seq, (WI * 4) as usize, (FF * 4) as usize));
                let g = s::ggml_mul(ctx, s::ggml_gelu(ctx, gate), up);
                let o = s::ggml_mul_mat(ctx, w.expect(&format!("mlp_wo.{l}.weight"), &[FF, D], s::GGML_TYPE_Q8_0)?, g);
                x = s::ggml_add(ctx, x, o);
            }
            x = s::ggml_mul(ctx, s::ggml_norm(ctx, x, EPS), w.expect("final_norm_w", &[D], s::GGML_TYPE_F32)?);
            let hidden = x;

            // --- TEMPORARY head + scorer (Loop 11 owns the real one) ---------------------
            let te = s::ggml_get_rows(ctx, w.expect("type_emb.weight", &[D, 3], s::GGML_TYPE_Q8_0)?, qtype);
            x = s::ggml_add(ctx, x, te);
            for i in 0..HEAD_LAYERS {
                let lin = |name: &str, inp: *mut s::ggml_tensor| -> Result<*mut s::ggml_tensor, String> {
                    let y = s::ggml_mul_mat(ctx, w.get(&format!("{name}.weight"))?, inp);
                    Ok(s::ggml_add(ctx, y, w.get(&format!("{name}.bias"))?))
                };
                let h = s::ggml_norm(ctx, x, EPS);
                let qkv = lin(&format!("head_qkv.{i}"), h)?;
                let a = sdpa(
                    ctx,
                    split_head(ctx, qkv, seq, 0),
                    split_head(ctx, qkv, seq, 1),
                    split_head(ctx, qkv, seq, 2),
                    mask_g16,
                    seq,
                );
                x = s::ggml_add(ctx, x, lin(&format!("head_wo.{i}"), a)?);
                let h = s::ggml_norm(ctx, x, EPS);
                let f = s::ggml_relu(ctx, lin(&format!("head_fc1.{i}"), h)?);
                x = s::ggml_add(ctx, x, lin(&format!("head_fc2.{i}"), f)?);
            }
            // gather the [MASK] marker positions, then scorer: LN -> Linear -> GELU -> Linear
            let m = s::ggml_get_rows(ctx, x, mpos); // [D, kmax]
            let m = s::ggml_norm(ctx, m, EPS);
            let m = s::ggml_mul_mat(ctx, w.expect("scorer_fc1.weight", &[D, D], s::GGML_TYPE_Q8_0)?, m);
            // two biases: the folded LayerNorm beta (`linear_120_baked_bias`) + the Linear's own.
            let m = s::ggml_add(ctx, m, w.expect("linear_120_baked_bias", &[D], s::GGML_TYPE_F32)?);
            let m = s::ggml_gelu(ctx, s::ggml_add(ctx, m, w.expect("scorer_fc1.bias", &[D], s::GGML_TYPE_F32)?));
            let m = s::ggml_mul_mat(ctx, w.expect("scorer_fc2.weight", &[D, 1], s::GGML_TYPE_F32)?, m);
            let logits = s::ggml_add(ctx, m, w.expect("scorer_fc2.bias", &[1], s::GGML_TYPE_F32)?); // [1, kmax]

            s::ggml_build_forward_expand(gf, hidden);
            s::ggml_build_forward_expand(gf, logits);
            Ok(Graph { ctx, gf, seq, kmax, inp_ids, pos, mask_g, mask_l, qtype, mpos, hidden, logits })
        }
    }

    pub fn n_nodes(&self) -> i32 {
        unsafe { s::ggml_graph_n_nodes(self.gf) }
    }

    pub fn compute(&self, n_threads: i32) -> Result<(), String> {
        unsafe {
            let st = s::ggml_graph_compute_with_ctx(self.ctx, self.gf, n_threads);
            if st != s::GGML_STATUS_SUCCESS {
                return Err(format!("ggml_graph_compute failed: {st:?}"));
            }
        }
        Ok(())
    }
}
