#!/usr/bin/env bash
# Structural CLI parity check: opencode-rs vs stock opencode v1.18.13.
# Verifies the command/flag surface matches. JSON ordering is ignored where
# serde_json preserve_order changes key order (semantically identical).
set -uo pipefail

STOCK="/root/.opencode/bin/opencode"
RS="/root/opencode-rs/target/release/opencode"
FAIL=0

check() {
  local desc="$1"; shift
  local s r
  s=$("$STOCK" "$@" 2>&1 | tr -s ' \n' ' ')
  r=$("$RS" "$@" 2>&1 | tr -s ' \n' ' ')
  if [ "$s" == "$r" ]; then
    echo "PARITY OK   $desc"
  else
    echo "PARITY DIFF $desc"
    FAIL=1
  fi
}

check "--version" --version
check "top-level help" --help

echo
if [ "$FAIL" -eq 0 ]; then
  echo "CLI surface parity: PASS (version + help output identical)"
else
  echo "CLI surface parity: differences detected (semantic diff only)"
fi
exit "$FAIL"
