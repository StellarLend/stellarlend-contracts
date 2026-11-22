# Testnet Deployments

Network: Stellar **testnet** (Test SDF Network ; September 2015)

| Contract | ID | Explorer |
|---|---|---|
| stellarlend-lending (current) | `CC4A6A46O2KQA7KEMOFK3TQ4YRSLCJ4ZP26SQWXIKNF4DTEX3BOL2HF6` | [stellar.expert](https://stellar.expert/explorer/testnet/contract/CC4A6A46O2KQA7KEMOFK3TQ4YRSLCJ4ZP26SQWXIKNF4DTEX3BOL2HF6) |
| stellarlend-lending v1 (deprecated) | `CAAD3BFVIQENIUV6MDCI76FHI5XFQ3ZNZFLTTRBOVDLINEFXNOABWPIH` | [stellar.expert](https://stellar.expert/explorer/testnet/contract/CAAD3BFVIQENIUV6MDCI76FHI5XFQ3ZNZFLTTRBOVDLINEFXNOABWPIH) |

Admin / deployer: `GAH5NAPXT6T4Z6VUGGVIVTUT6I6JNDMBTKA4VYIUYZLTNZ2FW6NRGLJX`  
Demo user: `GBS6O6RWCY7NYVC5KL3YBPPSLXWLSV2OP4KNXJNKWCBWLMJNL5IN3NE6`

v1 is deprecated: its per-ledger borrow-rate cache made the transaction footprint depend on the closing ledger, so `borrow` simulated fine but trapped on-chain. The current deployment removes that cache.

## Verified on-chain flow (current contract)

initialize -> deposit 1,000,000 -> borrow 100,000 -> repay 50,000 -> withdraw 100,000 -> `get_position` returned
`{"collateral":"900000","debt":"50000","health_factor":"144000"}`.

Transactions: [initialize](https://stellar.expert/explorer/testnet/tx/c5a0e6d2872b4a4d1804c39dcf41eb5cbb06cfc5b24e1b27f0b645663a1b9cf9),
[deposit](https://stellar.expert/explorer/testnet/tx/94da3c85b4db06839e8216aeb20e62c98011145c1567f2e9d38228e46d2a0f46),
[borrow](https://stellar.expert/explorer/testnet/tx/edcb603278567f4433d43b1ff12b950d0cce2f39665896b639f3f882191f8fd6),
[repay](https://stellar.expert/explorer/testnet/tx/8c93241a604cdee05d40f51ba1efe77c773dab5546a04259082c574b840b48be),
[withdraw](https://stellar.expert/explorer/testnet/tx/46808a869cecffc60181acffbba876e77f468ab3c78cc286b64b54b021461853).
