# Bridge Epoch Invariants

This document lists the epoch-monotonicity properties proven by the
property-based test suite in
`src/epoch_monotonicity_proptest.rs`.

---

## Proven Properties

### P1 — Strict Monotonicity

> `bridge.epoch` never decreases between any two consecutive observations.

Formally:

```
∀ rotation attempt r:
    epoch_after(r) >= epoch_before(r)
```

Verified by `prop_epoch_monotonic_and_no_skip` and
`prop_invariants_hold_at_large_epoch` across thousands of randomly generated
sequences.

---

### P2 — No-Skip Rule (exactly +1 on success)

> Every *successful* call to `rotate_validators` advances `bridge.epoch`
> by exactly one.

Formally:

```
rotate_validators(new_set, epoch, proofs) == Ok(())
    ⟹  bridge.epoch_after == bridge.epoch_before + 1
```

The contract enforces this directly:

```rust
if epoch != self.epoch + 1 {
    return Err(anyhow!("invalid epoch: must be current_epoch + 1"));
}
// …
self.epoch = epoch;   // epoch == self.epoch + 1, so increment is always +1
```

Proptest probes both the boundary (P1+P2 hold at `epoch = 0`) and large
epoch numbers (P2 still holds at `epoch = 50`) via
`prop_invariants_hold_at_large_epoch` and `edge_large_epoch_no_overflow_or_regress`.

---

### P3 — No-Regress on Rejection

> Every *rejected* call to `rotate_validators` leaves `bridge.epoch` unchanged.

Formally:

```
rotate_validators(new_set, epoch, proofs) == Err(_)
    ⟹  bridge.epoch_after == bridge.epoch_before
```

Because `rotate_validators` is structured as:

1. Check epoch validity (returns `Err` before any mutation).
2. Verify quorum proof (returns `Err` before any mutation).
3. Atomically swap `self.validators` and `self.epoch`.

A rejection at steps 1 or 2 leaves the struct untouched.  Proptest confirms
this for all eight fault-injection categories (see table below).

---

### P4 — Final Epoch Equals Success Count

> After a sequence of _N_ successful rotations (starting from epoch 0),
> `bridge.epoch == N`.

Formally:

```
bridge.epoch_initial == 0
∀ sequence S of rotation attempts:
    bridge.epoch_final == |{r ∈ S : rotate_validators(r) == Ok(())}|
```

This is a corollary of P2 and P3 but is asserted explicitly as a sequence-level
invariant in `prop_epoch_monotonic_and_no_skip` and
`prop_interleaved_valid_and_fault`.

---

## Fault-Injection Categories

The proptest strategy generates sequences of [`RotationAttempt`] variants
uniformly at random.  Every category is expected to be rejected (except
`Valid`), and P3 is verified for each.

| Variant | Injected fault | Expected outcome |
|---------|---------------|-----------------|
| `Valid` | None | `Ok(())` — epoch + 1 |
| `WrongEpochSame` | `epoch == current` | `Err` — epoch unchanged |
| `WrongEpochSkip` | `epoch == current + 2` | `Err` — epoch unchanged |
| `WrongEpochStale` | `epoch == current − 1` (or 0) | `Err` — epoch unchanged |
| `InsufficientQuorum` | `(threshold − 1)` signatures | `Err` — epoch unchanged |
| `EmptyProofs` | Zero signatures | `Err` — epoch unchanged |
| `OutsideSigner` | One signer not in current set | `Err` — epoch unchanged |
| `WrongPayloadSig` | Signatures over wrong epoch number | `Err` — epoch unchanged |

---

## Test Cases

### Property-based (`proptest`)

| Test | Input space | Properties checked |
|------|-------------|-------------------|
| `prop_epoch_monotonic_and_no_skip` | Random sequences of 1–20 attempts from all 8 variants | P1, P2, P3, P4 |
| `prop_invariants_hold_at_large_epoch` | 30 warm-up valid rotations + 1–10 random fault attempts | P1, P2, P3 at large epoch |
| `prop_repeated_same_epoch_never_advances` | 1–15 repeated same-epoch attempts | P3 (same-epoch variant) |
| `prop_interleaved_valid_and_fault` | Alternating fault + valid, 2–10 pairs | P2, P3, P4 for interleaved sequences |

### Deterministic edge-case regression

| Test | Scenario | Property |
|------|----------|----------|
| `edge_rejected_mid_sequence_leaves_state_intact` | valid → rejected (skip) → valid | P3 mid-sequence; final epoch == 2 |
| `edge_large_epoch_no_overflow_or_regress` | 50 sequential valid rotations | P2 at epoch 50; `validate_inbound_epoch` correctness |
| `edge_stale_at_epoch_zero_rejected` | epoch 0 attempt when current == 0 | P3 at the zero boundary |

---

## Running the Tests

```sh
# Run all bridge tests (unit + proptest)
cargo test -p bridge

# Run only the epoch monotonicity proptest
cargo test -p bridge epoch_monotonicity

# Increase proptest cases (default: 256)
PROPTEST_CASES=10000 cargo test -p bridge epoch_monotonicity
```

---

## Security Assumptions

The epoch monotonicity guarantee relies on the following assumptions being
upheld by callers:

1. **Atomic mutation** — `rotate_validators` is the only code path that writes
   `self.epoch` and `self.validators`.  No out-of-band storage mutation is
   possible in the bridge's pure-Rust model.

2. **Quorum authenticity** — the ed25519 signatures in `proofs` are
   cryptographically bound to `(new_set_bytes, epoch)` via the payload
   serialised by `bincode::serialize`.  An attacker cannot forge a proof
   for a different epoch without breaking ed25519.

3. **No integer overflow** — `u64` epochs starting at 0 with one increment
   per rotation would require 1.8 × 10¹⁹ rotations to overflow.
   `prop_invariants_hold_at_large_epoch` and
   `edge_large_epoch_no_overflow_or_regress` confirm correct behaviour at
   epoch 50; the overflow scenario is not separately tested because it is
   physically unreachable in any realistic deployment.
