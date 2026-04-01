# Storage Key Audit - hello-world

## Summary

- Found duplicate enum name `DepositDataKey` in `src/storage.rs` and `src/deposit.rs`.
- Renamed `src/storage.rs` enum to `LegacyDepositDataKey` to avoid key-type naming collision and accidental encoding confusion.

## DepositDataKey Layout

Each variant uses a unique discriminant and structured encoding via Soroban XDR.

Example variants:

- `CollateralBalance(Address)`
- `PauseSwitches`
- `ProtocolAnalytics`
- `ProtocolReserve(Option<Address>)`

No overlapping discriminants or ambiguous tuple layouts are introduced in this audit scope.

## Risk Considerations

- Collision risk exists if enum variants are reused, reordered without care, or duplicated under the same storage-key identity assumptions.
- Mitigated by explicit variant separation and a regression test asserting distinct encodings for representative `DepositDataKey` variants.

## Scope

- `hello-world` contract only.
- Does not cover cross-contract key collisions.

## Trust Boundaries

- Admin and guardian authority model is unchanged by this patch.
- Token transfer flows and authorization paths are unchanged by this storage-key audit patch.

## Testing

- Added regression test `test_deposit_data_key_unique_encoding` in `src/tests/storage_test.rs`.
- Intended command for isolated verification:
  - `cargo test test_deposit_data_key_unique_encoding`
- Full `cargo test` currently fails in this legacy crate due to unrelated pre-existing compile issues.
