# Vesting Contract (stellarlend-vesting)

This contract implements on-ledger vesting for tokens with a configurable cliff, linear vesting duration, and administrative revocation. Unvested tokens are clawed back to a designated treasury address upon revocation.

## On-Ledger Interface

- `cliff_seconds` prevents any claims until `now >= start + cliff_seconds`.
- Linear vesting after the cliff over `duration_seconds`.
- Multiple schedules can be recorded for the same `grantee`.
- `get_grants(grantee)` returns every schedule currently recorded for that grantee.
- `total_locked()` returns the aggregate locked supply tracked across all grants.
- `claim(grantee, now)` advances that grantee's schedules to `now`, decreases `total_locked()` by newly vested amounts, and transfers the newly claimable balance.
- `revoke(grantee)` callable only by admin; it advances that grantee's schedules to `now`, transfers any still-locked amount to the treasury sink, and removes the revoked schedules from the aggregate locked supply.

`total_locked()` is maintained incrementally during `add_grant`, `claim`, and `revoke`; it is not recomputed by scanning all stored schedules.

See unit tests in `src/lib.rs`, `src/vesting_views_test.rs`, and `src/vesting_doc_example_test.rs` for expected behavior and examples.

For the full schedule math, cliff semantics, and revoke split with a worked example, see [`VESTING_MATH.md`](./VESTING_MATH.md).
