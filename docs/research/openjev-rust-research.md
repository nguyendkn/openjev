# Research: OpenJev / SemIf — porting to Rust

**Scope**: hiểu openjev.com (SemIf) để triển khai lại bằng Rust. Recency: 2026, ưu tiên GitHub gốc + crates.io hiện hành. Nguồn: web fetch trang chính + repo gốc, web search hệ sinh thái Rust LLM inference.

## 1. OpenJev / SemIf là gì

- Tên hiện tại: **SemIf** (Semantic If), trước đây gọi là OpenJev — demo tại openjev.com.
- Ý tưởng lõi: "semantic if" — dùng một LLM cục bộ để trả lời câu hỏi trắc nghiệm (2–20 lựa chọn) và so sánh **2 cách suy luận**:
  1. **Direct readout (constrained single-token)**: 1 forward pass, đọc log-prob của các token nhãn hợp lệ (vd "A","B","C"...), normalize (softmax) chỉ trên các option hiển thị → xác suất "có điều kiện trên các nhãn", KHÔNG phải confidence hiệu chỉnh.
  2. **Generation**: model tự sinh JSON `{"option": prob, ...}` token-by-token (greedy, giới hạn 512 token), phải strip block `<think>...</think>` trước khi parse & validate JSON.
- Đo hiệu năng thực (wall-clock `performance.now()`), so sánh 2 phương pháp. Vd trên RTX 3090: direct ~1.02s cho 21 tiêu chí nhị phân vs generation ~5.33s.
- Không phải chatbot — là công cụ benchmark/lab cho kỹ thuật "typed decision/classification qua logits" (tương tự structured output nhưng đọc trực tiếp phân phối xác suất thay vì decode JSON).

## 2. Kiến trúc bản gốc (browser, workszop/openjev)

| Layer | Công nghệ |
|---|---|
| Inference runtime | WebGPU, qua **wllama** (wrapper WASM của llama.cpp) 3.6.1, vendored |
| Model | Qwen3-0.6B Q8_0 GGUF (~639MB), cũng hỗ trợ MiniCPM4 2B, Qwen3 4B |
| Frontend | Vue 3.5.21 + Lucide icons |
| Deploy | Static hosting cần header COOP/COEP (Cloudflare Pages/Netlify) |
| File structure | `index.html`/`app.js` (UI), `worker.js` (Web Worker inference), `serve.py` (local HTTPS dev server) |

Luồng load model: kiểm tra WebGPU → tải model từ Hugging Face (pin theo revision) vào cache trình duyệt → khởi tạo wllama engine → warmup ("Paris is the capital of..." sanity check).

Đo lường tách biệt các giai đoạn: model load / warmup / tokenize / constrained readout / generation — quan trọng để port sang Rust giữ đúng benchmark methodology.

## 3. Hệ sinh thái Rust cho phần lõi (inference + logits)

Không có "port chính thức" bằng Rust — phải tự dựng dựa trên 2 lựa chọn runtime GGUF:

**A. `candle` + `candle-transformers`** (Hugging Face, pure Rust, không cần bind C++)
- Có `quantized_llama.rs`/Qwen3 quantized GGUF support sẵn trong candle-transformers; `forward()` trả logits chỉ cho token cuối cùng — phù hợp trực tiếp cho constrained single-token readout (không cần decode thêm).
- Có activity 2026 mở rộng Qwen3.5 (Gated DeltaNet + partial RoPE) — cho thấy hệ sinh thái đang theo kịp dòng model mới, nhưng còn non (PR đang review).
- Ưu điểm: build thuần Rust, dễ deploy cross-platform, không phụ thuộc libllama.cpp native.
- Nhược điểm: hiệu năng & tối ưu kernel kém llama.cpp gốc hơn ở một số backend; ít tính năng constrained-grammar decoding có sẵn so với llama.cpp.

**B. Bindings tới llama.cpp**: `llama-cpp-2` (thin wrapper theo sát C API), `llama_cpp` (bindings an toàn cấp cao hơn), `llm_client`, `drama_llama`.
- Tận dụng toàn bộ tối ưu GGUF/GGML gốc (kernel CPU/GPU, KV cache, đã có sẵn cơ chế lấy logits/probs).
- Cảnh báo thực tế: bật `--flash-attn` cùng quantized KV cache có thể silent fallback về CPU — cần đo & log thay vì tin cấu hình.
- Phù hợp hơn nếu ưu tiên đúng hành vi/perf tham chiếu với bản gốc (vốn cũng chạy trên llama.cpp qua wllama).

**Khuyến nghị**: dùng `llama-cpp-2` (bindings mỏng, sát API C, kiểm soát được logits thô) làm engine chính vì bản gốc SemIf cũng chạy trên llama.cpp (qua wllama) — giữ tương đồng hành vi/số liệu benchmark. `candle` là phương án dự phòng nếu muốn 100% pure-Rust không phụ thuộc native lib.

## 4. Điểm cần giữ khi port sang Rust

1. **2 pipeline riêng biệt, đo thời gian độc lập**: model-load, warmup, tokenize, constrained-readout, generation.
2. **Constrained single-token readout**: giới hạn logits chỉ trên tập token nhãn hợp lệ trước softmax (không phải full-vocab softmax rồi lọc).
3. **Generation path**: giới hạn max_tokens=512, strip `<think>...</think>` trước khi `serde_json` parse, validate schema khớp options.
4. **Model**: bắt đầu với Qwen3-0.6B GGUF quantized (Q8_0 hoặc Q4_K_M để nhẹ hơn) tải từ Hugging Face, pin revision.
5. **Không có tầng "calibrated confidence"** — phải giữ disclaimer xác suất chỉ có điều kiện trên nhãn hiển thị.
6. Vì port sang backend (không phải browser WASM), có thể bỏ ràng buộc WebGPU/COOP-COEP, nhưng nên giữ tuỳ chọn CLI/HTTP API để so sánh 2 phương pháp — có thể expose qua CLI trước, tuỳ dùng thêm HTTP nếu cần multi-request sau.

## 5. Rủi ro / chưa chốt

- Chưa rõ user muốn Rust port ở dạng: CLI benchmark tool, hay service/API, hay cả GUI (khó vì gốc là web UI) — cần làm rõ ở bước plan.
- Chưa xác nhận license/điều khoản dùng lại trọng số Qwen3 GGUF cho mục đích distribute lại (nếu có).
- `candle-transformers` Qwen3.5 support còn ở dạng PR chưa merge tại thời điểm nghiên cứu — nếu chọn Qwen3.5 cần theo dõi merge status.

**Sources:**
- https://openjev.com/
- https://github.com/workszop/openjev
- https://github.com/HeapHeapHooray/JEV-CPU-Gemma4
- https://github.com/james-see/SemIf
- https://github.com/theoleecj/SemIf
- https://jevlist.ai/projects/semif
- https://buildwithjev.com/builds/kev-and-semif-open-source-jev-style-decision-models
- http://ai-tldr.dev/releases/theoleecj-semif/
- https://docs.rs/llama_cpp
- https://docs.rs/candle-transformers
- https://github.com/huggingface/candle/pull/3461
- https://docs.rs/candle-transformers/latest/src/candle_transformers/models/quantized_llama.rs.html
