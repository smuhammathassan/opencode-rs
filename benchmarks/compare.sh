#!/usr/bin/env bash
# Differential CPU/RAM benchmark: opencode-rs vs stock opencode v1.18.13.
set -euo pipefail

STOCK="/root/.opencode/bin/opencode"
RS="/root/opencode-rs/target/release/opencode"
OUT="/root/opencode-rs/benchmarks/results.txt"
RUNS=8

measure() {
  local label="$1"; shift
  local bin="$1"; shift
  local total_ns=0 peak=0 p t0 t1
  for _ in $(seq 1 "$RUNS"); do
    t0=$(date +%s%N)
    /usr/bin/time -v "$bin" "$@" >/dev/null 2>/tmp/time.txt || true
    t1=$(date +%s%N)
    total_ns=$((total_ns + t1 - t0))
    p=$(grep -E "Maximum resident set size" /tmp/time.txt | awk '{print $6}')
    peak=$((peak > p ? peak : p))
  done
  avg_ms=$((total_ns / RUNS / 1000000))
  peak_mb=$((peak / 1024))
  printf "%-28s %-22s avg %6d ms   peak RSS %6d MB\n" "$label" "$bin" "$avg_ms" "$peak_mb" | tee -a "$OUT"
}

{
  echo "opencode-rs differential benchmark — $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "machine: $(uname -m), $(nproc) cores, $(free -h | awk '/Mem:/{print $2}') RAM"
  echo "stock: $(stat -c%s "$STOCK") bytes | opencode-rs: $(stat -c%s "$RS") bytes"
  echo "--------------------------------------------------------------------------------"
} > "$OUT"

measure "cold-start --version" "$STOCK" --version
measure "cold-start --version" "$RS" --version
measure "--help" "$STOCK" --help
measure "--help" "$RS" --help
measure "run --help" "$STOCK" run --help
measure "run --help" "$RS" run --help
measure "models --help" "$STOCK" models --help
measure "models --help" "$RS" models --help

echo "--------------------------------------------------------------------------------" >> "$OUT"
cat "$OUT"
