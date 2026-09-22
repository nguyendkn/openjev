# jq filter for scripts/bench-harness.sh: aggregates raw per-request timing JSONL (read via
# `jq -s`, so `.` here is the full array of request objects) into mean/median/max per timing
# field, per model, across all successful (error == null) scenarios in the run.

def median(a):
  (a | sort) as $s
  | ($s | length) as $n
  | if $n == 0 then null
    elif ($n % 2) == 1 then $s[($n - 1) / 2 | floor]
    else ($s[$n / 2 - 1] + $s[$n / 2]) / 2
    end;

def stats(a):
  { mean: (if (a | length) == 0 then null else (a | add) / (a | length) end),
    median: median(a),
    max: (if (a | length) == 0 then null else (a | max) end),
    n: (a | length) };

. as $all
| ($all | map(select(.error != null)) | length) as $error_count
| ($all | map(select(.error == null))) as $ok
| {
    total_requests: ($all | length),
    error_count: $error_count,
    per_model: (
      $ok
      | group_by(.model)
      | map({
          model: .[0].model,
          n: length,
          wall_ms: stats(map(.wall_ms)),
          model_load_ms: stats(map(.timings.model_load_ms)),
          warmup_ms: stats(map(.timings.warmup_ms)),
          tokenize_ms: stats(map(.timings.tokenize_ms)),
          constrained_readout_ms: stats(map(.timings.constrained_readout_ms)),
          generation_ms: stats(map(.timings.generation_ms)),
          laya_model_load_ms: stats(map(.timings.laya_model_load_ms)),
          laya_inference_ms: stats(map(.timings.laya_inference_ms))
        })
    )
  }
