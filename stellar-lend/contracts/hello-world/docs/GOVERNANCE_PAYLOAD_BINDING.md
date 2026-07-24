# Governance Proposal Payload Binding (#1120)

## Problem

`gov_create_proposal` / `gov_execute_proposal` track a proposal through vote and
queue, but the executed action was **not cryptographically bound** to what
voters approved. If the action payload is resolved at execution time, a
privileged caller could queue one action and execute a different one — breaking
the "what you voted for is what runs" guarantee.

## Solution

Record a hash of the canonical proposal payload at **creation**, and verify it
at **execution**. Execution recomputes the hash of the action it is about to run
and refuses to proceed on any mismatch.

The binding lives in [`src/governance.rs`](../src/governance.rs):

| Function | When | Purpose |
|----------|------|---------|
| `bind_payload(env, &payload) -> BytesN<32>` | `gov_create_proposal` | compute + return the hash to persist with the proposal |
| `verify_payload(env, &bound_hash, &payload) -> Result<(), PayloadBindingError>` | `gov_execute_proposal` | recompute and reject on mismatch (`PayloadMismatch`) |
| `compute_payload_hash(env, &payload) -> BytesN<32>` | — | underlying hash used by both |

## Formula

```text
preimage = be32(len(target)) ‖ target
         ‖ be32(len(params)) ‖ params
         ‖ be64(proposal_id)

payload_hash = keccak256(preimage)
```

Where `be32` / `be64` are 4- and 8-byte big-endian encodings and `‖` is byte
concatenation.

### Why the hash covers target, params, **and** proposal id

- **target + params** — these fully describe *what runs*. Changing either at
  execution time changes the hash and is rejected.
- **proposal_id** — folded in so the same approved action bound to proposal `A`
  cannot be replayed under proposal `B` (cross-proposal replay / aliasing).

### Why each field is length-prefixed

A naive `target ‖ params` concatenation is **not injective**: `("ab","")` and
`("a","b")` produce the same bytes and therefore the same hash, letting an
attacker substitute one action for another. Prefixing each variable-length field
with its length makes the encoding injective, so distinct `(target, params)`
pairs always hash differently.

## Worked example

Action: call `set_rate` with the 4-byte param `0x00000005`, bound to proposal
`1`.

```text
target       = "set_rate"            (8 bytes)
params       = 00 00 00 05           (4 bytes)
proposal_id  = 1

preimage     = 00 00 00 08           # be32(len(target)=8)
             ‖ 73 65 74 5f 72 61 74 65   # "set_rate"
             ‖ 00 00 00 04           # be32(len(params)=4)
             ‖ 00 00 00 05           # params
             ‖ 00 00 00 00 00 00 00 01   # be64(proposal_id=1)

payload_hash = keccak256(preimage)   # stored at creation
```

At execution, the executor rebuilds the payload for the action it is about to
run and calls `verify_payload`:

- Same `("set_rate", 0x00000005, 1)` → recomputed hash equals stored hash → `Ok(())`, execution proceeds.
- Param changed to `0x00000006` → different hash → `Err(PayloadMismatch)`, execution aborts.
- Target changed to `drain_treasury` → different hash → `Err(PayloadMismatch)`.
- Same action replayed under proposal `2` → different hash → `Err(PayloadMismatch)`.

## Tests

[`src/gov_payload_hash_test.rs`](../src/gov_payload_hash_test.rs) covers:

- matching payload verifies,
- mutated param rejected,
- substituted target rejected,
- replay across proposal ids rejected,
- hash determinism,
- length-prefix aliasing resistance (`("ab","")` ≠ `("a","b")`).

Run: `cargo test -p hello-world gov`

## Integration note

The full proposal/vote/queue/execute lifecycle in this crate is currently
behind stubbed modules (replaced with minimal stubs to keep CI green). This
module provides the cryptographic binding primitive so it can be wired into
`gov_create_proposal` (store `bind_payload(...)`) and `gov_execute_proposal`
(gate on `verify_payload(...)`) once that lifecycle is restored.
