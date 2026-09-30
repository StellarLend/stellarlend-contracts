# Bridge Outbound Sequencing

## Overview

Every outbound bridge message is assigned a **monotonically increasing, per-destination nonce**.
This gives relayers and the destination chain a unique, ordered, replay-resistant identity for
each message.

## Per-Destination Monotonicity

The nonce ledger is a `Map<u32, u64>` keyed by destination network ID.  Each entry is
independent: advancing the nonce for destination A does not affect destination B.

| Property | Guarantee |
|---|---|
| First nonce per destination | Always `0` |
| Subsequent nonces | Strictly `prev + 1` |
| Rollover | Rejected — `BridgeError::NonceOverflow` |
| Cross-destination isolation | Independent sequences |

## Relayer Ordering Contract

1. **Uniqueness** — `(dest, nonce)` is globally unique.  No two outbound events share the same
   pair, so deduplication on the destination chain is straightforward.
2. **Ordering** — A nonce of `N` was emitted strictly before any nonce `> N` for the same
   destination.  Relayers can deliver messages in nonce order to preserve causality.
3. **Gap detection** — If a relayer observes nonce `N+2` before `N+1`, it knows a message is
   missing and can hold delivery until the gap is filled.
4. **Replay resistance** — Because the nonce is bound into the `OutboundMessageEvent`, a
   replayed event carries the same `(dest, nonce)` pair and can be rejected by the destination
   chain's inbound deduplication logic.

## API

```rust
/// Assign and return the next nonce for `dest`, then increment the ledger.
/// Returns BridgeError::NonceOverflow if the nonce would wrap past u64::MAX.
pub fn next_outbound_nonce(env: Env, dest: u32) -> Result<u64, BridgeError>;

/// Return the nonce that the next call to next_outbound_nonce will return,
/// without modifying state.
pub fn peek_outbound_nonce(env: Env, dest: u32) -> u64;
```

## Events

Each call to `next_outbound_nonce` emits an `OutboundMessageEvent`:

```rust
pub struct OutboundMessageEvent {
    pub dest: u32,   // destination network ID
    pub nonce: u64,  // nonce assigned to this message
}
```

Relayers subscribe to these events to build an ordered delivery queue per destination.
