# Bridge crate

This crate implements a minimal, auditable validator-set rotation primitive for a cross-chain bridge.

Key features
- `Bridge` struct with an `epoch` counter and validator set
- `rotate_validators(new_set, epoch, proofs)` requires a quorum proof from the *current* validator set and advances the epoch atomically
- `validate_inbound_epoch(signed_epoch)` rejects messages signed by retired epochs (signed_epoch < current epoch)

Design notes
- Validator public keys are stored as raw bytes to keep on-disk/state serialization simple and unambiguous.
- Quorum threshold is a supermajority > 2/3 of current validators.
- Signatures are over a canonical payload: `bincode((new_set_bytes_vec, epoch))` — this binds the epoch to the new set.

See `src/lib.rs` for implementation and unit tests.
