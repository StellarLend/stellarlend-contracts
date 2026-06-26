#!/usr/bin/env bash
set -euo pipefail

# Snapshot Drift Check
# Compares committed test_snapshots/ against freshly regenerated output.
# Fails with non-zero exit and prints diff if drift detected.
#
# Usage:
#   SNAPSHOT_CHECK=1 ./scripts/check-snapshots.sh
#
# Env:
#   SNAPSHOT_CHECK=1  Enable strict mode (fail on any diff)
#   SNAPSHOT_CHECK=0  Warning only, do not fail

SNAPSHOT_CHECK="${SNAPSHOT_CHECK:-1}"
CRATES=("hello-world" "lending")
DRIFT_FOUND=0
DIFF_FILES=()

echo "========================================"
echo "  Snapshot Drift Check"
echo "========================================"
echo "SNAPSHOT_CHECK=${SNAPSHOT_CHECK}"
echo ""

for crate in "${CRATES[@]}"; do
    CRATE_DIR="stellar-lend/contracts/${crate}"
    SNAPSHOT_DIR="${CRATE_DIR}/test_snapshots"

    echo "--- Checking ${crate} ---"

    if [ ! -d "${CRATE_DIR}" ]; then
        echo "WARNING: Crate directory not found: ${CRATE_DIR}"
        continue
    fi

    if [ ! -d "${SNAPSHOT_DIR}" ]; then
        echo "WARNING: Snapshot directory not found: ${SNAPSHOT_DIR}"
        echo "  Creating empty directory..."
        mkdir -p "${SNAPSHOT_DIR}"
    fi

    # Save current committed snapshots for comparison
    BACKUP_DIR=$(mktemp -d)
    cp -r "${SNAPSHOT_DIR}" "${BACKUP_DIR}/" 2>/dev/null || true

    # Clean and regenerate
    echo "  Regenerating snapshots..."
    rm -rf "${SNAPSHOT_DIR}"
    mkdir -p "${SNAPSHOT_DIR}"

    (
        cd "${CRATE_DIR}"
        cargo test --features testutils 2>&1 | tail -10
    )

    # Compare regenerated against committed
    echo "  Comparing snapshots..."
    if ! diff -ru "${BACKUP_DIR}/test_snapshots" "${SNAPSHOT_DIR}" > "/tmp/snapshot-diff-${crate}.txt" 2>&1; then
        echo "  ERROR: Snapshot drift detected in ${crate}!"
        echo ""
        cat "/tmp/snapshot-diff-${crate}.txt"
        echo ""
        DIFF_FILES+=("/tmp/snapshot-diff-${crate}.txt")
        DRIFT_FOUND=1
    else
        echo "  OK: No drift in ${crate}"
    fi

    # Restore committed snapshots (leave working tree clean)
    rm -rf "${SNAPSHOT_DIR}"
    cp -r "${BACKUP_DIR}/test_snapshots" "${SNAPSHOT_DIR}" 2>/dev/null || true
    rm -rf "${BACKUP_DIR}"
    echo ""
done

if [ "${DRIFT_FOUND}" -eq 1 ]; then
    echo "========================================"
    echo "  SNAPSHOT DRIFT DETECTED"
    echo "========================================"
    echo ""
    echo "The following snapshot files differ from committed baseline:"
    for f in "${DIFF_FILES[@]}"; do
        echo "  - ${f}"
    done
    echo ""
    echo "To fix (if drift is INTENTIONAL):"
    echo "  1. Run: ./scripts/regenerate-snapshots.sh"
    echo "  2. Review: git diff stellar-lend/contracts/*/test_snapshots/"
    echo "  3. Commit:  git add stellar-lend/contracts/*/test_snapshots/"
    echo "             git commit -m 'chore: regenerate test snapshots'"
    echo ""
    echo "If drift is NOT intentional: your code changes introduced"
    echo "unintended behavior. Fix the code, do NOT regenerate."
    echo ""
    echo "See docs/LOCAL_CI_RUNBOOK.md for full documentation."
    echo ""

    if [ "${SNAPSHOT_CHECK}" = "1" ]; then
        exit 1
    else
        echo "SNAPSHOT_CHECK=0: Warning only, exiting with success."
        exit 0
    fi
fi

echo "========================================"
echo "  All snapshots clean ✓"
echo "========================================"
exit 0