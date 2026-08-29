# Approval Domain Binding

This document describes how `approve_proposal` in
[`src/lib.rs`](src/lib.rs) binds each approval to the exact proposal it was
intended for, closing the cross-proposal approval-reuse vector discussed in
issue [#1278](https://github.com/StellarLend/stellarlend-contracts/issues/1278).

## Threat model

An approval authorizes a signer to count toward quorum for a **specific**
proposal. If the signed authorization did not uniquely scope the
`proposal_id` (and a purpose tag), an approval gathered for proposal `A` could
in principle be replayed to satisfy quorum on a different proposal `B` created
in the same context (same contract, same signer set).

The fix makes each approval **cryptographically scoped** to exactly one
`(contract, proposal_id, signer_set_hash, approver)` tuple.

## Payload layout

Every approval authorization payload is the SHA-256 hash of the following
concatenated byte string:

```text
sha256(
    DOMAIN_SEPARATOR
    || contract_id_xdr
    || proposal_id (8-byte big-endian)
    || signer_set_hash (32 bytes)
    || approver_xdr
)
```

| Field | Source | Purpose |
|-------|--------|---------|
| `DOMAIN_SEPARATOR` | `APPROVAL_DOMAIN_SEPARATOR` = `b"STELLARLEND_MULTISIG_APPROVAL_V1"` | Purpose tag. Prevents an approval signature from being reinterpreted for another contract feature. Bump the `_V1` suffix on any breaking layout change. |
| `contract_id_xdr` | `env.current_contract_address().to_xdr(env)` | Scopes the approval to this multisig instance. |
| `proposal_id` | `u64` big-endian | Scopes the approval to exactly one proposal (anti-replay across ids). |
| `signer_set_hash` | `sha256(SIGNER_SET_DOMAIN_SEPARATOR || ordered signer XDR)` | Prevents an approval from surviving signer-set rotation. |
| `approver_xdr` | `approver.to_xdr(env)` | Binds the hash to the signing address. |

The constant is exported as:

```rust
pub const APPROVAL_DOMAIN_SEPARATOR: &[u8] = b"STELLARLEND_MULTISIG_APPROVAL_V1";
```

## Enforcement

1. **Auth-layer binding (primary).** `approve_proposal` computes the domain-
   and signer-set-separated hash and calls:

   ```rust
   caller.require_auth_for_args((binding_hash,).into_val(&env));
   ```

   The host therefore requires an authorization entry whose args equal the
   hash for **this** `(contract, proposal_id, signer_set_hash, approver)`. An
   auth entry built for a different proposal or signer set produces a different
   hash and is rejected.

2. **Storage binding (audit / off-chain).** On successful approval the same
   hash is persisted under
   `MultisigDataKey::ApprovalBinding(proposal_id, approver)`.

3. **Views.**
   - `approval_binding_hash(id, approver)` — pure recompute of the hash
     (clients can precompute auth args).
   - `get_approval_binding(id, approver)` — returns the stored hash if any.
   - `verify_approval_binding(id, approver)` — recomputes and compares;
     returns `false` when no approval exists or the binding would not match
     this id (i.e. an approval intended for a different proposal).

4. **Unchanged guards.** Duplicate-approver rejection (`AlreadyApproved`),
   expiry (`ProposalExpired`), status checks, and the `approvals` list used
   for quorum counting are all preserved.

## Why this defeats cross-proposal replay

| Scenario | Result |
|----------|--------|
| Signer authorizes hash(…, id=1, …) and calls `approve_proposal(id=1)` | **Accepted** — auth args match |
| Signer reuses the id=1 auth entry to call `approve_proposal(id=2)` | **Rejected** — `require_auth_for_args` expects hash(…, id=2, …) |
| Approver list for id=1 inspected via `verify_approval_binding(id=2, …)` | **false** — no binding stored under id=2 |

Because the domain separator, contract id, proposal id, and signer-set hash
are folded in, the same bytes cannot be reinterpreted as an approval for a
different purpose, proposal, signer set, or multisig deployment.

## Execution nonce and migration

Every new proposal receives a monotonic execution nonce. The nonce is marked
consumed only after its action dispatch succeeds, in the same transaction as
the proposal status transition. A failed cross-contract execution therefore
does not burn a nonce, while a successful retry is rejected by both the
consumed marker and `ProposalStatus::Executed`.

Replay metadata is stored under new `MultisigDataKey` variants rather than by
changing the serialized `Proposal` layout. Older records remain readable. A
legacy active or passed proposal must be explicitly passed through
`migrate_proposal_security`; migration is accepted only while the proposer and
all recorded approvers remain current signers. If that cannot be proven, the
contract blocks the legacy proposal instead of guessing its original signer
set.

## Tests

See [`src/approval_binding_test.rs`](src/approval_binding_test.rs):

| Test | Asserts |
|------|---------|
| `approve_correct_id_records_verifiable_binding` | Binding stored + verifies; non-approver has none |
| `normal_approval_still_reaches_quorum` | Existing quorum path still works |
| `binding_hashes_differ_across_proposal_ids` | Distinct ids → distinct hashes |
| `binding_is_scoped_to_proposal_id` | Approve id1 does not create binding for id2 |
| `cross_proposal_auth_payload_rejected` | Auth args for id1 cannot approve id2 |
| `correct_domain_auth_payload_accepted` | Explicit correct-domain auth succeeds |
| `duplicate_approval_still_rejected` | `AlreadyApproved` preserved |
| `approval_after_expiry_rejected` | `ProposalExpired` preserved |
| `distinct_approvers_have_distinct_bindings` | Per-approver isolation |
| `domain_separator_constant_is_pinned` | Tag cannot change silently |

Run:

```bash
cargo test -p stellarlend-multisig approval_binding
```

## Client integration notes

When constructing a Soroban authorization entry for `approve_proposal`, set the
function args in the auth payload to the single domain-bound hash:

```text
args = [ approval_binding_hash(proposal_id, approver) ]
```

Do **not** sign the raw invocation args `(caller, id)` alone; the contract
verifies the domain-separated hash via `require_auth_for_args`.
