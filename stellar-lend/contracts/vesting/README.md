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

## Admin And Treasury Rotation Runbook

Vesting schedules can run for years, so the admin key and treasury multisig
must be rotatable without redeploying the contract. The token address remains
immutable.

### Roles

- **Admin** – may add/accelerate/transfer/revoke grants, pause/resume, propose
  a successor, cancel a proposal, and change the treasury.
- **Pending admin** – the proposed successor. It has no powers until it accepts.
- **Treasury** – receives unvested tokens when `revoke` is called. It does not
  authorize anything.

`MAX_ADMIN_PROPOSAL_TTL` is 30 days (2,592,000 seconds).

### Rotate the admin (two-step, recommended)

1. From the current admin key, propose the successor. `ttl_secs` must be
   non-zero; values above the 30-day cap are clamped. Re-proposing replaces
   any earlier proposal.

   ```sh
   stellar contract invoke --id <VESTING_ID> --source <CURRENT_ADMIN> -- \
     propose_admin --new_admin <NEW_ADMIN> --ttl_secs 86400
   ```

   This emits `admin_proposed` with the current admin, the pending admin, and
   the expiry timestamp. Verify it with `get_pending_admin`.

2. From the **new** admin key, accept before the proposal expires.

   ```sh
   stellar contract invoke --id <VESTING_ID> --source <NEW_ADMIN> -- \
     accept_admin
   ```

   This emits `admin_changed` with the old and new admin and clears the
   pending slot. `get_admin` returns the new admin immediately; the old admin
   is unauthorized for every admin entrypoint.

3. If the successor was mistyped or the handover is no longer wanted, the
   current admin cancels it before acceptance:

   ```sh
   stellar contract invoke --id <VESTING_ID> --source <CURRENT_ADMIN> -- \
     cancel_admin_proposal
   ```

   This emits `admin_proposal_cancelled`. A subsequent `accept_admin` fails
   with `NoPendingAdmin`.

An expired proposal cannot be accepted (`AdminProposalExpired`); the current
admin can cancel it or simply propose a replacement.

### Rotate the treasury

The admin can point future clawbacks at a new treasury at any time:

```sh
stellar contract invoke --id <VESTING_ID> --source <ADMIN> -- \
  set_treasury --caller <ADMIN> --new_treasury <NEW_MULTISIG>
```

This emits `treasury_changed` with the old and new addresses. `revoke` reads
this value at call time, so the next revocation sends unvested tokens to the
new treasury. The vesting contract's own address is rejected
(`InvalidTreasury`) to keep clawbacks from looping back into the vault.

### Views

- `get_admin() -> Option<Address>`
- `get_pending_admin() -> Option<(Address, u64)>` – proposed address and expiry
  (ledger timestamp).
- `get_treasury() -> Option<Address>`

### Failure modes

| Error | Meaning |
| --- | --- |
| `NoPendingAdmin` | `accept_admin`/`cancel_admin_proposal` called with no proposal |
| `AdminProposalExpired` | acceptance attempted at or after the expiry timestamp |
| `InvalidAdminProposal` | proposed the current admin, or `ttl_secs == 0` |
| `InvalidTreasury` | treasury set to the vesting contract itself |
| `Unauthorized` | caller is not the stored admin |

Acceptance is signed by the pending admin, so a mistyped or compromised
proposal cannot take effect unless that address also signs. Keep the pending
admin's key offline until the handover window.
