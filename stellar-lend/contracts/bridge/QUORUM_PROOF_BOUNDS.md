# Quorum Proof Bounds

`Bridge::rotate_validators` accepts a quorum proof from the current validator set
before it installs a new validator set. The proof vector is supplied by the
caller, so its size and signer contents must be bounded before any expensive
signature verification runs.

## Bound

`verify_quorum_proof` rejects any proof vector with more entries than the current
validator set has unique validators:

```text
proofs.len() <= current_validator_set.len()
```

The current set is the correct bound because every proof signer must be a member
of the current set. A larger proof cannot add valid voting power; it can only
force redundant parsing, membership checks, and signature work.

## Duplicate Signers

Each signer public key may appear at most once in the proof vector. Duplicate
proof entries are rejected before payload construction and before any signature
verification.

This is stricter than counting a duplicate once. Rejecting the whole proof makes
malformed or spammy relay input visible to callers and prevents an attacker from
padding a rotation attempt with repeated signatures.

## DoS Rationale

Signature verification dominates rotation cost. Without a proof-vector bound, an
attacker can submit a very large `proofs` list with duplicated keys and force the
bridge to do work unrelated to the number of validators.

The early checks keep worst-case proof processing proportional to the current
validator set:

```text
O(current_validator_set_size)
```

The verifier still preserves the existing valid-proof behavior:

- an exact-quorum proof is accepted,
- a proof with one entry per current validator is accepted when signatures are valid,
- a below-quorum proof is rejected as insufficient,
- paused validator signatures remain ignored for quorum weight after the vector
  has passed the size and duplicate checks.
