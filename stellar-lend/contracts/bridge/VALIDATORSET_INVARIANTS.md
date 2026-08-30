# ValidatorSet Invariants

This note explains the safety invariants now covered by
`src/validatorset_proptest.rs` for the bridge `ValidatorSet`.

## Why these invariants matter

`ValidatorSet::threshold`, `ValidatorSet::len`, and `ValidatorSet::contains_pk`
feed directly into bridge quorum verification. If any of them drift from the
effective validator membership, the bridge can become unsafe in either
direction:

- Too low a threshold weakens quorum safety.
- Too high a threshold can deadlock validator rotation.
- Inconsistent membership checks can accept or reject proofs incorrectly.

The new properties keep those three methods aligned even when the raw stored
validator list contains duplicates.

## Invariants

For every generated validator set:

- If the set is non-empty, `1 <= threshold() <= len()`.
- `contains_pk(pk)` is true if and only if `pk.to_bytes()` appears in
  `to_bytes_vec()`.
- Duplicate keys do not increase `len()` or `threshold()`.

`len()` now means the number of unique validator public keys, not the raw byte
vector length. That matches how quorum proofs are counted in
`Bridge::verify_quorum_proof`, which now rejects duplicate proof signers before
comparing unique valid signatures to the threshold.

## Worked example

Suppose a malformed validator list is stored as:

```text
[A, A, A, B]
```

Raw length is 4, but the effective validator membership is only `{A, B}`.

- Unique `len()` = 2
- `threshold()` = `(2 * 2) / 3 + 1 = 2`
- `contains_pk(A)` = true
- `contains_pk(B)` = true
- `contains_pk(C)` = false

Without deduplication, the threshold would have been computed from 4 entries and
become 3, which is impossible to satisfy with only 2 distinct signers. The new
behavior prevents that silent liveness failure.

## Edge cases

- Empty set: `threshold()` remains the arithmetic result `1`, but the bounded
  threshold invariant is enforced only for non-empty sets.
- Singleton set: `len() == 1` and `threshold() == 1`.
- Large set: the supermajority formula remains `floor(2n / 3) + 1`, applied to
  the unique validator count.
- Duplicate-heavy set: `to_bytes_vec()` still preserves raw storage order and
  duplicates for audit/debug visibility, while quorum math ignores repeats.
