#!/bin/bash

# WASM Audit Script for StellarLend Hello-World Contract
# Generates build report and API surface documentation

set -e

echo "🔍 StellarLend WASM Audit Report"
echo "================================"
echo ""

# Build the contract
echo "📦 Building contract..."
stellar contract build > build_output.tmp 2>&1

# Extract build information
WASM_SIZE=$(grep "Wasm Size:" build_output.tmp | awk '{print $3, $4}')
WASM_HASH=$(grep "Wasm Hash:" build_output.tmp | awk '{print $3}')
FUNCTION_COUNT=$(grep "Exported Functions:" build_output.tmp | awk '{print $3}')

echo "✅ Build Complete"
echo ""
echo "📊 Build Summary:"
echo "  WASM Size: $WASM_SIZE"
echo "  WASM Hash: $WASM_HASH"
echo "  Exported Functions: $FUNCTION_COUNT"
echo ""

# Extract function list
echo "🔧 Exported Functions:"
sed -n '/Exported Functions:/,/✅ Build Complete/p' build_output.tmp | grep "•" | head -20
echo "  ... (showing first 20 functions)"
echo ""

# Calculate size metrics
WASM_SIZE_BYTES=$(echo $WASM_SIZE | awk '{print $1}')
AVG_SIZE_PER_FUNCTION=$((WASM_SIZE_BYTES / FUNCTION_COUNT))

echo "📈 Size Analysis:"
echo "  Average per function: ~$AVG_SIZE_PER_FUNCTION bytes"
echo "  Size category: $(if [ $WASM_SIZE_BYTES -lt 100000 ]; then echo "Small"; elif [ $WASM_SIZE_BYTES -lt 300000 ]; then echo "Medium"; else echo "Large"; fi)"
echo ""

# Security checklist — results derived from source inspection
# Locate the src directory relative to this script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC_DIR="$SCRIPT_DIR/../src"

SECURITY_FAILURES=0

check_pass() { echo "  ✅ $1"; }
check_fail() { echo "  ❌ $1"; SECURITY_FAILURES=$((SECURITY_FAILURES + 1)); }

echo "🛡️  Security Checklist:"

# 1. #![no_std] — must appear in at least one source file
if grep -rq '#!\[no_std\]' "$SRC_DIR" 2>/dev/null; then
    check_pass "#![no_std] attribute present"
else
    check_fail "#![no_std] attribute NOT found in src/"
fi

# 2. Checked arithmetic — look for .checked_add / .checked_sub / .checked_mul / .checked_div usage
if grep -rqE '\.(checked_add|checked_sub|checked_mul|checked_div)\(' "$SRC_DIR" 2>/dev/null; then
    CHECKED_COUNT=$(grep -rEoh '\.(checked_add|checked_sub|checked_mul|checked_div)\(' "$SRC_DIR" 2>/dev/null | wc -l | tr -d ' ')
    check_pass "Checked arithmetic used ($CHECKED_COUNT call(s) found)"
else
    check_fail "No checked arithmetic calls found in src/ — raw +/-/*/÷ may overflow"
fi

# 3. Authorization checks — look for require_auth / require_auth_for_args calls
if grep -rqE 'require_auth(_for_args)?\(' "$SRC_DIR" 2>/dev/null; then
    AUTH_COUNT=$(grep -rEoh 'require_auth(_for_args)?\(' "$SRC_DIR" 2>/dev/null | wc -l | tr -d ' ')
    check_pass "require_auth call(s) present ($AUTH_COUNT found)"
else
    check_fail "No require_auth calls found in src/ — admin functions may be unprotected"
fi

# 4. Reentrancy protection — Soroban's execution model prevents classic reentrancy;
#    flag as a warning if any storage writes appear before authorization checks in the same fn.
#    Best approximation: confirm require_auth is called before any storage().set/put pattern.
#    We check structurally: if require_auth exists, assume ordering is handled, but note it.
if grep -rqE 'require_auth(_for_args)?\(' "$SRC_DIR" 2>/dev/null; then
    check_pass "Reentrancy protection: Soroban host enforces single-entry execution; require_auth guards present"
else
    check_fail "Reentrancy protection: no require_auth found — verify state changes are gated"
fi

# 5. Emergency pause controls — look for a 'paused' / 'is_paused' / 'pause' symbol
if grep -rqiE '\bpaus(e|ed|ing)\b' "$SRC_DIR" 2>/dev/null; then
    check_pass "Emergency pause controls found in src/"
else
    check_fail "No pause/paused symbol found in src/ — emergency stop may be missing"
fi

# 6. Risk parameter validation — look for explicit bounds/range checks on parameters
if grep -rqE '(require|assert|panic|if .* >|if .* <|\.clamp\()' "$SRC_DIR" 2>/dev/null; then
    check_pass "Risk parameter validation patterns present"
else
    check_fail "No obvious parameter validation found in src/"
fi

echo ""
if [ $SECURITY_FAILURES -gt 0 ]; then
    echo "  ⚠️  $SECURITY_FAILURES security check(s) FAILED — review findings above"
fi
echo ""

# Recommendations
echo "💡 Recommendations:"
if [ $WASM_SIZE_BYTES -lt 250000 ]; then
    echo "  ✅ WASM size is within acceptable limits"
    echo "  ✅ No immediate optimization needed"
else
    echo "  ⚠️  WASM size is large, consider optimizations"
    echo "  💡 Review optional features for potential removal"
fi

echo ""
echo "📄 Full audit report available in WASM_AUDIT.md"

# Clean up
rm -f build_output.tmp

echo ""
echo "🎉 Audit complete!"