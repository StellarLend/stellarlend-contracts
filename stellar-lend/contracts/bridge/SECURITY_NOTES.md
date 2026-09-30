# Security Notes — Bridge Validator Rotation

Threat model and mitigations

- Operator key compromise: Rotation requires a quorum proof signed by the *current* validator set. An operator private key compromise (single key) cannot rotate the set unless a quorum of current validators collude.
- Replay and downgrade: The `epoch` counter prevents accepting messages signed by retired validator sets (any signed_epoch < current epoch is rejected). Rotation requires epoch == current_epoch + 1, preventing out-of-order rotations.
- **Not-yet-active validator sets (#1147)**: `validate_inbound_epoch` also rejects any `signed_epoch > current_epoch` (see the dedicated section below). A naive lower-bound-only check would let an attacker pre-collect a message valid under a not-yet-rotated validator set and replay it once that future epoch actually arrives; the upper bound closes that window.
- Signature binding: The proof signs the serialized tuple `(new_set_bytes_vec, epoch)`, binding the new validator set to the specific epoch.

Implementation notes

- Quorum: uses strict supermajority (floor(2n/3)+1). This should be chosen to match protocol requirements; adjust if BFT tolerance differs.
- Serialization: validators stored as `Vec<Vec<u8>>` (raw public key bytes) to ensure deterministic encoding and avoid cross-crate serde issues.
- Atomicity: `rotate_validators` performs proof verification before swapping validators and advancing the epoch.
- **Validator-set size bounds**: Before verifying the quorum proof, `rotate_validators` validates that the deduplicated count of the incoming `new_set` lies within [`MIN_VALIDATORS`, `MAX_VALIDATORS`] (currently 3 and 32 respectively). Duplicate public keys are rejected outright — a set that relies on dedup to meet its size bound always indicates an operator error.

### Validator-set size bounds

`rotate_validators` enforces two pre-proof checks on every `new_set`:

1. **Size bounds**: the deduplicated validator count must be ≥ `MIN_VALIDATORS` (3) and ≤ `MAX_VALIDATORS` (32).
   - A 1‑ or 2‑validator set produces a supermajority threshold of 1 or 2 — too few for meaningful fault tolerance.
   - An empty set produces `threshold() = 1` over zero validators — an un‑securable state.
2. **Duplicate-key rejection**: the raw (pre‑dedup) list must contain no duplicate public‑key byte representations.

| Scenario | Expected outcome |
|---|---|
| Empty `new_set` | **Rejected** — `ValidatorSetTooSmall` |
| `new_set` with 1 unique validator | **Rejected** — `ValidatorSetTooSmall` |
| `new_set` with 2 unique validators | **Rejected** — `ValidatorSetTooSmall` |
| `new_set` exactly `MIN_VALIDATORS` (3) | **Accepted** (if quorum met) |
| `new_set` exactly `MAX_VALIDATORS` (32) | **Accepted** (if quorum met) |
| `new_set` > `MAX_VALIDATORS` (33+) | **Rejected** — `ValidatorSetTooLarge` |
| Duplicate public key bytes in `new_set` | **Rejected** — `DuplicateValidatorKey` |

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

### Not-yet-active validator-set rejection (#1147)

`validate_inbound_epoch` ALSO rejects any inbound message bearing
`signed_epoch > current_epoch + INBOUND_EPOCH_TOLERANCE` with
`"not-yet-active validator set"`. This closes a window that the retired-set
guard leaves open: an attacker who knows (or guesses) a future not-yet-rotated
validator set can produce a message well in advance, then surface it on this
bridge once the future epoch actually arrives — the message would already pass
`rotate_validators`-style checks because the future keys would be authoritative
by then, but the bridge would not otherwise have recorded it. By refusing the
message NOW, the bridge ensures any inbound under that future epoch must go
through a fresh submission path with the new keys, instead of replaying
pre-collected proof material.

**The tolerance is explicitly `0` (strict equality) by default.** Epochs are
discrete monotonically-incremented sequence numbers, not physical timestamps —
there is no realistic "clock skew" to absorb, and any positive tolerance
weakens replay resistance without justification. Raising
`INBOUND_EPOCH_TOLERANCE` is a security-sensitive change and must be paired
with a test update.

**Overflow is handled with `saturating_add`.** `signed_epoch = u64::MAX`
cannot weaponise an integer overflow into a comparison that panics or wraps
to a small value; the comparison is evaluated safely.

#### Formula

```text
min_accepted_epoch = self.epoch
max_accepted_epoch = self.epoch.saturating_add(INBOUND_EPOCH_TOLERANCE)

accepted iff  min_accepted_epoch <= signed_epoch <= max_accepted_epoch
```

#### Worked example

With `self.epoch = 5` and `INBOUND_EPOCH_TOLERANCE = 0`:

| `signed_epoch` | Outcome | Reason |
|---:|---|---|
| `3` | **Rejected** (`retired validator set`) | `< self.epoch` |
| `5` | **Accepted** | exactly the active epoch |
| `6` | **Rejected** (`not-yet-active`) | `> self.epoch + INBOUND_EPOCH_TOLERANCE` |
| `u64::MAX` | **Rejected** (`not-yet-active`) | strictly above the active epoch |

#### Tolerance expression

If `INBOUND_EPOCH_TOLERANCE` is ever raised to `N`:

| `signed_epoch` | Outcome (when `self.epoch == 5`) |
|---:|---|
| `5 + N` | **Accepted** (upper-bound inclusive) |
| `5 + N + 1` | **Rejected** (one above `max_accepted_epoch`) |

`N = 0` enforces strict equality. The bridge is shipped at `N = 0`; an
operator must re-justify, re-test, and document any change.

#### Coverage

`src/inbound_epoch_test.rs` covers, at a minimum,

| Scenario | Expected outcome |
|---|---|
| `signed_epoch` = `self.epoch - 1` | **Rejected** — retired validator set |
| `signed_epoch` = `self.epoch` | **Accepted** |
| `signed_epoch` = `self.epoch + 1` | **Rejected** — not-yet-active |
| `signed_epoch` = `self.epoch + 10⁹` | **Rejected** — not-yet-active |
| `signed_epoch` = `u64::MAX` | **Rejected** — not-yet-active |
| Error messages reference both `self.epoch` and `signed_epoch` | |
| `INBOUND_EPOCH_TOLERANCE` is observed to equal `0` at test time | |

### Multi-rotation correctness

Sequential rotations A → B → C → … produce a strictly monotonically increasing
epoch sequence. All epochs prior to the current one are rejected for inbound
messages.

### References

- `src/rotation_test.rs` — full test implementations.
- Before deployment, run integration tests and perform a security review
  comparing the on-chain encoding and off-chain signing tools.

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

For operator-facing parameter guidance and code-verified examples, see
[INBOUND_WINDOW_TUNING.md](./INBOUND_WINDOW_TUNING.md).

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

## Quorum-Proof Domain Separation (#1146)

### Threat

`verify_quorum_proof` originally had validators sign over
`bincode((new_set_bytes, epoch))`. That payload carries **no domain separator
and no bridge/chain identifier**, so a validator signature collected for one
bridge instance (or one purpose) could be replayed against another instance that
happens to share the same validator set and epoch — a cross-context
signature-reuse attack.

### Fix

The signed payload is now **domain-separated**. Signers and verifiers both build
it through `Bridge::quorum_proof_payload`:

```text
payload = bincode((
    QUORUM_PROOF_DOMAIN,   // constant purpose tag: b"stellarlend::bridge::quorum_proof::v1"
    bridge_id,             // per-instance id (set via Bridge::new_with_id)
    new_set_bytes,         // Vec<Vec<u8>> validator public keys
    epoch,                 // u64
))
```

- **`QUORUM_PROOF_DOMAIN`** (the *purpose* tag) ensures a signature produced for
  validator-set rotation can never be reinterpreted as a signature for some
  other context. The trailing `v1` lets the format be bumped to atomically
  invalidate all prior signatures.
- **`bridge_id`** (per-instance) ensures a proof gathered for instance A does not
  verify on instance B, even with an identical validator set and epoch.

`bincode` length-prefixes each field, so the encoding is unambiguous (no field
can "borrow" bytes from its neighbour).

### Worked example

Two bridges share validator set `S` and are rotating to set `S'` at epoch `1`:

| | bridge_id | payload prefix | result of A's signatures on B |
|---|---|---|---|
| Instance A | `bridge-A` | `(DOMAIN, "bridge-A", S', 1)` | — |
| Instance B | `bridge-B` | `(DOMAIN, "bridge-B", S', 1)` | **rejected** (different preimage → different hash → signature invalid) |

A signature over A's payload does not satisfy B's verification because the
`bridge_id` bytes differ, so `pk.verify(&payload_B, sig_over_A)` fails and the
proof never reaches quorum.

### Invariants preserved

The quorum-counting and duplicate-signer dedup logic is unchanged; only the
bytes being signed/verified changed. Old, un-tagged signatures no longer verify.

### Coverage

`src/domain_separation_test.rs`: correct-domain accepted; wrong-bridge-id
rejected; wrong-purpose tag rejected; legacy un-tagged signature rejected;
instance-A proof rejected on instance B; domain constant pinned.
