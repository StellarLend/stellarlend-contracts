# Multisig Action and Approval Binding

An approval authorizes a signer to count toward quorum for a **specific**
proposal, signer set and action. Issue
[#1278](https://github.com/StellarLend/stellarlend-contracts/issues/1278)
scoped approvals to `(contract, proposal_id, signer_set_hash, approver)`;
issue [#2287](https://github.com/StellarLend/stellarlend-contracts/issues/2287)
additionally binds each approval to the exact action stored on the proposal.
This changes the off-chain authorization format; clients must use V2.

## Action hash

At creation, the contract serializes the typed `ProposalAction` to canonical
Soroban XDR and computes:

```text
action_hash = SHA256(
  b"STELLARLEND_MULTISIG_ACTION_V1" || action.to_xdr(env)
)
```

The action hash is always computed by the contract (never taken from the
caller) and stored separately from the proposal record under
`MultisigDataKey::ActionHash(id)`; `get_action_hash(id)` exposes it.
`create_proposal`'s `payload_hash: Bytes` argument is unchanged and remains the
caller-supplied value checked again at execution time. Execution (single and
batch) recomputes the action hash and checks it against the pinned value before
dispatch.

## Approval authorization V2 byte layout

The signer authorizes the SHA-256 of this exact concatenation:

```text
SHA256(
  DOMAIN_SEPARATOR
  || contract_id_xdr
  || proposal_id_be64
  || signer_set_hash
  || action_hash
  || approver_xdr
)
```

| Order | Bytes | Encoding |
|---|---|---|
| 1 | `STELLARLEND_MULTISIG_APPROVAL_V2` | ASCII domain separator, no terminator |
| 2 | Current multisig contract address | Canonical Soroban XDR |
| 3 | Proposal id | Unsigned 64-bit integer, 8-byte big-endian |
| 4 | Signer-set hash captured when the proposal was created: `sha256(SIGNER_SET_DOMAIN_SEPARATOR \|\| ordered signer XDR)` | Raw 32 bytes |
| 5 | Action hash (see above) | Raw 32 bytes |
| 6 | Approver address | Canonical Soroban XDR |

The V2 format is breaking for off-chain signing clients. They should call
`approval_binding_hash(id, approver)` (or fetch `get_proposal_signer_set_hash(id)`
and `get_action_hash(id)`) to construct authorization args.

## Enforcement

1. **Auth-layer binding.** `approve_proposal` computes the hash above and calls
   `caller.require_auth_for_args((binding_hash,).into_val(&env))`. An auth
   entry built for a different proposal, signer set or action produces a
   different hash and is rejected.
2. **Storage binding.** On successful approval the same hash is persisted under
   `MultisigDataKey::ApprovalBinding(proposal_id, approver)`.
3. **Views.** `approval_binding_hash`, `get_approval_binding`,
   `verify_approval_binding`, `get_action_hash`.
4. **Unchanged guards.** Duplicate-approver rejection, expiry, status checks
   and quorum counting are preserved.

## Execution nonce and migration

Every new proposal receives a monotonic execution nonce, marked consumed only
after its action dispatch succeeds. Replay metadata (nonce, signer-set hash,
action hash) is stored under separate `MultisigDataKey` variants rather than by
changing the serialized `Proposal` layout, so older records remain readable.

A legacy active or passed proposal must be explicitly passed through
`migrate_proposal_security(caller, id)`; migration is accepted only while the
proposer and all recorded approvers remain current signers, and it stamps the
action hash from the stored action. Existing legacy approvals do not become V2
approvals and must be collected again if the proposal needs further approval.

## Tests

See `src/approval_binding_test.rs` and `src/action_hash_binding_test.rs`.

```bash
cargo test -p stellarlend-multisig approval_binding
cargo test -p stellarlend-multisig action_hash
```
