set -e
cd /work/models
echo "=== convert Qwen3-0.6B safetensors -> F16 GGUF ==="
python3 /app/convert_hf_to_gguf.py /work/src/Qwen3-0.6B --outfile /work/models/Qwen3-0.6B-F16.gguf --outtype f16 2>&1 | tail -5
for q in Q4_K_M Q5_K_M Q6_K Q8_0; do
  echo "=== Qwen3-0.6B $q ==="
  /app/llama-quantize --allow-requantize /work/models/Qwen3-0.6B-F16.gguf /work/models/Qwen3-0.6B-mk-$q.gguf $q 20 2>&1 | tail -3
done
for q in Q5_K_M Q6_K; do
  echo "=== MiniCPM5-2B $q ==="
  /app/llama-quantize /work/models/MiniCPM5-2B-F16.gguf /work/models/MiniCPM5-2B-mk-$q.gguf $q 20 2>&1 | tail -3
done
echo ALLDONE
