# Multisig Action and Approval Binding

Issue [#2287](https://github.com/StellarLend/stellarlend-contracts/issues/2287) binds each approval to the exact action stored on a proposal. This changes the off-chain authorization format; clients must use V2.

## Action hash

At creation, the contract serializes the typed `ProposalAction` to canonical Soroban XDR and computes:

```text
action_hash = SHA256(
  b"STELLARLEND_MULTISIG_ACTION_V1" || action.to_xdr(env)
)
```

`create_proposal` retains its `payload_hash: Bytes` argument for interface compatibility and rejects it with `PayloadHashMismatch` unless it equals the computed 32-byte hash. The contract stores the hash separately from the proposal record; `get_action_hash(id)` exposes it. Execution recomputes the hash and checks it before dispatch.

## Approval authorization V2 byte layout

The signer authorizes the SHA-256 of this exact concatenation:

```text
SHA256(
  DOMAIN_SEPARATOR
  || contract_id_xdr
  || proposal_id_be64
  || signer_set_hash_xdr
  || action_hash_xdr
  || approver_xdr
)
```

| Order | Bytes | Encoding |
|---|---|---|
| 1 | `STELLARLEND_MULTISIG_APPROVAL_V2` | ASCII domain separator, no terminator |
| 2 | Current multisig contract address | Canonical Soroban XDR |
| 3 | Proposal id | Unsigned 64-bit integer, 8-byte big-endian |
| 4 | SHA-256 of current signer vector XDR | 32-byte hash, encoded as `BytesN<32>` XDR |
| 5 | Action hash | 32-byte hash, encoded as `BytesN<32>` XDR |
| 6 | Approver address | Canonical Soroban XDR |

The signer-set hash is `SHA256(signers.to_xdr(env))`; action hashing uses XDR values, never host `Val` bits. The V2 format is breaking for off-chain signing clients. They must fetch `get_action_hash(id)` and call `approval_binding_hash(id, approver)` to construct authorization args.

## Migration

The action hash is stored under a separate storage key to preserve the serialized `Proposal` layout. Existing proposals without that key can be migrated by a registered signer using `migrate_proposal_security(caller, id)`. Migration computes and stamps the action hash from the stored action. Existing legacy approvals do not become V2 approvals and must be collected again if the proposal needs further approval.

## Views and checks

- `get_action_hash(id)` returns the stored hash, or `None` before migration for a legacy proposal.
- `approval_binding_hash(id, approver)` recomputes the V2 authorization hash.
- `get_approval_binding(id, approver)` returns the recorded hash.
- `verify_approval_binding(id, approver)` recomputes and compares the binding.

Expiry and proposal status checks remain before action verification and dispatch. Batch execution validates each proposal's stored hash against its action before dispatching any proposal.
