# Approval Domain Binding

This document describes how `approve_proposal` in `src/lib.rs` binds each
approval to the exact proposal it was intended for, closing the
cross-proposal approval-reuse vector discussed in issue #1278.

## Threat model

An approval authorizes a signer to count toward quorum for a specific proposal.
If the authorization payload were not uniquely scoped, a signature/approval
gathered for proposal `A` could in principle be replayed to satisfy quorum on a
different proposal `B` created in the same context.

## Payload layout

Every approval binding is the SHA-256 hash of the following concatenated byte
string:

```
sha256(
    DOMAIN_SEPARATOR
    || contract_id_xdr
    || proposal_id (8-byte big-endian)
    || approver_xdr
)
```

* `DOMAIN_SEPARATOR` — the fixed byte string
  `b"STELLARLEND_MULTISIG_APPROVAL_V1"` (see `APPROVAL_DOMAIN_SEPARATOR`).
  Bump the `_V1` suffix on any breaking change to the layout.
* `contract_id_xdr` — the XDR encoding of the executing contract's address
  (`env.current_contract_address().to_xdr(env)`).
* `proposal_id` — the `u64` proposal id, big-endian.
* `approver_xdr` — the XDR encoding of the approving signer's address.

## Enforcement

1. `approve_proposal` already calls `caller.require_auth()`, which cryptographically
   scopes the authorization to the exact invocation `(contract, approve_proposal,
   id)`. This is the primary, Soroban-native protection against cross-proposal
   replay.
2. On top of that, `approve_proposal` computes the domain-separated binding hash
   and persists it under `MultisigDataKey::ApprovalBinding(proposal_id, approver)`.
3. `verify_approval_binding(proposal_id, approver)` recomputes the hash and
   compares it against the stored value, returning `false` when no approval
   exists for that `(proposal_id, approver)` pair or when the binding does not
   match — i.e. an approval intended for a different proposal.
4. `get_approval_binding(proposal_id, approver)` exposes the raw stored hash for
   off-chain/indexer verification.

Because the binding is keyed by `(proposal_id, approver)` and derived from a
domain separator that includes the contract id, an approval recorded for one
proposal can never validate against another proposal's id.

## Tests

See `src/approval_binding_test.rs`:

* `approval_records_verifiable_binding_for_approver` — a recorded approval
  verifies, and a non-approving signer does not.
* `binding_is_scoped_to_proposal_id` — approving proposal 1 does not create a
  binding for proposal 2.
* `distinct_approvers_have_distinct_bindings` — multiple approvers each get their
  own binding; an unrelated address has none.
