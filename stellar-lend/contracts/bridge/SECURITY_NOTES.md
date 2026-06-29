# Security Notes — Bridge Validator Rotation

Threat model and mitigations

- Operator key compromise: Rotation requires a quorum proof signed by the *current* validator set. An operator private key compromise (single key) cannot rotate the set unless a quorum of current validators collude.
- Replay and downgrade: The `epoch` counter prevents accepting messages signed by retired validator sets (any signed_epoch < current epoch is rejected). Rotation requires epoch == current_epoch + 1, preventing out-of-order rotations. **Far-future epochs** (`signed_epoch > current_epoch + MAX_FUTURE_EPOCH_TOLERANCE`, currently `current + 1`) are also rejected — see [Future-epoch window policy](#future-epoch-window-policy) below.
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
- `src/inbound_epoch_test.rs` — bounded active-epoch-window test implementations.
- Before deployment, run integration tests and perform a security review
  comparing the on-chain encoding and off-chain signing tools.

---

## Future-epoch window policy

### Threat model

The previous policy accepted any inbound message whose `signed_epoch ≥ self.epoch`.
That had a quiet but serious gap: a message claiming an absurdly large future
epoch (say, `signed_epoch = 1_000_000` when `self.epoch == 1`) was still
honoured as long as it was greater than the active epoch. The validator set
had not yet rotated to that future epoch, so the message was not actually
authorised by anyone — yet the bridge would accept it. Two concrete
consequences followed:

1. **Trust extended to an unrotated-out set.** Honoring a message signed by a
   not-yet-active validator set grants inbound-value authority to a key set
   the bridge has never installed.
2. **Replay window.** Once the future epoch eventually arrived through a
   legitimate rotation, the same signed message would still be valid under
   the now-active set, with no new signer involvement required. An attacker
   who captured one such message would be able to replay it.

### Policy

`Bridge::validate_inbound_epoch` now enforces a **bounded active epoch
window**:

```text
  current = self.epoch
  tolerance = MAX_FUTURE_EPOCH_TOLERANCE   // currently 1

  accepted  ⇔  current ≤ signed_epoch ≤ current + tolerance

  signed_epoch <  current               →  rejected: "retired validator set"
  signed_epoch == current               →  accepted (active set)
  signed_epoch == current + tolerance   →  accepted (in-flight rotation window)
  signed_epoch >  current + tolerance   →  rejected: "not-yet-active"
```

`MAX_FUTURE_EPOCH_TOLERANCE = 1` is chosen deliberately:

- Rotation in this codebase already requires `epoch == current + 1`, so a
  message with `signed_epoch == current + 1` can realistically originate
  from the *current* validator set recorded in-flight, before the rotation
  transaction has been finalised on-chain.
- A tolerance of `1` is the smallest non-zero value that absorbs the
  in-flight transition window without admitting any further future epoch.
- A larger tolerance (say `2`) would re-open the replay window by allowing
  signed messages to be honoured before the corresponding rotation has
  taken effect, and is therefore explicitly **not** recommended. Any change
  to this constant must come with a matching update to the rotation-event
  timing model and a release note.

### Worked example

With `self.epoch = 5`, `MAX_FUTURE_EPOCH_TOLERANCE = 1`:

| `signed_epoch` | Verdict                                              |
|----------------|------------------------------------------------------|
| `3`            | **Rejected** — retired validator set (epoch too old)|
| `4`            | **Rejected** — retired validator set (epoch too old)|
| `5`            | **Accepted** — current                               |
| `6`            | **Accepted** — current + 1, in-flight rotation window|
| `7`            | **Rejected** — not-yet-active (first far-future)     |
| `8`, `9`, …    | **Rejected** — not-yet-active                        |
| `1_000_000`    | **Rejected** — not-yet-active (absurd far-future)    |
| `u64::MAX`     | **Rejected** — not-yet-active (saturating arithmetic guards against overflow)|

### Arithmetic

The upper bound is computed as `self.epoch.saturating_add(MAX_FUTURE_EPOCH_TOLERANCE)`,
so that if `self.epoch` is ever `u64::MAX` (defensive — unreachable in
practice), the comparison does not panic.

### Testing and coverage

`src/inbound_epoch_test.rs` provides ≥ 95 % coverage on
`Bridge::validate_inbound_epoch` and locks down the following invariants:

| Scenario | Expected outcome |
|---|---|
| `signed_epoch < self.epoch` (one or many) | **Rejected** — `retired validator set` |
| `signed_epoch == self.epoch` (current) | **Accepted** |
| `signed_epoch == self.epoch + MAX_FUTURE_EPOCH_TOLERANCE` (in-flight) | **Accepted** |
| `signed_epoch == self.epoch + MAX_FUTURE_EPOCH_TOLERANCE + 1` (first far-future) | **Rejected** — `not-yet-active` |
| `signed_epoch >> self.epoch` (absurd far-future, `u64::MAX`) | **Rejected** — `not-yet-active`, no panic |
| Two rejection branches must produce distinct, recognisable error strings | assertion in code |
| Boundary sweep over `[current - 2, …, current + tolerance + 100]` | every verdict matches the table above |

---

## Per-Window Inbound Value Cap

### Threat model and rationale

Validator quorum and epoch checks defend against an *unauthorized* validator
set making changes. They do not bound how much value an *authorized* (but
compromised, buggy, or malicious-majority) validator set can move across the
bridge in one window. A quorum compromise or a logic bug elsewhere in the
inbound-processing path can otherwise drain an unbounded amount in a single
epoch.

`Bridge::admit_inbound` adds a second, independent layer: a configurable cap
on the total inbound value admitted within a rolling ledger-time window. This
is defense-in-depth — it limits the *blast radius* of a failure elsewhere,
it does not replace quorum/epoch validation.

### Design notes

- **Fail-closed by default.** A freshly constructed `Bridge` has
  `max_per_window == 0`. Per the explicit design requirement, a cap of `0`
  means *no inbound* — not *unlimited* — so the bridge admits nothing until
  an operator calls `set_inbound_cap` with a positive value. This also means
  an explicitly-configured `0` (e.g. an emergency pause) behaves identically:
  it rejects every amount, including `0`-value transfers.
- **Ledger time, not block/call count.** The window is tracked against a
  `current_time: u64` passed in by the caller (intended to be the chain's
  ledger timestamp), per the requirement that the window reset on monotonic
  time rather than on a fixed number of calls. This means an attacker can't
  extend or shrink the effective window by batching many small calls.
- **Window realignment on rollover, not fixed-step advancement.** When
  `current_time` has moved past the end of the current window,
  `roll_window_if_expired` resets `window_start` to `current_time` directly
  (rather than repeatedly adding `window_size`). If the bridge sits idle for
  much longer than one window, the next inbound transfer gets a fresh full
  window starting *now*, instead of cargo-culting forward through however
  many idle windows elapsed. This is a deliberate simplicity/safety choice:
  it avoids unbounded loops on a stale `window_start` and avoids any
  ambiguity about which of several elapsed windows "counts."
- **Checked arithmetic throughout.** `admit_inbound` uses `checked_add` on
  the running total and rejects with an explicit "overflow" error rather
  than panicking or wrapping. Negative amounts are rejected outright, since
  inbound value is never negative in practice.
- **Rejections never mutate state.** A call that fails any check
  (negative amount, zero cap, cap exceeded, overflow) leaves
  `window_inbound_total` untouched, so a sequence of failed admission
  attempts can never partially consume the window.
- **Reconfiguration starts a clean window.** `set_inbound_cap` resets
  `window_start` to the given `current_time` and zeroes
  `window_inbound_total`. An operator raising or lowering the cap mid-window
  doesn't inherit whatever value was admitted under the old configuration.

### Testing and coverage

`inbound_cap_test.rs` covers:

| Scenario | Expected outcome |
|---|---|
| Fresh `Bridge`, cap never configured | **Rejected** — fail-closed default |
| Explicit `max_per_window = 0` | **Rejected**, including a `0`-value transfer |
| Inbound strictly under the cap | **Admitted**, accumulates correctly |
| Inbound that lands exactly on the cap | **Admitted** |
| Inbound that would exceed the cap | **Rejected**, running total unchanged |
| `current_time` crosses the window boundary | Window resets; previously-blocked amounts become admissible |
| Long idle gap (many window-lengths) before next call | Window realigns to `current_time`, no stale carry-over |
| Negative amount | **Rejected** |
| `set_inbound_cap` with `window_size == 0` | **Rejected** |
| `set_inbound_cap` with negative `max_per_window` | **Rejected** |
| Reconfiguring cap mid-window | Running total resets to `0`, new window starts at the given time |
| Running total at `i128::MAX` plus further inbound | **Rejected** — checked-add overflow guard, no panic |

Every conditional branch in `admit_inbound`, `set_inbound_cap`, and
`roll_window_if_expired` is exercised by at least one test above.
