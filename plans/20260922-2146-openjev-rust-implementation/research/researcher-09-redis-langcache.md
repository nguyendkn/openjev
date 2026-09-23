# Researcher-09: Redis LangCache — should openjev-rs integrate it?

## TL;DR
**KHÔNG NÊN tích hợp.** LangCache giải quyết đúng vấn đề (cache lặp câu hỏi) nhưng sai công cụ: nó là managed
Cloud service, thêm network dependency + embedding-inference overhead cho MỌI request (kể cả cache miss), không
có Rust SDK, và quan trọng nhất — bottleneck đã xác nhận qua Loop 6-10 là COMPUTE (threads/quant/build
flags/native ggml rewrite), không phải I/O. Semantic similarity matching giải quyết "paraphrase variation" —
vấn đề KHÔNG tồn tại ở đây vì benchmark suite dùng 10 kịch bản câu hỏi CỐ ĐỊNH (nghiên cứu-05), không phải
traffic người dùng biến thiên. Nếu muốn cache lặp-câu-hỏi, một `HashMap` exact-match trong Rust, tại chỗ, đã có
đủ lợi ích với chi phí ~0.

## 1. LangCache là gì, cơ chế hoạt động
- Managed semantic-caching service trên **Redis Cloud** (public preview, không phải core Redis open-source
  feature). Flow: app POST `/v1/caches/{cacheId}/entries/search` → LangCache gọi embedding model riêng
  (`redis/langcache-embed-v3-small`, ~20M params, max 128 tokens) để encode prompt → tìm kiếm vector-similarity
  trong Redis (RediSearch module) → trả cache hit nếu similarity vượt threshold, ngược lại miss.
- Mỗi request — kể cả cache-miss — phải trả **1 lần network round-trip tới Redis Cloud + 1 lần embedding
  inference** trước khi biết hit/miss. Không có số latency-overhead công khai cụ thể (ms) từ Redis; chỉ có
  claim marketing "cache HIT tới 15x nhanh hơn" (so với gọi lại LLM lớn) — không phải overhead của bước check.
- Redis core open-source (BSD-ish/RSALv2 tùy version) tách biệt khỏi LangCache — LangCache-the-product hiện
  chỉ nghe nói chạy trên Redis Cloud (managed), KHÔNG xác nhận được self-host on-prem cho chính LangCache
  service (chỉ Redis Stack/RediSearch — nền tảng bên dưới — là self-hostable, nhưng đó là tự xây semantic
  cache thủ công, không phải dùng LangCache sản phẩm).

## 2. Hạ tầng, license, pricing
- Yêu cầu **Redis Cloud account** (network dependency mới hoàn toàn, ra khỏi máy chủ CPU-only hiện tại) —
  không có bản self-hosted xác nhận công khai cho LangCache-product.
- Pricing: consumption-based (token + data dùng), không công bố số cụ thể — cần liên hệ sales/xem calculator.
  Không phải free/open-source cho phần LangCache.
- SDK: **Python và JavaScript only** — không có Rust SDK. Tích hợp từ Rust phải tự gọi REST API thô (qua
  `reqwest`), tự serialize/deserialize, tự xử lý auth/retry — không tận dụng được `redis` crate (redis-rs,
  mature, production-ready) vì đó là giao thức RESP thấp cấp, khác hẳn REST API của LangCache.

## 3. Có cần semantic similarity không, hay HashMap exact-match là đủ?
- Benchmark suite của dự án dùng **10 kịch bản câu hỏi cố định** (nghiên cứu-05: email routing, jailbreak
  detection, invoice categorization, v.v., lặp lại qua nhiều vòng tuning Loop 6-10) — không phải câu hỏi người
  dùng thật biến thiên cách diễn đạt. Với input cố định, `(model, prompt, options)` là key chính xác, tái lặp
  y hệt mỗi lần — **exact-match HashMap đạt gần 100% lợi ích cache-hit mà semantic similarity nhắm tới**, không
  cần đo độ tương đồng ngữ nghĩa vì câu hỏi không hề "gần giống" — nó GIỐNG HỆT.
