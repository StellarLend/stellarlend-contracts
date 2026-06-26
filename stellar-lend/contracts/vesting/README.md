# Vesting Contract (stellarlend-vesting)

This crate implements a simple vesting contract with a configurable cliff and an admin `revoke` entrypoint that claws back unvested tokens to a treasury address.

Key behavior:

- `cliff_seconds` prevents any claims until `now >= start + cliff_seconds`.
- Linear vesting after the cliff over `duration_seconds`.
- `revoke(grantee)` callable only by admin; unvested tokens are transferred to the treasury sink.

See unit tests in `src/lib.rs` for expected behavior and examples.
