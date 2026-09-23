#!/usr/bin/env bash
# Loop 8 fix: laya serve was deployed in Loops 5-7 with NO --threads flag, silently defaulting
# to 4 CPU workers (per `laya --help`) on a 32-vCPU box -- a 5x perf regression that went
# unnoticed because Laya's speed was never benchmarked in isolation. This script is the single,
# canonical way to (re)start `laya serve` so that regression can't silently recur from an ad-hoc
# `setsid nohup ...` typed by hand. Prefer the systemd unit (`openjev-laya-serve.service`,
# installed by this same loop) for production; this script is what that unit's ExecStart runs,
# and is also usable standalone for local/dev restarts.
#
# Usage: ./start-laya-serve.sh [threads] [port] [gguf_path]
set -euo pipefail

THREADS="${1:-28}"
PORT="${2:-8090}"
# Loop 8 quant check: UD_Q4_K_M's K-quant CPU dequant path in this `ggmlc` build is measurably
# slower than Q8_0/F16 on this box (single-request /v1/decide, 5 reps each, threads=28: Q4_K_M
# mean ~1.90s, Q8_0 mean ~1.49s, F16 mean ~1.45s -- Q8_0 chosen as the production default: same
# ~20% win as F16 at half the file size/RAM). See docs/benchmarks/server-tuning-results.md Loop 8
# section for the full comparison.
GGUF="${3:-/root/.cache/laya-models/laya_english_q8_0.gguf}"
LAYA_BIN="${LAYA_BIN:-/tmp/ggmlc/build/examples/laya/laya}"
LOG_DIR="${LOG_DIR:-/tmp/openjev/logs}"

mkdir -p "$LOG_DIR"
echo "starting laya serve: threads=$THREADS port=$PORT gguf=$GGUF" >&2
exec "$LAYA_BIN" serve "$GGUF" --port "$PORT" --device cpu --threads "$THREADS" >>"$LOG_DIR/laya-serve.log" 2>&1
