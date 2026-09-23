set -x
export LD_LIBRARY_PATH=/app T=16
M=/work/models
OUT=/work/results/qwen3-0.6b.json  python3 /work/bench_llm.py \
  q8_0_official=$M/Qwen3-0.6B-Q8_0.gguf \
  q4_k_m=$M/Qwen3-0.6B-mk-Q4_K_M.gguf \
  q5_k_m=$M/Qwen3-0.6B-mk-Q5_K_M.gguf \
  q6_k=$M/Qwen3-0.6B-mk-Q6_K.gguf \
  q8_0_local=$M/Qwen3-0.6B-mk-Q8_0.gguf \
  f16=$M/Qwen3-0.6B-F16.gguf
OUT=/work/results/minicpm5-2b.json python3 /work/bench_llm.py \
  q4_k_m_official=$M/MiniCPM5-2B-Q4_K_M.gguf \
  q5_k_m=$M/MiniCPM5-2B-mk-Q5_K_M.gguf \
  q6_k=$M/MiniCPM5-2B-mk-Q6_K.gguf \
  q8_0_official=$M/MiniCPM5-2B-Q8_0.gguf \
  f16=$M/MiniCPM5-2B-F16.gguf
OUT=/work/results/qwen3-4b.json     python3 /work/bench_llm.py \
  q4_k_m_official=$M/Qwen3-4B-Q4_K_M.gguf \
  q5_k_m_official=$M/Qwen3-4B-Q5_K_M.gguf \
  q6_k_official=$M/Qwen3-4B-Q6_K.gguf \
  q8_0_official=$M/Qwen3-4B-Q8_0.gguf
echo SWEEPDONE
