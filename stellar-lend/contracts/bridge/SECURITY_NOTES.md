# Security Notes — Bridge Validator Rotation

Threat model and mitigations

- Operator key compromise: Rotation requires a quorum proof signed by the *current* validator set. An operator private key compromise (single key) cannot rotate the set unless a quorum of current validators collude.
- Replay and downgrade: The `epoch` counter prevents accepting messages signed by retired validator sets (any signed_epoch < current epoch is rejected). Rotation requires epoch == current_epoch + 1, preventing out-of-order rotations.
- Signature binding: The proof signs the serialized tuple `(new_set_bytes_vec, epoch)`, binding the new validator set to the specific epoch.

Implementation notes

- Quorum: uses strict supermajority (floor(2n/3)+1). This should be chosen to match protocol requirements; adjust if BFT tolerance differs.
- Serialization: validators stored as `Vec<Vec<u8>>` (raw public key bytes) to ensure deterministic encoding and avoid cross-crate serde issues.
- Atomicity: `rotate_validators` performs proof verification before swapping validators and advancing the epoch.

Operational guidance

- Ensure secure key management for validator private keys and rotate keys off-channel when needed.
- When rotating, collect signatures from the current validator set over the exact payload — tooling should canonicalize key ordering and serialization before signing.
- Audit the on-chain representation to guarantee encoding matches the signing payload used by operator tooling.

Testing and coverage

`rotation_test.rs` provides ≥ 95 % coverage on `rotate_validators` and
`validate_inbound_epoch` and locks down the following invariants:

### Epoch monotonicity

| Scenario | Expected outcome |
|---|---|
| `epoch == current_epoch` (same, non-incrementing) | **Rejected** — `invalid epoch` |
| `epoch == current_epoch + 2` (skipped) | **Rejected** — `invalid epoch` |
| `epoch < current_epoch` (stale replay) | **Rejected** — `invalid epoch` |
| `epoch == current_epoch + 1` (correct) | **Accepted** |

The epoch counter must increment by exactly **1** on every successful rotation.
After `n` rotations the bridge's `epoch` field equals `n`.

### Quorum-threshold enforcement on rotation

The supermajority threshold is `floor(2n/3) + 1` for an `n`-validator set.

| Scenario | Expected outcome |
|---|---|
| Exactly `threshold` unique valid signatures | **Accepted** |
| `threshold − 1` unique valid signatures | **Rejected** — `insufficient quorum` |
| Duplicate signer entries (counted once each) | Deduplicated before counting |
| Duplicate signer that inflates apparent count to threshold but unique count is below | **Rejected** |
| Signer whose public key is not in the current set | **Rejected** — `signer not in current validator set` |
| Empty proof list | **Rejected** — `empty proofs` |

### Rotated-out-set replay rejection

- After rotation A → B, any inbound message bearing `signed_epoch < current_epoch`
  is rejected by `validate_inbound_epoch` with `retired validator set`.
- Attempting to trigger a *further* rotation (B → C) using signatures from the
  already-rotated-out set A is rejected because A's keys are no longer in the
  current validator set.

### Multi-rotation correctness

Sequential rotations A → B → C → … produce a strictly monotonically increasing
epoch sequence. All epochs prior to the current one are rejected for inbound
messages.

### References

- `src/rotation_test.rs` — full test implementations.
- Before deployment, run integration tests and perform a security review
  comparing the on-chain encoding and off-chain signing tools.
