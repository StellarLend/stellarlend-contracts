# Guardian Recovery

## Overview

StellarLend supports guardian-based recovery of the protocol's
governance admin. If the current admin key is lost or compromised, a
configured set of guardians can collectively vote to rotate admin
control to a new address — without requiring the old admin's
cooperation.

This is implemented in
[`governance.rs`](../stellar-lend/contracts/hello-world/src/governance.rs)
as part of the broader governance module. Guardian membership and
threshold configuration are also documented in
[`RECOVERY_GUARDIANS.md`](../stellar-lend/contracts/hello-world/RECOVERY_GUARDIANS.md).

> **Note:** `recovery.rs` exists in this crate but is currently empty —
> the recovery logic described here lives in `governance.rs`, not
> `recovery.rs`.

## Storage Model

- **`GuardianConfig`** — the set of guardian addresses and the
  approval `threshold` (a `u32`) required to execute a recovery.
  Managed via `add_guardian`, `remove_guardian`, and
  `set_guardian_threshold` (all admin-only).
- **`RecoveryRequest`** — the single in-flight recovery request, with
  fields:
  - `old_admin` — the admin address being replaced
  - `new_admin` — the proposed replacement admin
  - `initiated_at` — ledger timestamp the request was started
  - `approval_count` — number of approvals recorded so far
- **`RecoveryApprovals`** — a `Vec<Address>` of guardians who have
  approved the current request.

Only one recovery request can be pending at a time; starting a new one
overwrites the previous request and approval list.

## Recovery Lifecycle

1. **`start_recovery(initiator, old_admin, new_admin)`**
   Any guardian can initiate a recovery. Creates a new
   `RecoveryRequest` and automatically records the initiator as the
   first approval.

2. **`approve_recovery(approver)`**
   Any other guardian can add their approval to the pending request.
   Duplicate approvals from the same guardian are ignored.

3. **`execute_recovery(executor)`**
   Callable by anyone once approvals reach the configured guardian
   `threshold`. Sets the governance config's `admin` field to
   `new_admin` and clears both the `RecoveryRequest` and
   `RecoveryApprovals` storage entries.

## Read Functions

- **`get_recovery_request`** — returns the pending `RecoveryRequest`,
  if any.
- **`get_recovery_approvals`** — returns the list of guardian
  addresses that have approved the pending request.

## Known Gap vs. `RECOVERY_GUARDIANS.md`

`RECOVERY_GUARDIANS.md` documents several invariants that the current
`governance.rs` implementation does **not** yet enforce:

- `old_admin` is not verified to be the current admin before recovery
  starts.
- `new_admin` is not checked against the existing admin set.
- There's no expiry mechanism for stale recovery requests.
- Guardian removal does not clamp the threshold if it would exceed the
  remaining guardian count.

These should be tracked as follow-up hardening work if not already
covered by a separate issue.