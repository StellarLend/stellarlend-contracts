#!/usr/bin/env bash
# =============================================================================
# scripts/check_wasm_sizes.sh – Validate release WASM artifact sizes against budgets
#
# Usage:
#   ./scripts/check_wasm_sizes.sh [--wasm-dir <dir>] [--budgets-file <path>]
#
# Default paths:
#   WASM directory: stellar-lend/target/wasm32v1-none/release (falls back to wasm32-unknown-unknown/release)
#   Budgets file:   scripts/wasm_budgets.json
#
# Exits with non-zero if any WASM exceeds its configured budget in bytes or
# if a required contract WASM is missing.
# If $GITHUB_STEP_SUMMARY is set, appends a Markdown summary table.
# =============================================================================
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(dirname "$SCRIPT_DIR")"

WASM_DIR=""
BUDGETS_FILE="$SCRIPT_DIR/wasm_budgets.json"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --wasm-dir)
      WASM_DIR="$2"
      shift 2
      ;;
    --budgets-file)
      BUDGETS_FILE="$2"
      shift 2
      ;;
    -h|--help)
      echo "Usage: $0 [--wasm-dir <dir>] [--budgets-file <path>]"
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$WASM_DIR" ]]; then
  if [[ -d "$REPO_ROOT/stellar-lend/target/wasm32v1-none/release" ]]; then
    WASM_DIR="$REPO_ROOT/stellar-lend/target/wasm32v1-none/release"
  elif [[ -d "$REPO_ROOT/stellar-lend/target/wasm32-unknown-unknown/release" ]]; then
    WASM_DIR="$REPO_ROOT/stellar-lend/target/wasm32-unknown-unknown/release"
  else
    WASM_DIR="$REPO_ROOT/stellar-lend/target/wasm32v1-none/release"
  fi
fi

if [[ ! -f "$BUDGETS_FILE" ]]; then
  echo "ERROR: Budgets file not found at $BUDGETS_FILE" >&2
  exit 1
fi

echo "======================================================================"
echo " Checking WASM Release Sizes Against Budgets"
echo " Directory: $WASM_DIR"
echo " Budgets:   $BUDGETS_FILE"
echo "======================================================================"

python3 - "$WASM_DIR" "$BUDGETS_FILE" <<'PY'
import json
import os
import sys

wasm_dir = sys.argv[1]
budgets_file = sys.argv[2]
step_summary = os.environ.get('GITHUB_STEP_SUMMARY', '')

with open(budgets_file, 'r', encoding='utf-8') as f:
    config = json.load(f)

budgets = config.get('budgets', {})
if not budgets:
    print('ERROR: No budgets defined in', budgets_file, file=sys.stderr)
    sys.exit(1)

table_rows = []
has_failures = False

print(f"{'Contract':<30} {'Actual (bytes)':<15} {'Budget (bytes)':<15} {'Margin (bytes)':<15} {'Status':<10}")
print('-' * 85)

for filename, budget in sorted(budgets.items()):
    filepath = os.path.join(wasm_dir, filename)
    if not os.path.isfile(filepath):
        print(f"{filename:<30} {'MISSING':<15} {budget:<15} {'N/A':<15} {'FAIL':<10}")
        table_rows.append((filename, 'MISSING', str(budget), 'N/A', '❌ MISSING'))
        has_failures = True
        continue
    
    actual_size = os.path.getsize(filepath)
    margin = budget - actual_size
    
    if actual_size <= budget:
        status = 'PASS'
        status_md = '✅ PASS'
    else:
        status = 'EXCEEDED'
        status_md = '❌ EXCEEDED'
        has_failures = True
        
    print(f'{filename:<30} {actual_size:<15} {budget:<15} {margin:<15} {status:<10}')
    table_rows.append((filename, str(actual_size), str(budget), str(margin), status_md))

print('=' * 85)

if step_summary:
    try:
        with open(step_summary, 'a', encoding='utf-8') as f:
            f.write('### 📦 WASM Release Size Budgets\n\n')
            f.write('| Contract | Actual Size (bytes) | Budget (bytes) | Margin (bytes) | Status |\n')
            f.write('| :--- | :--- | :--- | :--- | :--- |\n')
            for row in table_rows:
                f.write(f'| `{row[0]}` | {row[1]} | {row[2]} | {row[3]} | {row[4]} |\n')
            f.write('\n')
    except Exception as e:
        print(f'Warning: Failed to write to GITHUB_STEP_SUMMARY: {e}', file=sys.stderr)

if has_failures:
    print('ERROR: One or more WASM contracts exceeded their size budget or were missing.', file=sys.stderr)
    sys.exit(1)
else:
    print('SUCCESS: All contract WASM artifacts are within their size budget.')
    sys.exit(0)
PY
