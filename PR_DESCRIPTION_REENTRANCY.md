# Reentrancy Module Audit And Documentation

## Summary

Audits the `hello-world` contract reentrancy guard against Soroban's synchronous cross-contract execution model, versions the temporary lock key, and adds focused tests plus contract-crate documentation.

## Changes

- documented `src/reentrancy.rs` with explicit guarantees, limits, and security notes,
- replaced the string temporary-storage key with a versioned `ReentrancyDataKey::LockV1`,
- added tests for lock acquisition/release, callback-driven re-entry rejection, and entrypoint error mapping,
- added [`stellar-lend/contracts/hello-world/REENTRANCY.md`](./stellar-lend/contracts/hello-world/REENTRANCY.md),
- linked crate documentation back to the reentrancy audit notes.

## Security Notes

- The guard protects only same-transaction nested entry on the same contract instance.
- It is defense in depth; authorization, pause switches, and collateral checks remain mandatory.
- Token contracts are treated as untrusted and may attempt callback-based re-entry during `transfer` or `transfer_from`.
- `borrow_asset` keeps the guard in place for production token transfers; tests verify lock rejection at the entrypoint because the token transfer branch is compiled out in unit tests.

## Test Output

Run:

```bash
cargo test -p hello-world test_reentrancy --lib
```

Summary:

```text
All reentrancy tests passed locally.
```
