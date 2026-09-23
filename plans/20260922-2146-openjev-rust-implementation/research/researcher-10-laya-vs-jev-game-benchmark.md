# Research: Laya vs Jev — game benchmark claim

## Bối cảnh xác nhận được
TypeSafe ra mắt model "Jev" (decision model, không sinh text) ngày 15/09/2026. ~3 ngày sau (~18/09/2026),
Convai Innovations phát hành **Laya** (421M param, Apache 2.0, open-weights) làm đối trọng mã nguồn mở.
Đây là sự kiện có thật, lan truyền rộng qua X/blog/GitHub trong tuần 15–23/09/2026 — KHÔNG phải 1 bài
báo chính thức duy nhất, mà là hàng chục demo/benchmark cộng đồng rời rạc, không đồng nhất phương pháp.

## Game "xếp hình" = Tetris (xác nhận được)
Tìm thấy nhiều bản demo AI-chơi-Tetris dùng Laya vs Jev:
- Tweet gốc lan truyền nhất: atomic.chat (X, @atomic_chat_hq, status 2102160983409955244):
  > "Local Laya moggs Jev at @grok 4.7-built Tetris. An open-weights System One model called Laya,
  > beat cloud-based Jev at playing Tetris by making decisions 11 times faster, running locally on a
  > 16GB MacBook Air!"
  (https://x.com/atomic_chat_hq/status/2102160983409955244)
- GitHub `huhao121/jev-tetris`, `thelau/jev-tetris`, `Tsagaanbayr1/jev-tetris` — các bản độc lập cùng
  ý tưởng "Tetris do model quyết định nước đi".

## KHÔNG xác nhận được: "Laya đạt điểm tối đa"
Claim này KHÔNG khớp với dữ liệu benchmark cộng đồng tìm thấy — thậm chí bị mâu thuẫn:
- PR `Tsagaanbayr1/jev-tetris#1` (headless match, seed 4242): **Jev thắng trận**, Laya "topped out"
  (thua vì đầy bảng) — ngược hoàn toàn với claim của user.
- 1 benchmark khác (nguồn qua search, không tên tác giả rõ): "None of the three models cleared a
  Tetris line ... Laya's probabilities over the six candidates are nearly flat, it prefers whichever
  options are listed first" — tức Laya chơi kém, không đạt điểm tối đa.
- Không tìm thấy bất kỳ nguồn nào nói Laya đạt "perfect/max score" trong Tetris.
→ Nhiều khả năng user nhớ nhầm/diễn giải sai từ 1 trong các bài marketing về **tốc độ** (không phải
điểm số) của Laya.

## Số liệu Decision Time cụ thể (nguyên văn, nhiều nguồn khác nhau — KHÔNG đồng nhất)
- Model card Laya (chính chủ): "32.8ms p50 latency" vs Jev "236 to 276ms p50 latency" (~7.8x).
  (https://mer.vin/news/laya-the-33ms-open-source-decision-model-beating-jev/)
- PR `Tsagaanbayr1/jev-tetris#1`: Jev "~290 ms" vs Laya "~82 ms (MPS fp32)" trên M1 Pro (~3.5x).
- Bài AlphaSignal: "7x Faster Decision Speed".
- AI Weekly: "Laya-CoreML port hits 4.98 ms decisions on M3 Max Neural Engine" — con số gần "5x ms"
  nhất tìm được, nhưng đây là port riêng (CoreML/M3 Max), không phải bản gốc, và không so trực tiếp X ms.
- Tweet atomic.chat (Tetris cụ thể): "11 times faster" — không cho số ms tuyệt đối.
→ KHÔNG có nguồn nào nói đúng "5x ms Decision Time"; các con số dao động 3.5x–11x tùy nguồn/máy đo,
và ~4.98ms/~5ms chỉ xuất hiện ở 1 bản port CoreML, không phải benchmark game.

## Đánh giá độ tin cậy
- Không có benchmark chính thức/có phương pháp luận công bố chung giữa 2 hãng (TypeSafe không tự so Tetris).
- Toàn bộ là marketing blog (mer.vin, dev.to, AlphaSignal, AI Weekly, Flowtivity...) diễn giải lại model
  card của Laya (bên thắng tự công bố), + demo cá nhân trên X/GitHub không kiểm soát biến số (phần cứng,
  seed, phiên bản Jev khác nhau: "1.13.0").
- Các GitHub PR (nguồn kỹ thuật độc lập nhất) cho kết quả NGƯỢC claim marketing về chất lượng chơi
  (Jev thắng match, Laya topped out / không clear được dòng nào).
- Kết luận: tin về TỐC ĐỘ (Laya nhanh hơn Jev nhiều lần) có cơ sở, lặp lại nhất quán qua nhiều nguồn.
  Tin về ĐIỂM SỐ TỐI ĐA của Laya trong Tetris KHÔNG có cơ sở, mâu thuẫn với bằng chứng kỹ thuật tìm được.

## Chưa tìm thấy
- Bài viết "sáng nay 23/09/2026" cụ thể mà user nhắc tới — có thể là 1 trong các bài trên đăng ngày
  gần đó, nhưng không xác định được bài NÀO chính xác khớp mọi chi tiết user mô tả (max score + 5x ms).
- Đề xuất: nếu user có link cụ thể, gửi để đối chiếu trực tiếp — tin loại này lan truyền rất nhanh, có
  thể có bản mới hơn 22-23/09 chưa được index đầy đủ.

## Nguồn chính đã kiểm tra
- https://mer.vin/news/laya-the-33ms-open-source-decision-model-beating-jev/
- https://x.com/atomic_chat_hq/status/2102160983409955244
- https://github.com/Tsagaanbayr1/jev-tetris/pull/1
- https://aiweekly.co/alerts/laya-coreml-port-hits-498-ms-decisions-on-m3-max-neural-engine
- https://alphasignal.ai/news/open-source-laya-beats-closed-jev-api-at-7x-faster-decision-speed
- https://huggingface.co/datasets/Luni/laya-jev-benchmark
