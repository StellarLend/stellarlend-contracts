import os
import re

files_to_check = [
    "stellar-lend/contracts/lending/src/lib.rs",
    "stellar-lend/contracts/lending/src/invariants.rs",
    "stellar-lend/contracts/lending/src/debt.rs",
    "stellar-lend/contracts/lending/src/cross_asset.rs",
    "stellar-lend/contracts/lending/src/events.rs",
    "stellar-lend/contracts/lending/src/math.rs",
    "stellar-lend/contracts/lending/src/rate_model.rs",
    "stellar-lend/contracts/lending/src/rounding_strategy.rs",
    "stellar-lend/contracts/lending/src/upgrade.rs",
    "stellar-lend/contracts/lending/src/audit_log.rs",
]

for filepath in files_to_check:
    if not os.path.exists(filepath):
        continue
    with open(filepath, "r", encoding="utf-8") as f:
        lines = f.readlines()
    print(f"\n--- {filepath} ---")
    in_test = False
    for i, line in enumerate(lines, 1):
        stripped = line.strip()
        # Only consider test if it's #[cfg(test)] mod tests {
        if stripped.startswith("//"):
            continue
        if any(k in stripped for k in [".expect(", "panic!(", "assert!(", "assert_eq!("]):
            print(f"L{i}: {stripped}")
