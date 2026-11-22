# Outbound Message Format

## Overview

This document specifies the byte-level format of outbound bridge messages emitted by the `send()` entrypoint. The message hash is computed deterministically from the message fields and is included in the `OutboundSentEvent` for verification by relayers and destination chains.

## Message Hash Computation

The message hash is computed as:

```text
message_hash = SHA-256(
    OUTBOUND_MSG_DOMAIN
    || bridge_id
    || dest_chain (4 bytes LE)
    || nonce (8 bytes LE)
    || sender (XDR-encoded Address)
    || recipient
    || token (XDR-encoded Address)
    || amount (16 bytes LE)
)
```

### Domain Separator

```rust
const OUTBOUND_MSG_DOMAIN: &[u8] = b"stellarlend::bridge::outbound_msg::v1";
```

The domain separator prevents hash collisions with other message types (inbound, quorum proofs, etc.) and ties the message to this specific bridge implementation version.

## Field Layout

| Field | Type | Encoding | Size | Notes |
|-------|------|----------|------|-------|
| Domain | const | UTF-8 bytes | 41 bytes | `b"stellarlend::bridge::outbound_msg::v1"` |
| Bridge ID | `Bytes` | Variable | Variable | Configured at bridge initialization |
| dest_chain | `u32` | 4 bytes LE | 4 bytes | Numeric destination chain identifier |
| nonce | `u64` | 8 bytes LE | 8 bytes | Per-destination monotonic sequence |
| sender | `Address` | XDR | Variable | Soroban Address XDR encoding |
| recipient | `Bytes` | Raw bytes | 1-128 bytes | Foreign chain address (opaque) |
| token | `Address` | XDR | Variable | Token contract address (XDR) |
| amount | `i128` | 16 bytes LE | 16 bytes | Transfer amount in token's smallest unit |

### Endianness

- **Little-endian (LE)** is used for multi-byte numeric fields (`dest_chain`, `nonce`, `amount`) to match Soroban SDK conventions and simplify relayer implementations on common architectures.

### Variable-Length Fields

#### Bridge ID
The bridge ID is a `Bytes` value configured at bridge initialization. It uniquely identifies this bridge instance and is concatenated directly into the message (no length prefix needed since we're hashing the entire structure).

#### Sender and Token (XDR)
The sender and token `Address` values are encoded using Soroban's XDR serialization via `Address::to_xdr(env)`. The XDR format includes its own length encoding, so the resulting bytes are self-describing.

#### Recipient
The recipient is an opaque byte string representing an address on the destination chain. The format and structure are foreign to Stellar and are not interpreted by this contract. The recipient length is bounded:
- **Minimum:** 1 byte (`RecipientEmpty` error if empty)
- **Maximum:** 128 bytes (`RecipientTooLong` error if exceeded; see `MAX_RECIPIENT_LEN`)

These bounds prevent oversized events and ensure the hash input stays within reasonable limits.

## Verification by Destination Chain

The destination chain's inbound bridge contract should:

1. **Recompute the message hash** using the same layout and encoding.
2. **Verify the hash** matches the one signed by the validator set.
3. **Check the nonce** is the expected next value for the source chain (prevents replay and out-of-order delivery).
4. **Decode the sender and token** using XDR deserialization if needed for accounting or authorization.
5. **Interpret the recipient** according to the destination chain's address format (e.g., Ethereum 20-byte hex, Solana base58, etc.).

## Example

For a message with:
- bridge_id = `b"stellarlend_mainnet_bridge_v1"`
- dest_chain = `42`
- nonce = `7`
- sender = (some XDR-encoded Address)
- recipient = `b"0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb1"` (42 bytes)
- token = (some XDR-encoded Address)
- amount = `1000000` (1 million in smallest unit, as i128)

The hash input would be:

```
b"stellarlend::bridge::outbound_msg::v1"  // 41 bytes
b"stellarlend_mainnet_bridge_v1"          // 30 bytes
[42, 0, 0, 0]                              // dest_chain (LE)
[7, 0, 0, 0, 0, 0, 0, 0]                   // nonce (LE)
<sender XDR bytes>                         // variable
b"0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb1"  // 42 bytes
<token XDR bytes>                          // variable
[64, 66, 15, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]  // amount=1000000 (LE i128)
```

The SHA-256 of this concatenated byte sequence is the `message_hash` emitted in the event and verified on the destination side.

## Security Considerations

- **Unique hashes:** The combination of domain separator, bridge ID, dest_chain, and nonce ensures every outbound message has a globally unique hash, even if the same user sends the same amount to the same recipient multiple times.
- **Replay protection:** The nonce is monotonically increasing per destination and is part of the hash, so replaying an old message signature will be rejected (destination chain has already consumed that nonce).
- **Integrity:** Any tampering with the message fields (amount, recipient, token, etc.) will result in a different hash, which will fail signature verification on the destination chain.

## Related Documents

- [OUTBOUND_SEQUENCING.md](OUTBOUND_SEQUENCING.md) — Nonce allocation and ordering guarantees
- [OUTBOUND_CAP.md](OUTBOUND_CAP.md) — Rate limiting for outbound transfers
- [README.md](README.md) — Bridge architecture overview
