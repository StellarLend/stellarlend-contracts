# Bridge validator-set rotation protocol

This document defines how bridge operators propose, prove, and apply a
validator-set rotation. It describes the protocol implemented by
`Bridge::quorum_proof_payload`, `Bridge::rotate_validators`, and
`Bridge::validate_inbound_epoch` in `src/lib.rs`.

## Protocol roles and terms

- **Current set**: the validator set stored on the bridge before a rotation.
- **Active validator**: a member of the current set that is not paused.
- **Proposed set**: the ordered validator public keys that will replace the
  current set if the rotation succeeds.
- **Epoch**: the monotonically increasing validator-set version. A new bridge
  starts at epoch `0`.
- **Quorum proof**: a list of `(public_key, Ed25519 signature)` pairs from
  unique active members of the current set.

The current set authorizes its successor. Validators in the proposed set do
not authorize their own installation unless they are also active members of
the current set.

## What validators sign

Every signer must sign the exact bytes returned by
`Bridge::quorum_proof_payload`:

```text
payload = bincode((
    QUORUM_PROOF_DOMAIN,
    bridge_id,
    proposed_set.to_bytes_vec(),
    next_epoch,
))
```

The fields bind the proof to:

1. the validator-rotation purpose and payload version through
   `QUORUM_PROOF_DOMAIN`;
2. one bridge deployment through `bridge_id`;
3. the complete proposed validator set, including its order; and
4. the one epoch at which the set may be installed.

Operators must distribute one canonical payload to all signers. Reordering
the proposed keys, changing a key, changing the bridge ID, or changing the
epoch produces different bytes and invalidates previously collected
signatures.

## Supermajority quorum

For `n` active validators, the required number of unique valid signatures is:

```text
threshold(n) = floor((n * 2) / 3) + 1
```

This is strictly greater than two thirds. With no paused validators, `n` is
the deduplicated size of the current set and matches
`ValidatorSet::threshold()`. If validators are paused, `n` is the remaining
active count and the bridge uses `Bridge::effective_threshold()`.

| Active validators (`n`) | Required signatures |
|---:|---:|
| 3 | 3 |
| 4 | 3 |
| 5 | 4 |
| 6 | 5 |
| 7 | 5 |
| 10 | 7 |
| 32 | 22 |

Proof verification applies these counting rules:

- a signer must belong to the current validator set;
- a paused validator is ignored and does not count toward quorum;
- duplicate proof entries for one public key count once;
- every counted signature must verify against the canonical payload; and
- the number of unique, valid, active signers must meet the threshold.

An empty proof, an outsider signer, an invalid counted signature, or too few
unique active signatures rejects the rotation.

## Rotation procedure

Assume the bridge is at epoch `e` with current set `S`.

1. **Choose the successor.** Build an ordered proposed set `S_next`. It must
   contain between `MIN_VALIDATORS` and `MAX_VALIDATORS` unique public keys and
   may not contain duplicate keys.
2. **Choose the next epoch.** Set `next_epoch = e + 1`. Reusing `e`, skipping
   to `e + 2`, or replaying an older epoch is rejected.
3. **Build the payload.** Call `Bridge::quorum_proof_payload` with this
   bridge's `bridge_id`, `S_next`, and `next_epoch`.
4. **Collect signatures.** Active validators in `S` sign those exact bytes.
   Collect at least `floor((n * 2) / 3) + 1` unique valid signatures, where
   `n` is the active validator count.
5. **Submit the proof.** Call
   `rotate_validators(S_next, next_epoch, proofs)`.
6. **Validate before mutation.** The bridge checks the epoch, proposed-set
   bounds and duplicates, any configured churn limit, proof membership,
   signatures, deduplication, and quorum.
7. **Commit atomically.** Only after every check succeeds does the bridge
   replace `S` with `S_next`, set its epoch to `next_epoch`, and clear pause
   flags inherited from the retired set. The returned value is the symmetric
   difference (churn) between the old and new sets.

Any error leaves the validator set and epoch unchanged.

## Worked example: five validators rotate at epoch 7

Suppose the bridge is at epoch `7` with five active validators:

```text
current set S = [A1, A2, A3, A4, A5]
proposed set  = [B1, B2, B3, B4]
next_epoch    = 8
```

The current-set threshold is:

```text
floor((5 * 2) / 3) + 1 = floor(10 / 3) + 1 = 3 + 1 = 4
```

The operator constructs the canonical payload for the proposed set and epoch
`8`, then asks `A1` through `A5` to sign it.

- Signatures from `A1`, `A2`, `A3`, and `A4`: **accepted** (4 unique current
  signers meet the threshold).
- Signatures from only `A1`, `A2`, and `A3`: **rejected** (3 is below the
  threshold).
- Four proof entries containing only three unique signers: **rejected**
  (duplicates count once).
- Signatures from `B1` through `B4` alone: **rejected** unless those keys are
  also in the current set; the proposed set cannot self-authorize.

After the valid proof is applied, the bridge is at epoch `8`, `[B1, B2, B3,
B4]` is the current set, and its unpaused quorum threshold is `3`. A later
rotation must be authorized by that new current set for epoch `9`.

## Inbound epoch validation and retired sets

Inbound processing calls `validate_inbound_epoch(signed_epoch)` to prevent
messages from a retired validator-set epoch from being replayed after a
rotation.

```text
signed_epoch < bridge.epoch  => reject as a retired validator set
signed_epoch == bridge.epoch => pass this retired-set check
```

In the worked example, once epoch `8` is installed, inbound messages carrying
epoch `7` or earlier are rejected. Messages carrying epoch `8` pass this
check. Signatures from set `S` also cannot authorize a later rotation because
proof signers must belong to the current set, which is now the proposed set.

`validate_inbound_epoch` is deliberately a retired-set guard, not a complete
inbound authentication routine: the current implementation also returns
success for a future epoch. Callers must still verify the message signature,
bridge/domain binding, and any policy that requires the inbound epoch to equal
the current epoch.

## Operator checklist

- Use a non-empty, deployment-unique `bridge_id`.
- Preserve the proposed validator ordering when distributing the payload.
- Confirm `next_epoch` equals the bridge's current epoch plus one.
- Compute quorum from the current active set, not the proposed set.
- Deduplicate signers before declaring the proof ready.
- Submit the same proposed set and epoch that were signed.
- After rotation, update relayers so they stop accepting or producing messages
  for the retired epoch.

## Related documentation

- [README.md](./README.md) - bridge overview and documentation index.
- [SECURITY_NOTES.md](./SECURITY_NOTES.md) - threat model and security rationale.
- [EPOCH_INVARIANTS.md](./EPOCH_INVARIANTS.md) - epoch monotonicity invariants.
- [VALIDATORSET_INVARIANTS.md](./VALIDATORSET_INVARIANTS.md) - validator-set
  and threshold invariants.
- [VALIDATOR_PAUSE.md](./VALIDATOR_PAUSE.md) - active-set quorum behavior when
  validators are paused.
