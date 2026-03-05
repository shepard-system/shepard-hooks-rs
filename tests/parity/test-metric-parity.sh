#!/usr/bin/env bash
# tests/parity/test-metric-parity.sh — verify Rust OTLP metric JSON matches bash
#
# Starts a netcat listener, captures POST body from both bash and Rust,
# compares the OTLP JSON structure (ignoring timestamps).
#
# Requires: jq, netcat (nc), bash metrics.sh (in shepard-obs-stack)
# Usage: ./tests/parity/test-metric-parity.sh

set -u

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"
OBS_STACK="${REPO_DIR}/../shepard-obs-stack"

RUST_BIN="${REPO_DIR}/target/release/shepard-hook"
BASH_METRICS="${OBS_STACK}/hooks/lib/metrics.sh"

RED='\033[0;31m'
GREEN='\033[0;32m'
NC_COLOR='\033[0m'
PASS=0
FAIL=0

# Build release binary if missing
if [[ ! -x "$RUST_BIN" ]]; then
  echo "Building release binary..."
  (cd "$REPO_DIR" && cargo build --release 2>/dev/null) || {
    echo -e "${RED}FAIL${NC_COLOR}: cargo build --release failed"
    exit 1
  }
fi

if [[ ! -f "$BASH_METRICS" ]]; then
  echo -e "${RED}SKIP${NC_COLOR}: bash metrics.sh not found: $BASH_METRICS"
  exit 0
fi

# Find a free port
find_free_port() {
  python3 -c 'import socket; s=socket.socket(); s.bind(("",0)); print(s.getsockname()[1]); s.close()'
}

# Capture HTTP POST body using netcat
# Returns the body (everything after the blank line)
capture_post_body() {
  local port="$1"
  local timeout_sec=3

  # Start nc listener, capture full request, extract body
  local raw
  raw=$(timeout "$timeout_sec" nc -l "$port" 2>/dev/null || true)

  # Extract body: everything after the first blank line
  echo "$raw" | sed -n '/^$/,$p' | tail -n +2
}

echo "=== Metric Parity Tests ==="
echo ""

# --- Test 1: Metric structure comparison ---

PORT=$(find_free_port)
TMPDIR_PARITY=$(mktemp -d)
BASH_BODY="${TMPDIR_PARITY}/bash_body.json"
RUST_BODY="${TMPDIR_PARITY}/rust_body.json"

echo "Using port $PORT for capture"

# Capture bash metric
capture_post_body "$PORT" > "$BASH_BODY" &
CAPTURE_PID=$!
sleep 0.3

export OTEL_HTTP_URL="http://127.0.0.1:${PORT}"
(
  source "$BASH_METRICS"
  emit_counter "tool_calls" 1 '{"source":"claude-code","tool":"Read"}'
  wait
)
wait "$CAPTURE_PID" 2>/dev/null || true

# Capture Rust metric
PORT2=$(find_free_port)
capture_post_body "$PORT2" > "$RUST_BODY" &
CAPTURE_PID=$!
sleep 0.3

"$RUST_BIN" emit-metric "tool_calls" "1" '{"source":"claude-code","tool":"Read"}' 2>/dev/null &
RUST_PID=$!
export OTEL_HTTP_URL="http://127.0.0.1:${PORT2}"
"$RUST_BIN" emit-metric "tool_calls" "1" '{"source":"claude-code","tool":"Read"}' 2>/dev/null || true
wait "$CAPTURE_PID" 2>/dev/null || true

# Compare structures (strip timestamps for comparison)
normalize_metric() {
  jq -c '
    .resourceMetrics[0] |
    {
      service_name: .resource.attributes[0].value.stringValue,
      scope_name: .scopeMetrics[0].scope.name,
      metric_name: .scopeMetrics[0].metrics[0].name,
      value: .scopeMetrics[0].metrics[0].sum.dataPoints[0].asDouble,
      temporality: .scopeMetrics[0].metrics[0].sum.aggregationTemporality,
      monotonic: .scopeMetrics[0].metrics[0].sum.isMonotonic,
      attr_count: (.scopeMetrics[0].metrics[0].sum.dataPoints[0].attributes | length)
    }
  ' 2>/dev/null
}

BASH_NORM=$(normalize_metric < "$BASH_BODY")
RUST_NORM=$(normalize_metric < "$RUST_BODY")

if [[ -z "$BASH_NORM" || -z "$RUST_NORM" ]]; then
  echo -e "${RED}FAIL${NC_COLOR}: could not capture/parse metric payloads"
  echo "  bash body: $(cat "$BASH_BODY")"
  echo "  rust body: $(cat "$RUST_BODY")"
  FAIL=$((FAIL + 1))
elif [[ "$BASH_NORM" == "$RUST_NORM" ]]; then
  echo -e "  ${GREEN}PASS${NC_COLOR}: metric structure matches"
  echo "  structure: $RUST_NORM"
  PASS=$((PASS + 1))
else
  echo -e "${RED}FAIL${NC_COLOR}: metric structure mismatch"
  echo "  bash: $BASH_NORM"
  echo "  rust: $RUST_NORM"
  FAIL=$((FAIL + 1))
fi

# Cleanup
rm -rf "$TMPDIR_PARITY"

echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="

[[ "$FAIL" -eq 0 ]]
