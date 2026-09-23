export LD_LIBRARY_PATH=/app
F=/work/wikitext-2-raw/wiki.test.raw
for m in Qwen3-0.6B-Q8_0 Qwen3-0.6B-mk-Q4_K_M Qwen3-0.6B-mk-Q5_K_M Qwen3-0.6B-mk-Q6_K Qwen3-0.6B-mk-Q8_0 Qwen3-0.6B-F16 \
         MiniCPM5-2B-Q4_K_M MiniCPM5-2B-mk-Q5_K_M MiniCPM5-2B-mk-Q6_K MiniCPM5-2B-Q8_0 MiniCPM5-2B-F16 \
         Qwen3-4B-Q4_K_M Qwen3-4B-Q5_K_M Qwen3-4B-Q6_K Qwen3-4B-Q8_0; do
  echo "##MODEL $m"
  /app/llama-perplexity -m /work/models/$m.gguf -f $F -t 16 -c 512 --chunks 120 2>&1 | grep "Final estimate"
done
echo PPLDONE
