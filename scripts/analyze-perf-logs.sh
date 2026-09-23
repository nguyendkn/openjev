#!/usr/bin/env bash
# D_11b Task 3: reads a window of `logs/perf/*.jsonl` perf-span records (written by
# `timing::logger`, see crates/timing/src/logger.rs) and prints a sorted span_name ->
# count/mean/p50/p95/max/total-time summary table -- this is what the coordinator reads each
# cycle to find hot spots and decide what to direct optimization at next.
#
# Usage: ./scripts/analyze-perf-logs.sh [minutes] [logs_dir]
#   minutes    only include *.jsonl files modified within the last N minutes (default: 0 = all)
#   logs_dir   default: <repo_root>/logs/perf

set -euo pipefail

MINUTES="${1:-0}"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
LOGS_DIR="${2:-$(cd "$SCRIPT_DIR/.." && pwd)/logs/perf}"

if [ ! -d "$LOGS_DIR" ]; then
  echo "no perf logs found at $LOGS_DIR (is PERF_LOG=1 set, and has traffic run yet?)" >&2
  exit 1
fi

if [ "$MINUTES" -gt 0 ] 2>/dev/null; then
  mapfile -t FILES < <(find "$LOGS_DIR" -name '*.jsonl' -mmin "-$MINUTES")
else
  mapfile -t FILES < <(find "$LOGS_DIR" -name '*.jsonl')
fi

if [ "${#FILES[@]}" -eq 0 ]; then
  echo "no perf log files matched in $LOGS_DIR (window: last ${MINUTES}m, 0=all)" >&2
  exit 1
fi

echo "== perf-log analysis: ${#FILES[@]} file(s), window=${MINUTES}m (0=all) ==" >&2

jq -s '
  group_by(.span_name)
  | map({
      span_name: .[0].span_name,
      count: length,
      mean_us: (map(.duration_micros) | add / length | round),
      p50_us: (
        (map(.duration_micros) | sort) as $sorted
        | $sorted[($sorted | length) * 0.50 | floor]
      ),
      p95_us: (
        (map(.duration_micros) | sort) as $sorted
        | (($sorted | length) * 0.95 | floor) as $i
        | $sorted[if $i >= ($sorted | length) then ($sorted | length) - 1 else $i end]
      ),
      max_us: (map(.duration_micros) | max),
      total_ms: ((map(.duration_micros) | add) / 1000 | round)
    })
  | sort_by(-.total_ms)
' "${FILES[@]}" \
| jq -r '
  (["span_name","count","mean_us","p50_us","p95_us","max_us","total_ms"] | @tsv),
  (.[] | [.span_name, .count, .mean_us, .p50_us, .p95_us, .max_us, .total_ms] | @tsv)
' \
| awk -F'\t' '
  { for (i = 1; i <= NF; i++) { if (length($i) > w[i]) w[i] = length($i) }; lines[NR] = $0; cols = NF }
  END {
    for (l = 1; l <= NR; l++) {
      n = split(lines[l], f, "\t")
      line = ""
      for (i = 1; i <= n; i++) { line = line sprintf("%-*s  ", w[i], f[i]) }
      print line
    }
  }
'
