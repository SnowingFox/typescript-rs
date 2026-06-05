#!/usr/bin/env bash
# T5-1: Compare Go vs Rust tsgo --noEmit on bundled lib.dom.d.ts.
#
# Usage (from repo root):
#   bash internal/compiler/lib_dom_noemit_parity.sh
#
# Requires: Go toolchain, release Rust tsgo (builds via cargo if missing).

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
LIB_DOM="${ROOT}/internal/bundled/libs/lib.dom.d.ts"
GO_OUT="$(mktemp)"
RUST_OUT="$(mktemp)"
NORM_GO="$(mktemp)"
NORM_RUST="$(mktemp)"
trap 'rm -f "$GO_OUT" "$RUST_OUT" "$NORM_GO" "$NORM_RUST"' EXIT

normalize() {
  sed -E \
    -e 's|bundled:///libs/|LIB/|g' \
    -e "s|${ROOT}/internal/bundled/libs/|LIB/|g" \
    -e 's|internal/bundled/libs/|LIB/|g' \
    | grep -E 'error TS[0-9]+' \
    | sort
}

echo "=== T5-1 lib.dom.d.ts --noEmit parity ==="
echo "Input: ${LIB_DOM}"
echo

echo "Running Go tsgo..."
(cd "$ROOT" && go run ./cmd/tsgo --noEmit "$LIB_DOM" 2>&1 || true) | tee "$GO_OUT" >/dev/null

RUST_BIN="${ROOT}/target/release/tsgo"
if [[ ! -x "$RUST_BIN" ]]; then
  echo "Building release tsgo..."
  (cd "$ROOT" && cargo build --release -p tsgo)
fi

echo "Running Rust tsgo..."
("$RUST_BIN" --noEmit "$LIB_DOM" 2>&1 || true) | tee "$RUST_OUT" >/dev/null

normalize <"$GO_OUT" >"$NORM_GO"
normalize <"$RUST_OUT" >"$NORM_RUST"

GO_TOTAL=$(wc -l <"$NORM_GO" | tr -d ' ')
RUST_TOTAL=$(wc -l <"$NORM_RUST" | tr -d ' ')
GO_UNIQUE=$(sort -u "$NORM_GO" | wc -l | tr -d ' ')
RUST_UNIQUE=$(sort -u "$NORM_RUST" | wc -l | tr -d ' ')
ONLY_GO=$(comm -23 "$NORM_GO" "$NORM_RUST" | wc -l | tr -d ' ')
ONLY_RUST=$(comm -13 "$NORM_GO" "$NORM_RUST" | wc -l | tr -d ' ')
BOTH=$(comm -12 "$NORM_GO" "$NORM_RUST" | wc -l | tr -d ' ')

echo
echo "=== Summary ==="
echo "Go diagnostics (total / unique):   ${GO_TOTAL} / ${GO_UNIQUE}"
echo "Rust diagnostics (total / unique): ${RUST_TOTAL} / ${RUST_UNIQUE}"
echo "Overlap (identical lines):           ${BOTH}"
echo "Only in Go:                        ${ONLY_GO}"
echo "Only in Rust:                      ${ONLY_RUST}"
echo

echo "=== Go by error code (total) ==="
grep -oE 'error TS[0-9]+' "$NORM_GO" | sort | uniq -c | sort -rn

echo
echo "=== Rust by error code (total) ==="
grep -oE 'error TS[0-9]+' "$NORM_RUST" | sort | uniq -c | sort -rn || echo "(none)"

echo
echo "=== Go by error code (unique) ==="
grep -oE 'error TS[0-9]+' <(sort -u "$NORM_GO") | sort | uniq -c | sort -rn

echo
echo "=== Rust by error code (unique) ==="
grep -oE 'error TS[0-9]+' <(sort -u "$NORM_RUST") | sort | uniq -c | sort -rn || echo "(none)"

if [[ "$BOTH" -eq 0 && "$GO_TOTAL" -gt 0 && "$RUST_TOTAL" -gt 0 ]]; then
  echo
  echo "RESULT: DIVERGED (zero line-level overlap after path normalization)"
  echo
  echo "Top Go-only samples:"
  comm -23 "$NORM_GO" "$NORM_RUST" | sort -u | head -5
  echo
  echo "Rust-only samples:"
  comm -13 "$NORM_GO" "$NORM_RUST" | sort -u
elif [[ "$ONLY_GO" -eq 0 && "$ONLY_RUST" -eq 0 ]]; then
  echo
  echo "RESULT: MATCH"
else
  echo
  echo "RESULT: PARTIAL OVERLAP"
fi
