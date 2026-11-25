#!/usr/bin/env python3
"""
render_deployments.py
Generate (or verify) DEPLOYMENTS.md from DEPLOYMENTS.json.

Usage:
  python3 scripts/render_deployments.py            # write DEPLOYMENTS.md
  python3 scripts/render_deployments.py --check    # exit 1 if MD differs
"""

import json
import re
import sys
from pathlib import Path

REPO_ROOT = Path(__file__).parent.parent
JSON_PATH = REPO_ROOT / "DEPLOYMENTS.json"
MD_PATH   = REPO_ROOT / "DEPLOYMENTS.md"

CONTRACT_ID_RE = re.compile(r"^C[A-Z2-7]{55}$")


def load_and_validate(path: Path) -> dict:
    with open(path) as f:
        data = json.load(f)

    contracts = data.get("contracts", {})
    errors = []
    for name, info in contracts.items():
        cid = info.get("id", "")
        if not CONTRACT_ID_RE.match(cid):
            errors.append(f"  {name}: id '{cid}' does not match ^C[A-Z2-7]{{55}}$")
        if "explorer" not in info:
            errors.append(f"  {name}: missing 'explorer' field")
        if "status" not in info:
            errors.append(f"  {name}: missing 'status' field")

    if errors:
        print("Schema validation failed:\n" + "\n".join(errors), file=sys.stderr)
        sys.exit(1)

    return data


def render(data: dict) -> str:
    network  = data.get("network", "unknown")
    contracts = data.get("contracts", {})
    admin    = data.get("admin", "")
    demo     = data.get("demo_user", "")

    rows = []
    for name, info in contracts.items():
        cid      = info["id"]
        explorer = info["explorer"]
        status   = info["status"]
        wasm     = info.get("wasm_hash", "")
        label    = f"{name} ({status})"
        wasm_col = f"`{wasm}`" if wasm else ""
        rows.append(f"| {label} | `{cid}` | [stellar.expert]({explorer}) | {wasm_col} |")

    table_rows = "\n".join(rows)

    md = f"""# Testnet Deployments

Network: Stellar **{network}** (Test SDF Network ; September 2015)

| Contract | ID | Explorer | WASM hash |
|---|---|---|---|
{table_rows}

Admin / deployer: `{admin}`  
Demo user: `{demo}`

v1 is deprecated: its per-ledger borrow-rate cache made the transaction footprint depend on the closing ledger, so `borrow` simulated fine but trapped on-chain. The current deployment removes that cache.

## Verified on-chain flow (current contract)

initialize -> deposit 1,000,000 -> borrow 100,000 -> repay 50,000 -> withdraw 100,000 -> `get_position` returned
`{{"collateral":"900000","debt":"50000","health_factor":"144000"}}`.

Transactions: [initialize](https://stellar.expert/explorer/testnet/tx/c5a0e6d2872b4a4d1804c39dcf41eb5cbb06cfc5b24e1b27f0b645663a1b9cf9),
[deposit](https://stellar.expert/explorer/testnet/tx/94da3c85b4db06839e8216aeb20e62c98011145c1567f2e9d38228e46d2a0f46),
[borrow](https://stellar.expert/explorer/testnet/tx/edcb603278567f4433d43b1ff12b950d0cce2f39665896b639f3f882191f8fd6),
[repay](https://stellar.expert/explorer/testnet/tx/8c93241a604cdee05d40f51ba1efe77c773dab5546a04259082c574b840b48be),
[withdraw](https://stellar.expert/explorer/testnet/tx/46808a869cecffc60181acffbba876e77f468ab3c78cc286b64b54b021461853).
"""
    return md


def main():
    check_mode = "--check" in sys.argv
    data = load_and_validate(JSON_PATH)
    generated = render(data)

    if check_mode:
        if not MD_PATH.exists():
            print(f"FAIL: {MD_PATH} does not exist. Run without --check to generate it.", file=sys.stderr)
            sys.exit(1)
        current = MD_PATH.read_text()
        if current != generated:
            print("FAIL: DEPLOYMENTS.md is out of date. Run scripts/render_deployments.py to regenerate.", file=sys.stderr)
            sys.exit(1)
        print("OK: DEPLOYMENTS.md is up to date.")
    else:
        MD_PATH.write_text(generated)
        print(f"Written: {MD_PATH}")


if __name__ == "__main__":
    main()
