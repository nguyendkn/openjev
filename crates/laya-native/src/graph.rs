//! The hand-rolled ggml graph: 28-layer ModernBERT-large encoder + the typed decision head
//! (`head.rs`). Built once for a given padded sequence length and reused across requests; per
//! request only the input tensor DATA is rewritten.
//!
//! Op order transcribed node-by-node from the GGUF's own `ggmlc.graph_spec` (1364 nodes), not
//! from the HF reference: layer 0 has NO attention pre-norm, every other LayerNorm is
//! affine-free (its gamma is folded into the following Linear), attention alternates
//! global (layers 0,3,...,27, RoPE theta=160000) / sliding-window (RoPE theta=10000), and RoPE
//! reads the GGUF's baked cos/sin tables rather than recomputing trig (see `attn::rope_baked`).
use crate::attn::{rope_baked, rope_table, sdpa, split_head};
use crate::debug::{maybe_inject_hidden, maybe_perturb};
use crate::gguf::Weights;
use crate::head;
use llama_cpp_sys_2 as s;

pub const D: i64 = 1024;
pub const NH: i64 = 16;
pub const HD: i64 = 64;
pub const WI: i64 = 5248;
pub const FF: i64 = 2624;
pub const LAYERS: usize = 28;
pub const HEAD_LAYERS: usize = 2;
pub const EPS: f32 = 1e-5;
pub const THETA_GLOBAL: f32 = 160000.0;
pub const THETA_LOCAL: f32 = 10000.0;

pub struct Graph {
    pub ctx: *mut s::ggml_context,
    pub gf: *mut s::ggml_cgraph,
    pub seq: i64,
    pub kmax: i64,
    pub inp_ids: *mut s::ggml_tensor,
    pub mask_g: *mut s::ggml_tensor,
    pub mask_l: *mut s::ggml_tensor,
    pub qtype: *mut s::ggml_tensor,
    pub mpos: *mut s::ggml_tensor,
    pub hidden: *mut s::ggml_tensor,
    pub logits: *mut s::ggml_tensor,
    /// Landmark intermediates, in execution order, for `dump_intermediates`. Collecting the
    /// pointers is free; nothing is read unless a caller explicitly asks for a dump.
    pub dbg: Vec<(String, *mut s::ggml_tensor)>,
}

/// Frees a `ggml_context` unless it has been released to a `Graph`. Keeps the many `?` exits in
/// `Graph::build` from leaking a multi-hundred-MB arena (G22).
struct CtxGuard(*mut s::ggml_context);

impl CtxGuard {
    fn release(&mut self) -> *mut s::ggml_context {
        std::mem::replace(&mut self.0, std::ptr::null_mut())
    }
}

impl Drop for CtxGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { s::ggml_free(self.0) };
        }
    }
}

/// G22: the arena is hundreds of MB per sequence bucket. Without this, every bucket leaked for
/// the process lifetime — harmless for the probe, not harmless in a long-lived server.
impl Drop for Graph {
    fn drop(&mut self) {
        // `gf` and every tensor (incl. `logits`/`hidden`) live inside `ctx`; freeing it frees
        // them all. Weight tensors belong to `Weights`' own context and are untouched here.
        unsafe { s::ggml_free(self.ctx) };
    }
}

