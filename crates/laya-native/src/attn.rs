//! Attention building blocks shared by the encoder layers and the decision head: qkv splitting,
//! RoPE from the GGUF's baked cos/sin tables, and the flash-attention call.
use crate::gguf::Weights;
use crate::graph::{D, HD, NH};
use llama_cpp_sys_2 as s;

/// `qkv` is `[3D, seq]` F32 contiguous -> a contiguous `[HD, seq, NH]` copy of one of its three
/// slices, i.e. already in the head-major layout `ggml_flash_attn_ext` consumes.
///
/// D_13: this used to produce `[HD, NH, seq]` and leave `sdpa` to `cont(permute(.., 0,2,1,3))` it
/// into head-major, i.e. **two** full `[D, seq]` copies per q/k/v per layer. Strided `ggml_view_3d`
/// can express the head-major layout directly out of the `[3D, seq]` qkv block, so one of the two
/// copies (and its graph node, and its OpenMP barrier) is pure waste. Element `(h, t, nh)` sits at
/// float offset `part*D + t*3D + nh*HD + h`, hence `nb1 = 3D*4` (token stride) and `nb2 = HD*4`
/// (head stride). Values are bit-identical — only the copy schedule changes.
pub unsafe fn split_head(
    ctx: *mut s::ggml_context,
    qkv: *mut s::ggml_tensor,
    seq: i64,
    part: i64,
) -> *mut s::ggml_tensor {
    s::ggml_cont(ctx, split_head_view(ctx, qkv, seq, part))
}

/// The same slice, left as a strided view. V needs no RoPE and `sdpa` only casts it to F16, and
/// `ggml_cast`'s dup kernel reads arbitrary strides — so V can skip the materialising `ggml_cont`
/// entirely and fuse gather+convert into the one node.
pub unsafe fn split_head_view(
    ctx: *mut s::ggml_context,
    qkv: *mut s::ggml_tensor,
    seq: i64,
    part: i64,
) -> *mut s::ggml_tensor {
    s::ggml_view_3d(ctx, qkv, HD, seq, NH, (3 * D * 4) as usize, (HD * 4) as usize, (part * D * 4) as usize)
}

/// The GGUF's baked cos/sin table for `name`, sliced to `seq` positions -> `[HD, seq]`.
///
/// Left 2-D on purpose: against a head-major `[HD, seq, NH]` operand this already has the trailing
/// `ne[2] = 1` that `ggml_mul`'s repeat-broadcast needs, so the extra `ggml_reshape_3d` the
/// `[HD, NH, seq]` layout required (4 more graph nodes, 4 more barriers, zero arithmetic) is gone.
pub unsafe fn rope_table(
    ctx: *mut s::ggml_context,
    w: &Weights,
    name: &str,
    seq: i64,
) -> Result<*mut s::ggml_tensor, String> {
    let t = w.expect(name, &[HD, 513], s::GGML_TYPE_F32)?;
    Ok(s::ggml_cont(ctx, s::ggml_view_2d(ctx, t, HD, seq, (HD * 4) as usize, 0)))
}

/// RoPE done exactly the way the reference graph does it: with the GGUF's **baked** F32 cos/sin
/// tables, not trig recomputed at runtime.
///
/// This is not cosmetic. `ggml_rope_ext` recomputes `cos(p * theta^(-j/32))` itself and lands
/// ~2.6e-6 away from the baked tables. That looks like nothing, but every downstream Q8_0
/// `mul_mat` re-quantises its F32 activations, and a perturbation that crosses a block-rounding
/// boundary gets amplified sharply; compounded over 28 layers it was a large part of the Loop-10
/// probability gap. Measured on the reference question: layer-0 flash-attention output moved from
/// 2.25e-6 to 9.2e-8 L2-relative vs `ggmlc`, and the layer-0 output projection became bit-exact.
pub unsafe fn rope_baked(
    ctx: *mut s::ggml_context,
    x: *mut s::ggml_tensor, // [HD, seq, NH]
    cos: *mut s::ggml_tensor,
    sin: *mut s::ggml_tensor,
    seq: i64,
) -> *mut s::ggml_tensor {
    let half = HD / 2;
    let (nb1, nb2) = ((*x).nb[1], (*x).nb[2]);
    let lo = s::ggml_cont(ctx, s::ggml_view_3d(ctx, x, half, seq, NH, nb1, nb2, 0));
    let hi = s::ggml_cont(ctx, s::ggml_view_3d(ctx, x, half, seq, NH, nb1, nb2, (half * 4) as usize));
    // rotate_half(x) = cat(-x[HD/2..], x[..HD/2])
    let rot = s::ggml_concat(ctx, s::ggml_neg(ctx, hi), lo, 0);
    s::ggml_add(ctx, s::ggml_mul(ctx, x, cos), s::ggml_mul(ctx, rot, sin))
}

/// q/k/v (each already head-major `[HD, seq, NH]`, see [`split_head`]) ->
/// `softmax(QK^T / sqrt(HD) + mask) V`, returned as `[D, seq]`.
///
/// Deliberately `ggml_flash_attn_ext` with K/V cast to F16, because that is exactly what the
/// `ggmlc` runtime does for head_dim=64 (`runtime/src/executor.cpp`, `is_fattn_supported`) — the
/// goal is agreement with that reference, not maximum accuracy. Q stays F32 there too.
pub unsafe fn sdpa(
    ctx: *mut s::ggml_context,
    q: *mut s::ggml_tensor,
    k: *mut s::ggml_tensor,
    v: *mut s::ggml_tensor,
    mask: *mut s::ggml_tensor,
    seq: i64,
) -> *mut s::ggml_tensor {
    let k = s::ggml_cast(ctx, k, s::GGML_TYPE_F16);
    let v = s::ggml_cast(ctx, v, s::GGML_TYPE_F16);
    let o = s::ggml_flash_attn_ext(ctx, q, k, v, mask, 1.0 / (HD as f32).sqrt(), 0.0, 0.0);
    s::ggml_reshape_2d(ctx, o, D, seq) // [HD, NH, seq] -> [D, seq]
}
