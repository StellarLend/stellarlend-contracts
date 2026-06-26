# Vesting Contract (stellarlend-vesting)

This crate implements a simple vesting contract with a configurable cliff and an admin `revoke` entrypoint that claws back unvested tokens to a treasury address.

Key behavior:

- `cliff_seconds` prevents any claims until `now >= start + cliff_seconds`.
- Linear vesting after the cliff over `duration_seconds`.
- Multiple schedules can be recorded for the same `grantee`.
- `get_grants(grantee)` returns every schedule currently recorded for that grantee.
- `total_locked()` returns the aggregate locked supply tracked across all grants.
- `claim(grantee, now)` advances that grantee's schedules to `now`, decreases `total_locked()` by newly vested amounts, and transfers the newly claimable balance.
- `revoke(grantee)` callable only by admin; it advances that grantee's schedules to `now`, transfers any still-locked amount to the treasury sink, and removes the revoked schedules from the aggregate locked supply.

`total_locked()` is maintained incrementally during `add_grant`, `claim`, and `revoke`; it is not recomputed by scanning all stored schedules.

See unit tests in `src/lib.rs` and `src/vesting_views_test.rs` for expected behavior and examples.
