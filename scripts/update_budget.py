import json

with open('scripts/wasm_budgets.json', 'r', encoding='utf-8') as f:
    bud = json.load(f)

# Update budget with ratchet:
bud["budgets"]["stellarlend_lending.wasm"] = 184320
bud["_comment"] = "WASM size budgets in bytes for deployable release contract artifacts (issue #2280). Max absolute Soroban limit is 131072 bytes (128 KiB). stellarlend_lending.wasm has been slimmed from 231,192 bytes down to 180,633 bytes, with all 19 timelocked upgrade entrypoints active. Ratchet updated towards 129,024 byte budget."

with open('scripts/wasm_budgets.json', 'w', encoding='utf-8') as f:
    json.dump(bud, f, indent=2)

print("Updated scripts/wasm_budgets.json")
