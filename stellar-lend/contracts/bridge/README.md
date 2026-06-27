# Bridge crate

This crate implements a minimal, auditable validator-set rotation primitive for a cross-chain bridge.

Key features
- `Bridge` struct with an `epoch` counter and validator set
- `rotate_validators(new_set, epoch, proofs)` requires a quorum proof from the *current* validator set and advances the epoch atomically
- `validate_inbound_epoch(signed_epoch)` rejects messages signed by retired epochs (signed_epoch < current epoch)
- `set_inbound_cap(max_per_window, window_size, current_time)` / `admit_inbound(amount, current_time)` enforce a configurable, rolling-ledger-time cap on cumulative inbound value — defense-in-depth against an authorized-but-compromised validator set draining the bridge in a single window. **Fail-closed by default**: a fresh `Bridge` admits no inbound value until a cap is explicitly configured. See `SECURITY_NOTES.md` for the full threat model and design rationale.

Design notes
- Validator public keys are stored as raw bytes to keep on-disk/state serialization simple and unambiguous.
- Quorum threshold is a supermajority > 2/3 of current validators.
- Signatures are over a canonical payload: `bincode((new_set_bytes_vec, epoch))` — this binds the epoch to the new set.
- This crate is intentionally standalone (off-chain validator/quorum verification logic) and is not a member of the parent Soroban workspace; run `cargo test` from this directory.

See `src/lib.rs` for implementation and unit tests.

## Fee-Conservation Property (Issue #1137)

Every bridge deposit and withdraw must satisfy **value conservation**: no satoshi
is created or destroyed by the fee deduction.

### Formula

```
fee      = ⌊ amount × fee_bps / 10_000 ⌋   (floor division)
credited = amount − fee
```

### Invariants

| ID | Invariant |
|----|-----------|
| C-1 | `credited + fee == amount` for every transfer |
| C-2 | `accrued = Σ fee_i` — protocol accrual equals the running sum of individual fees |
| C-3 | `fee_bps = 0` → `fee = 0`, `credited = amount` |
| C-4 | Zero or negative amounts are rejected |
| C-5 | `fee ≤ amount` always (even at `fee_bps = 10_000`) |
| C-6 | deposit → withdraw round-trip: `final_out + total_accrued == amount_in` |

### Worked Example (`amount = 1 000`, `fee_bps = 300`)

```
fee      = ⌊ 1 000 × 300 / 10 000 ⌋ = ⌊ 30.0 ⌋ = 30
credited = 1 000 − 30 = 970
check:   970 + 30 = 1 000  ✓
```

### Rounding Note

Floor division means the protocol always rounds in its own favour. A dust
amount (e.g. `amount = 1, fee_bps = 9_999`) rounds the fee down to 0, so the
user is never overcharged relative to exact arithmetic.

See `stellar-lend/contracts/hello-world/src/bridge_fee_test.rs` for the full
property-based and deterministic test suite.