- Semantic caching chỉ có giá trị khi traffic paraphrase (vd người dùng hỏi "reset my password" vs "I can't log
  in") — use case đó không khớp với benchmark tool, nơi input được kiểm soát hoàn toàn bởi test harness.

## 4. Chi phí tích hợp vs kiến trúc hiện tại
- `apps/server` (`app.rs`) đã dùng pattern actor/channel: 1 worker thread duy nhất giữ `HashMap<String, Engine>`
  (model cache), MỌI request serialize qua `mpsc::Sender<WorkerJob>` + `oneshot` reply. Đã có graceful-degradation
  pattern riêng cho Laya (`laya serve` là process ngoài, lỗi thì `laya: None`, không 500 toàn bộ request).
- Thêm Redis LangCache = thêm **1 external network call nằm TRÊN critical path**, không giống Laya (Laya là
  optional side-comparison, cache-check semantic thì cần chạy TRƯỚC khi quyết định có compute hay không) → nếu
  Redis Cloud down/chậm, hoặc phải fallback y hệt pattern Laya (bỏ qua cache, luôn compute) — thêm 1 lớp
  complexity + 1 điểm lỗi mới cho lợi ích không rõ ràng.
- Ngược lại, một `HashMap<CacheKey, BenchReport>` sống trong CHÍNH worker thread hiện có (cùng chỗ với
  `engines: HashMap<String, Engine>` trong `worker_loop`, `apps/server/src/app.rs:144`) — không cần thread mới,
  không cần network, không cần dependency mới, tận dụng luôn serialize-guarantee sẵn có.

## 5. Cache có giải quyết đúng bottleneck không?
- Mục tiêu tối ưu của dự án qua Loop 6-10 (threads tuning, quant, build flags, và hiện đang viết lại native
  ggml — researcher-08) là **giảm độ trễ TÍNH TOÁN của 1 lần suy luận CPU**, không phải I/O/network. Đây là
  compute-bound workload đã xác nhận nhiều lần.
- Cache (dù exact-match hay semantic) **không giúp được gì cho lần gọi ĐẦU TIÊN** của 1 câu hỏi/model mới — nó
  chỉ giúp lần lặp lại. Với 10 kịch bản benchmark cố định chạy lặp qua nhiều vòng tuning, cache có thể hữu ích
  để tránh recompute khi KHÔNG đổi code — nhưng đó lại mâu thuẫn với mục đích của benchmark tool: mỗi vòng tuning
  (đổi threads/quant/build flags) CẦN đo lại timing thật, cache-hit trả kết quả cũ sẽ che giấu chính con số cần
  đo, trừ khi có invalidation logic gắn với build/config fingerprint (thêm phức tạp, không miễn phí).
- Kết luận: cache đúng là "giải quyết sai bottleneck" theo nghĩa nó không đụng vào compute path (đúng mục tiêu
  dự án), và với công cụ benchmark thì cache còn có nguy cơ **phản tác dụng** (làm sai lệch số đo) nếu áp dụng
  không cẩn thận.

## Khuyến nghị
**KHÔNG tích hợp Redis LangCache.** Trade-off (network dependency mới, không Rust SDK, thêm điểm lỗi trên
critical path, chi phí Cloud) lớn hơn hẳn lợi ích thực tế cho 1 benchmark tool CPU-bound, câu hỏi cố định,
chạy trên 1 server. Nếu tương lai thực sự cần tránh recompute cho câu hỏi lặp (vd demo interactive, không phải
đo timing), giải pháp đơn giản hơn nhiều: `HashMap<(model, prompt, options), BenchReport>` trong worker thread
hiện có của `apps/server`, có cờ `--no-cache`/TTL hoặc gắn theo build-fingerprint để không làm sai số benchmark.
Không cần dependency mới nào.

## Câu hỏi chưa xác minh
1. Số ms cụ thể overhead của bước "gọi embedding model + tìm kiếm vector" trong LangCache (Redis không công bố
   con số độc lập, chỉ có claim tổng "cache hit 15x nhanh hơn LLM call gốc").
2. LangCache-the-product có bản self-hosted/on-prem chính thức nào ngoài Redis Cloud không, hay bắt buộc Cloud
   (trang docs `/develop/ai/langcache/` trả 404 khi fetch trực tiếp — chỉ có thông tin gián tiếp qua search).
3. Pricing cụ thể (per-token/per-GB) — không tìm thấy bảng giá công khai, chỉ "consumption-based".
4. License chính xác của Redis Stack/RediSearch bản hiện tại (đã đổi license nhiều lần — RSALv2/SSPL/lại về
   AGPL tùy thời điểm) — ảnh hưởng nếu về sau muốn tự xây semantic cache dựa trên RediSearch thay vì LangCache.