impl Graph {
    pub fn build(w: &Weights, seq: i64, kmax: i64) -> Result<Graph, String> {
        // Upper bound on the arena: every intermediate stays materialised so the graph can be
        // recomputed in place. Deliberately simple (correctness-first); a gallocr pass would cut
        // this a lot. The post-build `ggml_used_mem` check below keeps the estimate honest.
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
            let mut guard = CtxGuard(ctx);
            let gf = s::ggml_new_graph_custom(ctx, 8192, false);

            let inp_ids = s::ggml_new_tensor_1d(ctx, s::GGML_TYPE_I32, seq);
            let mask_g = s::ggml_new_tensor_2d(ctx, s::GGML_TYPE_F32, seq, seq);
            let mask_l = s::ggml_new_tensor_2d(ctx, s::GGML_TYPE_F32, seq, seq);
            let qtype = s::ggml_new_tensor_1d(ctx, s::GGML_TYPE_I32, 1);
            let mpos = s::ggml_new_tensor_1d(ctx, s::GGML_TYPE_I32, kmax);
            // ggml_flash_attn_ext wants an F16 mask; ggmlc casts the same F32 masks the graph
            // builds, so cast once here and share across all layers.
            let (mask_g16, mask_l16) =
                (s::ggml_cast(ctx, mask_g, s::GGML_TYPE_F16), s::ggml_cast(ctx, mask_l, s::GGML_TYPE_F16));

            // --- embeddings + embedding LayerNorm (the only encoder norm that keeps its gamma)
            let mut dbg: Vec<(String, *mut s::ggml_tensor)> = Vec::new();
            let tok = w.expect("tok_emb.weight", &[D, 50368], s::GGML_TYPE_Q8_0)?;
            let mut x = s::ggml_get_rows(ctx, tok, inp_ids); // [D, seq]
            dbg.push(("embedding".into(), x));
            x = s::ggml_mul(ctx, s::ggml_norm(ctx, x, EPS), w.expect("emb_norm_w", &[D], s::GGML_TYPE_F32)?);
            x = maybe_perturb(ctx, x)?;
            dbg.push(("emb_norm".into(), x));

            // Baked RoPE tables, sliced + shaped once and shared by every layer of each kind.
            let (cos_g, sin_g) = (rope_table(ctx, w, "cos_full", seq)?, rope_table(ctx, w, "sin_full", seq)?);
            let (cos_l, sin_l) = (rope_table(ctx, w, "cos_slide", seq)?, rope_table(ctx, w, "sin_slide", seq)?);

            for l in 0..LAYERS {
                let global = l % 3 == 0;
                let (mask, cos, sin) = if global { (mask_g16, cos_g, sin_g) } else { (mask_l16, cos_l, sin_l) };
                // layer 0's attn_norm is Identity in ModernBERT (graph_spec node 18 reads the
                // embedding norm output directly).
                let h = if l == 0 { x } else { s::ggml_norm(ctx, x, EPS) };
                let qkv = s::ggml_mul_mat(ctx, w.expect(&format!("qkv.{l}.weight"), &[D, 3 * D], s::GGML_TYPE_Q8_0)?, h);
                let rope = |t: *mut s::ggml_tensor| rope_baked(ctx, t, cos, sin, seq);
                let q = rope(split_head(ctx, qkv, seq, 0));
                let k = rope(split_head(ctx, qkv, seq, 1));
                let v = split_head(ctx, qkv, seq, 2);
                let fa = sdpa(ctx, q, k, v, mask, seq);
                let a = s::ggml_mul_mat(ctx, w.expect(&format!("wo.{l}.weight"), &[D, D], s::GGML_TYPE_Q8_0)?, fa);
                if l == 0 {
                    dbg.push(("L0.qkv".into(), qkv));
                    dbg.push(("L0.fa_out".into(), fa));
                    dbg.push(("L0.wo_out".into(), a));
                }
                x = s::ggml_add(ctx, x, a);
                dbg.push((format!("L{l}.attn_res"), x));

                let h = s::ggml_norm(ctx, x, EPS);
                let wi = s::ggml_mul_mat(ctx, w.expect(&format!("mlp_wi.{l}.weight"), &[D, WI], s::GGML_TYPE_Q8_0)?, h);
                let gate = s::ggml_cont(ctx, s::ggml_view_2d(ctx, wi, FF, seq, (WI * 4) as usize, 0));
                let up = s::ggml_cont(ctx, s::ggml_view_2d(ctx, wi, FF, seq, (WI * 4) as usize, (FF * 4) as usize));
                let g = s::ggml_mul(ctx, s::ggml_gelu(ctx, gate), up);
                let o = s::ggml_mul_mat(ctx, w.expect(&format!("mlp_wo.{l}.weight"), &[FF, D], s::GGML_TYPE_Q8_0)?, g);
                if l <= 2 {
                    dbg.push((format!("L{l}.mlp_norm"), h));
                    dbg.push((format!("L{l}.mlp_wi"), wi));
                    dbg.push((format!("L{l}.mlp_geglu"), g));
                    dbg.push((format!("L{l}.mlp_wo_out"), o));
                }
                x = s::ggml_add(ctx, x, o);
                dbg.push((format!("L{l}.mlp_res"), x));
            }
            x = s::ggml_mul(ctx, s::ggml_norm(ctx, x, EPS), w.expect("final_norm_w", &[D], s::GGML_TYPE_F32)?);
            x = maybe_inject_hidden(ctx, x, seq)?;
            dbg.push(("final_norm".into(), x));
            let hidden = x;

            let logits = head::build(ctx, w, hidden, qtype, mpos, mask_g16, seq, &mut dbg)?;

            s::ggml_build_forward_expand(gf, hidden);
            s::ggml_build_forward_expand(gf, logits);
            // G23: `mem` above is a heuristic. ggml's bump allocator aborts rather than corrupting
            // if it runs out, but an abort inside FFI is a terrible failure mode — so turn
            // "nearly out of arena" into an ordinary Err while there is still headroom.
            let used = s::ggml_used_mem(ctx) as usize;
            if used * 100 > mem * 95 {
                return Err(format!("graph arena nearly exhausted: used {used} of {mem} bytes (seq={seq})"));
            }
            let ctx = guard.release(); // built successfully: ownership moves into the Graph
            Ok(Graph { ctx, gf, seq, kmax, inp_ids, mask_g, mask_l, qtype, mpos, hidden, logits, dbg })
        }
    }

    pub fn n_nodes(&self) -> i32 {
        unsafe { s::ggml_graph_n_nodes(self.gf) }
    }

    /// `(arena bytes actually used, arena bytes reserved)` — the G23 headroom check.
    pub fn arena_usage(&self) -> (usize, usize) {
        unsafe { (s::ggml_used_mem(self.ctx) as usize, s::ggml_get_mem_size(self.ctx) as usize) }
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
