# Vesting Contract (stellarlend-vesting)

This contract implements on-ledger vesting for tokens with a configurable cliff, linear vesting duration, and administrative revocation. Unvested tokens are clawed back to a designated treasury address upon revocation.

## On-Ledger Interface

- `cliff_seconds` prevents any claims until `now >= start + cliff_seconds`.
- Linear vesting after the cliff over `duration_seconds`.
- Multiple schedules can be recorded for the same `grantee`.
- `get_grants(grantee)` returns every schedule currently recorded for that grantee.
- `total_locked()` returns the aggregate locked supply tracked across all grants.
- `claim(grantee, now)` advances that grantee's schedules to `now`, decreases `total_locked()` by newly vested amounts, and transfers the newly claimable balance.
- `claimable_total(grantee, now)` view function returning the sum of claimable amounts across all grants without mutating state.
- `revoke(grantee)` callable only by admin; it advances that grantee's schedules to `now`, transfers any still-locked amount to the treasury sink, and removes the revoked schedules from the aggregate locked supply.

## Aggregate Claim Semantics

A grantee may hold multiple grants simultaneously. Both `claim` and `claimable_total` operate on all grants atomically:

- `claim` syncs each grant to `now`, sums claimable amounts across all non-revoked grants, and transfers the total in a single transaction.
- `claimable_total` computes the same sum without mutating state, allowing indexers to read aggregate claimable in one call.
- After `claim`, `total_locked` is decremented by the sum of newly vested amounts (not the claimed amount), preserving the invariant that locked tokens are those not yet vested.

`total_locked()` is maintained incrementally during `add_grant`, `claim`, and `revoke`; it is not recomputed by scanning all stored schedules.

See unit tests in `src/lib.rs`, `src/vesting_views_test.rs`, and `src/vesting_doc_example_test.rs` for expected behavior and examples.

For the full schedule math, cliff semantics, and revoke split with a worked example, see [`VESTING_MATH.md`](./VESTING_MATH.md).
