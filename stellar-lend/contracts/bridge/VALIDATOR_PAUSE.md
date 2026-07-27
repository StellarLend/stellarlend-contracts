# Per-Validator Signing Pause (Issue #1221)

This document describes the per-validator pause feature added to
`stellar-lend/contracts/bridge/src/lib.rs`. It explains the threat model the
feature is built against, the public-facing API, the fail-closed math used
to prevent the bridge from being frozen by an overly-aggressive guardian,
and the on-the-wire shape of the events emitted on pause / unpause.

## Why this feature exists

StellarLend's bridge contract authenticates validator-set changes via a
strict supermajority > 2/3 quorum proof. If a single validator key is
compromised, the safest recovery is to *rotate the entire validator set*
through a quorum vote. But quorum-of-current-validators requires the
*current* set to reach threshold, which a single compromised but otherwise
honest minority cannot obstruct — except operationally, a compromised key
is a *known* problem and the bridge operators want to be able to act faster
than the next rotation cycle.

This feature lets the bridge's guardian exclude a single compromised
validator from quorum counting immediately, **without rotating the full
set**. The remaining active validators continue running the bridge at the
new (smaller) effective threshold while a proper key rotation is
planned and executed on a normal timeline.

## Public API

The feature exposes three new methods on `Bridge`, plus a small set of
introspectors and one new typed error type, all defined in `src/lib.rs`.

### Guardian configuration

```rust
pub fn set_guardian(&mut self, guardian: PublicKey);
pub fn guardian(&self) -> Option<&PublicKey>;
```

- `set_guardian` is **not** signature-protected. The bridge is a pure-Rust
  data structure; the trust assumption is that whoever holds the
  `&mut Bridge` is also trusted to configure its guardian, similarly to
  how the validator set, inbound cap, and window size are configured.
  Operational guidance: call `set_guardian` exactly once, on a trusted host,
  immediately after `Bridge::new`. The companion toggles
  (`pause_validator`, `unpause_validator`) are signature-protected, so a
  later attempt to exfiltrate the bridge by rewriting the guardian still
  requires possession of the current guardian's signing key.

### Pause / unpause

```rust
pub fn pause_validator(&mut self, validator: &PublicKey, signature: &Signature)
    -> Result<ValidatorEvent>;

pub fn unpause_validator(&mut self, validator: &PublicKey, signature: &Signature)
    -> Result<ValidatorEvent>;
```

