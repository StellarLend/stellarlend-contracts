# Storage Key Audit - hello-world

## Status

This document was written ahead of the planned `DepositDataKey` / `LegacyDepositDataKey` split and describes an aspirational future state. The implementation is still at the stub stage:

- **`src/storage.rs`** contains only a `GuardianConfig` struct — no `DepositDataKey` or `LegacyDepositDataKey` enum exists yet.
- **`src/deposit.rs`** is a one-line module stub (`// Stub module`).
- **`src/lib.rs`** defines a simple two-variant `DataKey` enum (`Balance(Address)` / `Debt(Address)`) used by the demo entrypoints.

The rename and four-variant layout described below have **not been implemented**. This document should be updated once the deposit module is fleshed out.

## Planned Layout (not yet implemented)

Once implemented, the planned `DepositDataKey` variants are:

- `CollateralBalance(Address)`
- `PauseSwitches`
- `ProtocolAnalytics`
- `ProtocolReserve(Option<Address>)`

No overlapping discriminants or ambiguous tuple layouts are expected in scope.

## Risk Considerations

- Collision risk exists if enum variants are reused, reordered without care, or duplicated under the same storage-key identity assumptions.
- Mitigation: explicit variant separation and a regression test asserting distinct encodings for representative `DepositDataKey` variants.

## Scope

- `hello-world` contract only.
- Does not cover cross-contract key collisions.

## Exit criteria for this document

- [ ] `src/storage.rs` contains a `LegacyDepositDataKey` enum
- [ ] `src/deposit.rs` contains a `DepositDataKey` enum with the four variants above
- [ ] A regression test `test_deposit_data_key_unique_encoding` exists and passes
