//! The hand-rolled ggml graph: 28-layer ModernBERT-large encoder + the typed decision head
//! (`head.rs`). Built once for a given padded sequence length and reused across requests; per
//! request only the input tensor DATA is rewritten.
//!
//! Op order transcribed node-by-node from the GGUF's own `ggmlc.graph_spec` (1364 nodes), not
//! from the HF reference: layer 0 has NO attention pre-norm, every other LayerNorm is
//! affine-free (its gamma is folded into the following Linear), attention alternates
//! global (layers 0,3,...,27, RoPE theta=160000) / sliding-window (RoPE theta=10000), and RoPE
//! reads the GGUF's baked cos/sin tables rather than recomputing trig (see `attn::rope_baked`).
use crate::attn::{rope_baked, rope_table, sdpa, split_head, split_head_view};
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
    /// Encoder-only sub-graph (stops at `hidden`), for the stage profiler. Shares `ctx`.
    pub gf_enc: *mut s::ggml_cgraph,
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
    /// Persistent `ggml_cplan` scratch buffer, grown on demand and reused across every forward
    /// pass — see [`Graph::run`] for why this is not left to `ggml_graph_compute_with_ctx`.
    work: std::cell::RefCell<Vec<u8>>,
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
        let _sp = timing::perf_span!("laya_native::graph::build", "seq" => seq.to_string());
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
            let gf_enc = s::ggml_new_graph_custom(ctx, 8192, false);

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
            let sp_embed = timing::perf_span!("laya_native::graph::build::embed");
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
            drop(sp_embed);

            // One span per 4-layer group (D_13 Task 1), so the log shows whether graph
            // construction cost is uniform across depth or concentrated somewhere.
            const GROUP_NAMES: [&str; 7] = [
                "laya_native::graph::build::layers_00_03",
                "laya_native::graph::build::layers_04_07",
                "laya_native::graph::build::layers_08_11",
                "laya_native::graph::build::layers_12_15",
                "laya_native::graph::build::layers_16_19",
                "laya_native::graph::build::layers_20_23",
                "laya_native::graph::build::layers_24_27",
            ];
            let mut sp_group = timing::perf_span!(GROUP_NAMES[0]);
            for l in 0..LAYERS {
                if l > 0 && l % 4 == 0 {
                    sp_group = timing::perf_span!(GROUP_NAMES[l / 4]);
                }
                let global = l % 3 == 0;
                let (mask, cos, sin) = if global { (mask_g16, cos_g, sin_g) } else { (mask_l16, cos_l, sin_l) };
                // layer 0's attn_norm is Identity in ModernBERT (graph_spec node 18 reads the
                // embedding norm output directly).
                let h = if l == 0 { x } else { s::ggml_norm(ctx, x, EPS) };
                let qkv = s::ggml_mul_mat(ctx, w.expect(&format!("qkv.{l}.weight"), &[D, 3 * D], s::GGML_TYPE_Q8_0)?, h);
                let rope = |t: *mut s::ggml_tensor| rope_baked(ctx, t, cos, sin, seq);
                let q = rope(split_head(ctx, qkv, seq, 0));
                let k = rope(split_head(ctx, qkv, seq, 1));
                let v = split_head_view(ctx, qkv, seq, 2);
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
                // D_13: `ggml_geglu` replaces slice-gate + slice-up + gelu + mul (6 nodes, plus
                // two materialised [FF, seq] copies) with ONE fused node. Bit-exact, not merely
                // close: ggml's `ggml_vec_geglu_f32` is literally
                // `gelu_table_lookup(x[i]) * g[i]`, the same table and the same f32 multiply that
                // `ggml_gelu` + `ggml_mul` perform in two passes, and `swapped = false` means it
                // takes the first half as the gated side — exactly this graph's gate/up order.
                let g = s::ggml_geglu(ctx, wi);
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
            drop(sp_group);
            let sp_head = timing::perf_span!("laya_native::graph::build::head");
            x = s::ggml_mul(ctx, s::ggml_norm(ctx, x, EPS), w.expect("final_norm_w", &[D], s::GGML_TYPE_F32)?);
            x = maybe_inject_hidden(ctx, x, seq)?;
            dbg.push(("final_norm".into(), x));
            let hidden = x;

            let logits = head::build(ctx, w, hidden, qtype, mpos, mask_g16, seq, &mut dbg)?;

            s::ggml_build_forward_expand(gf, hidden);
            s::ggml_build_forward_expand(gf, logits);
            s::ggml_build_forward_expand(gf_enc, hidden);
            drop(sp_head);
            // G23: `mem` above is a heuristic. ggml's bump allocator aborts rather than corrupting
            // if it runs out, but an abort inside FFI is a terrible failure mode — so turn
            // "nearly out of arena" into an ordinary Err while there is still headroom.
            let used = s::ggml_used_mem(ctx) as usize;
            if used * 100 > mem * 95 {
                return Err(format!("graph arena nearly exhausted: used {used} of {mem} bytes (seq={seq})"));
            }
            let ctx = guard.release(); // built successfully: ownership moves into the Graph
            Ok(Graph {
                ctx, gf, gf_enc, seq, kmax, inp_ids, mask_g, mask_l, qtype, mpos, hidden, logits,
                dbg, work: std::cell::RefCell::new(Vec::new()),
            })
        }
    }

    pub fn n_nodes(&self) -> i32 {
        unsafe { s::ggml_graph_n_nodes(self.gf) }
    }

    /// `(op name, node count)` descending — D_13 candidate 1 ("which ops dominate node count").
    /// Every node costs an OpenMP barrier in `ggml_graph_compute`, including the pure-metadata
    /// view/reshape/permute ops that do no arithmetic at all, so this is the list to attack.
    pub fn op_histogram(&self) -> Vec<(String, i32)> {
        let mut h: std::collections::HashMap<String, i32> = std::collections::HashMap::new();
        unsafe {
            for i in 0..s::ggml_graph_n_nodes(self.gf) {
                let n = s::ggml_graph_node(self.gf, i);
                let name = std::ffi::CStr::from_ptr(s::ggml_op_name((*n).op)).to_string_lossy().into_owned();
                *h.entry(name).or_insert(0) += 1;
            }
        }
        let mut v: Vec<(String, i32)> = h.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v
    }

    /// `(arena bytes actually used, arena bytes reserved)` — the G23 headroom check.
    pub fn arena_usage(&self) -> (usize, usize) {
        unsafe { (s::ggml_used_mem(self.ctx) as usize, s::ggml_get_mem_size(self.ctx) as usize) }
    }

    pub fn compute(&self, n_threads: i32) -> Result<(), String> {
        let _sp = timing::perf_span!("laya_native::graph::compute");
        self.run(self.gf, n_threads)
    }

    /// Computes only the encoder prefix (everything `hidden` depends on). Used by the D_13
    /// stage-profiler to split the single `ggml_graph_compute` call into encoder vs head+scorer
    /// cost — the head's share is `compute() - compute_encoder_only()`.
    pub fn compute_encoder_only(&self, n_threads: i32) -> Result<(), String> {
        let _sp = timing::perf_span!("laya_native::graph::compute_encoder_only");
        self.run(self.gf_enc, n_threads)
    }

    /// D_13: deliberately NOT `ggml_graph_compute_with_ctx`. That helper does
    /// `cplan.work_data = ggml_new_buffer(ctx, cplan.work_size)` on **every** call, i.e. it
    /// bump-allocates a fresh multi-MB scratch buffer out of the graph arena per forward pass.
    /// Two consequences, both measured: every request touches never-faulted pages (the profile's
    /// 9.5% in `native_queued_spin_lock_slowpath` under `do_anonymous_page`, 28 threads racing on
    /// the same `pte` lock), and the arena grows without bound across requests — fine for a
    /// one-shot probe, a slow leak in a long-lived server. Planning into one reused, already-
    /// faulted `Vec<u8>` removes both. The computation itself is untouched, so this is bit-exact.
    fn run(&self, gf: *mut s::ggml_cgraph, n_threads: i32) -> Result<(), String> {
        unsafe {
            let mut plan = s::ggml_graph_plan(gf, n_threads, std::ptr::null_mut());
            let mut work = self.work.borrow_mut();
            if plan.work_size > work.len() {
                work.resize(plan.work_size, 0);
            }
            plan.work_data = if plan.work_size > 0 { work.as_mut_ptr() } else { std::ptr::null_mut() };
            let st = s::ggml_graph_compute(gf, &mut plan);
            if st != s::GGML_STATUS_SUCCESS {
                return Err(format!("ggml_graph_compute failed: {st:?}"));
            }
        }
        Ok(())
    }
}
