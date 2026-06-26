#!/usr/bin/env bash
set -euo pipefail

# Regenerate All Test Snapshots
# Use this when snapshot drift is intentional (e.g., contract logic change).
# After running, review the diff and commit the updated snapshots.
#
# Usage:
#   ./scripts/regenerate-snapshots.sh
#   git diff stellar-lend/contracts/*/test_snapshots/
#   git add stellar-lend/contracts/*/test_snapshots/
#   git commit -m "chore: regenerate test snapshots"

CRATES=("hello-world" "lending")

echo "========================================"
echo "  Regenerating Test Snapshots"
echo "========================================"
echo ""

for crate in "${CRATES[@]}"; do
    CRATE_DIR="stellar-lend/contracts/${crate}"
    SNAPSHOT_DIR="${CRATE_DIR}/test_snapshots"

    echo "--- ${crate} ---"

    if [ ! -d "${CRATE_DIR}" ]; then
        echo "WARNING: Crate directory not found: ${CRATE_DIR}"
        continue
    fi

    # Clean existing snapshots
    echo "  Cleaning old snapshots..."
    rm -rf "${SNAPSHOT_DIR}"
    mkdir -p "${SNAPSHOT_DIR}"

    # Run tests to regenerate
    echo "  Running tests..."
    (
        cd "${CRATE_DIR}"
        cargo test --features testutils 2>&1 | tail -10
    )

    echo "  Done: ${crate}"
    echo ""
done

echo "========================================"
echo "  Regeneration Complete"
echo "========================================"
echo ""
echo "Next steps:"
echo "  1. Review changes:"
echo "     git diff stellar-lend/contracts/*/test_snapshots/"
echo ""
echo "  2. Ensure every change matches your intent"
echo "     (changed contract logic → expected snapshot change)"
echo ""
echo "  3. Commit:"
echo "     git add stellar-lend/contracts/*/test_snapshots/"
echo "     git commit -m 'chore: regenerate test snapshots'"
echo ""