Both return a typed [``ValidatorEvent`] describing what just happened,
including the byte-encoding of the affected validator key and the bridge
epoch at the time the action took effect. The caller is expected to
serialize or log these events so downstream tooling can react (alert,
re-rotate, etc.).

### Event type

```rust
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ValidatorEvent {
    Paused   { validator: Vec<u8>, epoch: u64 },
    Unpaused { validator: Vec<u8>, epoch: u64 },
}
```

Events are returned from the operation rather than published through a
host-managed event bus, since this crate is off-chain Rust and does not
have access to a Soroban host. The `Serialize` / `Deserialize` impls let
callers encode events with their preferred format (JSON, bincode, etc.).

### Introspectors

```rust
pub fn is_paused(&self, pk: &PublicKey) -> bool;
pub fn is_active_validator(&self, pk: &PublicKey) -> bool;
pub fn active_validator_count(&self) -> usize;
pub fn effective_threshold(&self) -> usize;
pub fn paused_list(&self) -> Vec<Vec<u8>>;
```

- `active_validator_count` and `effective_threshold` are the on-the-wire
  versions of the count and threshold numbers used in
  `verify_quorum_proof`. They are recomputed dynamically so a caller can
  audit the live quorum math at any time.
- `is_active_validator` reports whether a given key would count toward
  quorum right now (i.e., is in the validator set AND not paused).
- `paused_list` returns the raw byte encodings of all currently paused
  keys in arbitrary set-iteration order.

## Authorisation: signature payload binding

A pause or unpause signature from the guardian must verify over a
domain-separated payload that binds the authorisation to (action, target):

| Action     | Payload prefix           | Bound target            |
|-----------|--------------------------|-------------------------|
| `pause`   | `"BRIDGE_PAUSE:"`        | `validator.to_bytes()` |
| `unpause` | `"BRIDGE_UNPAUSE:"`      | `validator.to_bytes()` |

Because the two prefixes differ, a `pause(A)` signature cannot be replayed
as `unpause(A)`, and the per-target binding means `pause(A)` cannot be
replayed as `pause(B)`. Both replay vectors are tested in
`validator_pause_test.rs`:

- `pause_signature_cannot_be_replayed_as_unpause`
- `pause_signature_for_a_cannot_be_replayed_for_b`

Similarly, a signature whose signer is not the configured guardian is
rejected with `BridgeError::InvalidGuardianSignature` (the signature is
checked after the fail-closed arithmetic, so a malicious caller cannot
burn a guardian signature on a request that would have been rejected on
quorum-math grounds anyway).

## Fail-closed semantics

`Bridge::pause_validator` is invoked with a target validator and a
guardian signature. Before verifying the signature, we compute what
the *post-pause* active validator count would be and reject if it would
fall below the new effective supermajority threshold. Concretely:

```
let current_active = bridge.active_validator_count();
let new_active     = current_active - 1;       // pausing reduces by exactly one
let new_threshold  = (new_active * 2) / 3 + 1; // supermajority of the remaining
if new_active < new_threshold {
    return Err(PauseWouldBreakQuorum);
}
```

This protects the bridge from being frozen by an overly aggressive
guardian — the bridge prefers to remain live with a known-compromised
key over freezing itself.

| n (current set) | Old threshold | Pausing 1 ⇒ new active | New threshold | Pausing 1 allowed? |
|----|------|------|------|------|
| 3  | 3    | 2    | 2    | ✅ (2 >= 2) |
| 4  | 3    | 3    | 3    | ✅ (3 >= 3) |
| 5  | 4    | 4    | 3    | ✅ |
| 6  | 5    | 5    | 4    | ✅ |
| …  | …    | …    | …    | ✅ until active count hits threshold |
| 2  | 2    | 1    | 1    | ✅ (1 >= 1) |
| 1  | 1    | 0    | 1    | ❌ (0 < 1) — fail-closed |
| 0  | 1    | 0    | 1    | ❌ (0 < 1) — unreachable via pause |

The fail-closed check is enforced *before* consuming the guardian's
signature, so a paused validator request that would have been rejected
on quorum-math grounds does not leak signature material via rejected-call
logs.

## Effect on quorum verification

`Bridge::verify_quorum_proof` is the workhorse function for all rotation
proofs; this feature modifies it in two ways:

1. **Paused signers are silently skipped.** When the loop encounters a
   signer whose byte-encoding is in `paused_validators`, it neither
   verifies the signature nor counts it toward quorum. This is a
   deliberate non-error: a paused signer may still appear in relay-
   network gossip, and rejecting an otherwise-valid proof solely because
   of a stale signature from a paused key would let an attacker (who
   possesses the compromised key but no longer any operational standing)
   perform a denial-of-service on bridge rotations. Skipping is the
   correct BFT-flavoured response to a known-untrusted voter.

2. **Quorum threshold is recomputed against active validators.** A
   `Bridge` with 4 validators and 1 paused has effective
   `threshold() = 3`, so quorum can be reached with 3 of the 3
   remaining active signers (the formerly-paused signer's vote is
   ignored). The original `ValidatorSet::threshold()` is unchanged and
   is preserved for callers that want the unpaused-baseline number.

The combined effect: after a non-breaking pause, a bridge can still
rotate to a new validator set using the (smaller) effective threshold,
without waiting for the compromised key to be removed from the
validator set proper.

## Effect on `rotate_validators`

`Bridge::rotate_validators` clears `paused_validators` on success. This
is intentional: pause flags are scoped to the *compromised key material*
in the *current* validator set, and the *new* set implies fresh,
unpaused keys by default. If a key from the old set happens to also be
present in the new set, that is a configuration choice the operator
must make explicitly, via a subsequent `pause_validator` call.

## Error types

All typed errors carry an explicit variant so callers (deployment
scripts, alerting tooling) can match on them rather than substring-
matching error strings. See

```rust
pub enum BridgeError {
    InvalidWindowSize,
    NoGuardianConfigured,
    InvalidGuardianSignature,
    UnknownValidator,
    PauseWouldBreakQuorum,
    AlreadyPaused,
    NotPaused,
}
```

The new variants are appended without disturbing existing ones. The
existing `Display` impl for `Display` and the in-crate
`std::error::Error` trait impl cover the new variants too.

## Edge cases covered by the test suite

`src/validator_pause_test.rs` exercises every documented branch above:

| Scenario | Test |
|---|---|
| Default state has no guardian and empty paused set | `fresh_bridge_has_no_guardian_and_empty_paused_set` |
| Pause / unpause reject without a configured guardian | `pause_rejects_without_configured_guardian`, `unpause_rejects_without_configured_guardian` |
| Pause / unpause reject on invalid guardian signature | `pause_rejects_if_signature_does_not_verify_against_guardian`, `unpause_rejects_if_signature_does_not_verify_against_guardian` |
| Pause signature cannot be replayed as unpause (and vice versa) | `pause_signature_cannot_be_replayed_as_unpause` |
| Pause(A) signature cannot be replayed as pause(B) | `pause_signature_for_a_cannot_be_replayed_for_b` |
| Double-pause is rejected (idempotent rejection) | `pause_rejects_when_already_paused` |
| Unpause of non-paused is rejected | `unpause_rejects_when_not_paused` |
| Pause / unpause reject unknown validator | `pause_rejects_unknown_validator`, `unpause_rejects_unknown_validator` |
| Pause rejected when active count would drop below new threshold | `pause_rejected_when_active_count_would_fall_below_new_threshold` |
| Pause accepted when active count still meets new threshold | `pause_accepted_when_active_count_meets_new_threshold` |
| Pause-and-pause-and-pause-and-pause — only the last one (which would leave zero active) is rejected | `pause_rejected_only_when_quorum_becomes_unreachable` |
| Rotation succeeds with reduced threshold after pause | `rotation_with_one_paused_validator_uses_new_threshold`, `rotation_uses_effective_threshold_for_size_check` |
| Rotation rejects when active signers < new threshold | `rotation_rejects_when_active_signers_below_new_threshold` |
| Malformed signature from a paused validator doesn't poison the proof | `malformed_paused_signature_does_not_break_proof` |
| Paused signers don't inflate duplicate counts | `paused_signer_does_not_count_as_unique_vote` |
| Round-trip pause → unpause restores active count | `pause_then_unpause_restores_active_count_and_threshold` |
| Rotation clears the paused set on success | `rotation_clears_paused_set` |
| `paused_list` returns the byte-encodings of paused keys | `paused_list_returns_byte_encodings_of_paused_keys` |
| `is_active_validator` returns correct status | `is_active_validator_returns_false_for_paused_or_unknown` |
| `ValidatorEvent` Display includes epoch and hex pk | `validator_event_display_includes_epoch_and_hex_pk` |

## Operational guidance for maintainers

- **Configure the guardian once, on a trusted host.** Replacing the
  guardian afterwards is currently an unauthenticated operation; if you
  need a two-step handover, build it on top of `set_guardian`.
- **Use the fail-closed check as a tool, not an obstacle.** The
  `PauseWouldBreakQuorum` guard is meant for "pause a single compromised
  validator" operations, not "paused-majority" operations. If you need to
  pause more than a minority and the math forbids it, schedule a proper
  rotation (using the still-active quorum).
- **Log every ValidatorEvent.** Pause and unpause operations are rare but
  security-critical; every successful (and rejected) event should make
  it to an audit pipeline so security teams can correlate with
  compromised-key disclosures from operators.
- **Re-check the threshold after a pause.** A pause lowers the
  effective quorum threshold. If the bridge was previously operating
  on the assumption `threshold = floor(2n/3) + 1` for the *original*
  `n`, downstream tooling that hard-codes a signer count must be
  reconfigured to use `effective_threshold()` instead.

## See also

- `src/lib.rs` — implementation (`pause_validator`, `unpause_validator`,
  `effective_threshold`, `ValidatorEvent`).
- `src/validator_pause_test.rs` — full test coverage.
- `src/rotation_test.rs` — quorum-proof tests that the new
  `verify_quorum_proof` rules apply to.
- `SECURITY_NOTES.md` — broader bridge threat model.
- `WINDOW_GUARD.md` — pattern for fail-closed inbound-value caps, which
  we follow for `pause` (prefer to stay live with a known-compromised
  key than freeze the bridge).
- `VALIDATORSET_INVARIANTS.md` — how deduplicated validator counts feed
  both the original `threshold` and the new `effective_threshold`.
