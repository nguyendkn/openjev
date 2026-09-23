//! The typed decision head from `laya/common.py`'s `DecisionModel.forward`, transcribed against
//! the GGUF's own `ggmlc.graph_spec` node stream (nodes 1565-1643):
//!
//! ```text
//! h = encoder_out + type_emb[qtype]                       (add_114)
//! for i in 0..2:                                          (nn.TransformerEncoderLayer, norm_first)
//!     h = h + Wo @ sdpa(LN(h) @ Wqkv)                      (layer_norm_57/58, linear_112/113)
//!     h = h + W2 @ relu(W1 @ LN(h))                        (relu / relu_1 -- NOT gelu)
//! m = gather(h, marker_pos)                               (embedding_2)
//! logits = W4 @ gelu(W3 @ LN(m) + beta_fold + b3) + b4    (linear_120, gelu_28, linear_121)
//! ```
//!
//! Two details that are easy to get wrong and are both verified against the reference's own
//! intermediate tensors:
//!
//! * The feed-forward activation is **ReLU** — `nn.TransformerEncoderLayer`'s default — even
//!   though the encoder's own MLP uses GeGLU.
//! * The scorer adds **two** biases before the GELU. `linear_120_baked_bias` is the scorer
//!   LayerNorm's beta folded through `scorer_fc1.weight`; `scorer_fc1.bias` is the Linear's own
//!   bias, which the reference folds into a fused bias+GELU custom op
//!   (`GGML_OP_CUSTOM_BIAS_GELU`, i.e. `gelu(x + b)`), so it is *not* a double-add. Checked
//!   numerically: `gelu_tanh(linear_120 + scorer_fc1.bias)` reproduces the reference's `gelu_28`
//!   to 3.5e-8 L2-relative, while omitting the bias is off by 1.1e-2.
use crate::attn::{sdpa, split_head, split_head_view};
use crate::gguf::Weights;
use crate::graph::{D, EPS, HEAD_LAYERS};
use llama_cpp_sys_2 as s;

/// Appends the head + scorer to the graph and returns the `[1, kmax]` raw logits.
#[allow(clippy::too_many_arguments)]
pub unsafe fn build(
    ctx: *mut s::ggml_context,
    w: &Weights,
    hidden: *mut s::ggml_tensor,
    qtype: *mut s::ggml_tensor,
    mpos: *mut s::ggml_tensor,
    mask: *mut s::ggml_tensor,
    seq: i64,
    dbg: &mut Vec<(String, *mut s::ggml_tensor)>,
) -> Result<*mut s::ggml_tensor, String> {
    let lin = |name: &str, inp: *mut s::ggml_tensor| -> Result<*mut s::ggml_tensor, String> {
        let y = s::ggml_mul_mat(ctx, w.get(&format!("{name}.weight"))?, inp);
        Ok(s::ggml_add(ctx, y, w.get(&format!("{name}.bias"))?))
    };

    let te = s::ggml_get_rows(ctx, w.expect("type_emb.weight", &[D, 3], s::GGML_TYPE_Q8_0)?, qtype);
    let mut x = s::ggml_add(ctx, hidden, te);
    dbg.push(("type_emb_add".into(), x));

    for i in 0..HEAD_LAYERS {
        let h = s::ggml_norm(ctx, x, EPS);
        let qkv = lin(&format!("head_qkv.{i}"), h)?;
        let a = sdpa(
            ctx,
            split_head(ctx, qkv, seq, 0),
            split_head(ctx, qkv, seq, 1),
            split_head_view(ctx, qkv, seq, 2),
            mask,
            seq,
        );
        x = s::ggml_add(ctx, x, lin(&format!("head_wo.{i}"), a)?);
        dbg.push((format!("H{i}.attn_res"), x));
        let h = s::ggml_norm(ctx, x, EPS);
        let f = s::ggml_relu(ctx, lin(&format!("head_fc1.{i}"), h)?);
        x = s::ggml_add(ctx, x, lin(&format!("head_fc2.{i}"), f)?);
        dbg.push((format!("H{i}.ff_res"), x));
    }

    // gather the [MASK] marker rows, then scorer: LN -> Linear -> GELU -> Linear
    let m = s::ggml_get_rows(ctx, x, mpos); // [D, kmax]
    dbg.push(("marker_gather".into(), m));
    let m = s::ggml_norm(ctx, m, EPS);
    let m = s::ggml_mul_mat(ctx, w.expect("scorer_fc1.weight", &[D, D], s::GGML_TYPE_Q8_0)?, m);
    let m = s::ggml_add(ctx, m, w.expect("linear_120_baked_bias", &[D], s::GGML_TYPE_F32)?);
    let m = s::ggml_add(ctx, m, w.expect("scorer_fc1.bias", &[D], s::GGML_TYPE_F32)?);
    dbg.push(("scorer_fc1".into(), m));
    let m = s::ggml_gelu(ctx, m);
    dbg.push(("scorer_gelu".into(), m));
    let m = s::ggml_mul_mat(ctx, w.expect("scorer_fc2.weight", &[D, 1], s::GGML_TYPE_F32)?, m);
    let logits = s::ggml_add(ctx, m, w.expect("scorer_fc2.bias", &[1], s::GGML_TYPE_F32)?);
    dbg.push(("logits".into(), logits));
    Ok(logits)
}
