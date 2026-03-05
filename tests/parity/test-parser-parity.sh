#!/usr/bin/env bash
# tests/parity/test-parser-parity.sh — verify Rust parser output matches bash
#
# Requires: jq, bash session parsers (in shepard-obs-stack)
# Usage: ./tests/parity/test-parser-parity.sh

set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
OBS_STACK="${REPO_DIR}/../shepard-obs-stack"

# Paths
RUST_BIN="${REPO_DIR}/target/release/shepard-hook"
FIXTURES_DIR="${REPO_DIR}/tests/fixtures"
BASH_PARSERS=(
  "claude:${OBS_STACK}/hooks/lib/session-parser.sh:${FIXTURES_DIR}/claude-session.jsonl"
  "codex:${OBS_STACK}/hooks/lib/codex-session-parser.sh:${FIXTURES_DIR}/codex-session.jsonl"
  "gemini:${OBS_STACK}/hooks/lib/gemini-session-parser.sh:${FIXTURES_DIR}/gemini-session.json"
)

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'
PASS=0
FAIL=0

# Build release binary if missing
if [[ ! -x "$RUST_BIN" ]]; then
  echo "Building release binary..."
  (cd "$REPO_DIR" && cargo build --release 2>/dev/null) || {
    echo -e "${RED}FAIL${NC}: cargo build --release failed"
    exit 1
  }
fi

compare_spans() {
  local provider="$1"
  local bash_script="$2"
  local fixture="$3"

  if [[ ! -f "$bash_script" ]]; then
    echo -e "${RED}SKIP${NC}: $provider — bash parser not found: $bash_script"
    return
  fi

  if [[ ! -f "$fixture" ]]; then
    echo -e "${RED}SKIP${NC}: $provider — fixture not found: $fixture"
    return
  fi

  # Run bash parser
  local bash_output
  bash_output=$(bash "$bash_script" "$fixture" 2>/dev/null)

  # Run Rust parser
  local rust_output
  rust_output=$("$RUST_BIN" parse-session "$provider" "$fixture" 2>/dev/null)

  # Compare span counts
  local bash_count rust_count
  bash_count=$(echo "$bash_output" | grep -c '^{')
  rust_count=$(echo "$rust_output" | grep -c '^{')

  if [[ "$bash_count" -ne "$rust_count" ]]; then
    echo -e "${RED}FAIL${NC}: $provider — span count mismatch: bash=$bash_count rust=$rust_count"
    FAIL=$((FAIL + 1))
    return
  fi

  echo "  $provider: span count matches ($rust_count)"

  # Compare each span's key fields
  local i=0
  while IFS= read -r bash_line; do
    local rust_line
    rust_line=$(echo "$rust_output" | sed -n "$((i + 1))p")

    # Compare trace_id
    local bash_tid rust_tid
    bash_tid=$(echo "$bash_line" | jq -r '.trace_id')
    rust_tid=$(echo "$rust_line" | jq -r '.trace_id')
    if [[ "$bash_tid" != "$rust_tid" ]]; then
      echo -e "${RED}FAIL${NC}: $provider span[$i] trace_id mismatch: bash=$bash_tid rust=$rust_tid"
      FAIL=$((FAIL + 1))
      return
    fi

    # Compare span_id
    local bash_sid rust_sid
    bash_sid=$(echo "$bash_line" | jq -r '.span_id')
    rust_sid=$(echo "$rust_line" | jq -r '.span_id')
    if [[ "$bash_sid" != "$rust_sid" ]]; then
      echo -e "${RED}FAIL${NC}: $provider span[$i] span_id mismatch: bash=$bash_sid rust=$rust_sid"
      FAIL=$((FAIL + 1))
      return
    fi

    # Compare name
    local bash_name rust_name
    bash_name=$(echo "$bash_line" | jq -r '.name')
    rust_name=$(echo "$rust_line" | jq -r '.name')
    if [[ "$bash_name" != "$rust_name" ]]; then
      echo -e "${RED}FAIL${NC}: $provider span[$i] name mismatch: bash=$bash_name rust=$rust_name"
      FAIL=$((FAIL + 1))
      return
    fi

    i=$((i + 1))
  done <<< "$bash_output"

  echo -e "  ${GREEN}PASS${NC}: $provider — all $rust_count spans match (trace_id, span_id, name)"
  PASS=$((PASS + 1))
}

echo "=== Parser Parity Tests ==="
echo ""

for entry in "${BASH_PARSERS[@]}"; do
  IFS=':' read -r provider script fixture <<< "$entry"
  compare_spans "$provider" "$script" "$fixture"
done

echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="

[[ "$FAIL" -eq 0 ]]
