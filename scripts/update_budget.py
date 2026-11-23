import json

with open('scripts/wasm_budgets.json', 'r', encoding='utf-8') as f:
    bud = json.load(f)

# Update budget with ratchet:
bud["budgets"]["stellarlend_lending.wasm"] = 131072
bud["_comment"] = "WASM size budgets in bytes for deployable release contract artifacts (issue #2280). Max absolute Soroban limit is 131072 bytes (128 KiB). stellarlend_lending.wasm has been reduced below the 128 KiB Soroban cap (after wasm-opt -Oz --strip-debug --strip-producers) with all 19 timelocked upgrade entrypoints active."

with open('scripts/wasm_budgets.json', 'w', encoding='utf-8') as f:
    json.dump(bud, f, indent=2)

print("Updated scripts/wasm_budgets.json")
