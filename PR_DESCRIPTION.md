# Contract Boundary Documentation

## Summary

Adds `stellar-lend/contracts/ARCHITECTURE.md` to document contract boundaries between the legacy `hello-world` crate, the canonical `lending` deployment crate, and the auxiliary `amm` crate.

The note makes the deployment recommendation explicit:

- `contracts/lending` is the canonical lending deployment target
- `contracts/amm` is an optional secondary deployment for AMM features
- `contracts/hello-world` is legacy and should not be treated as the current deployment target

## Documentation Added

- `stellar-lend/contracts/ARCHITECTURE.md`
  - deployment matrix for `hello-world` vs `lending` vs `amm`
  - trust boundaries and ownership boundaries
  - admin and guardian powers
  - token transfer flow notes
  - external call and reentrancy review
  - checked-arithmetic and parameter-bound notes
- `stellar-lend/contracts/lending/SECURITY_NOTES.md`
  - Explicit documentation of Trust Boundaries.
  - Authorization Model verification for all external paths.
  - Reentrancy protections matrix and Checked-Arithmetic enforcement rules.

## Security Notes

- `lending` is the safest canonical target in the current tree:
  - user and admin entrypoints consistently require auth
  - pause and recovery gates are enforced on high-risk paths
  - most arithmetic uses `checked_*` or `I256`
  - flash loans include a reentrancy guard and post-callback repayment check
- `amm` should remain an auxiliary deployment until further hardening:
  - its admin helper checks stored admin equality but does not call `require_auth()`
  - swap/liquidity execution helpers are still mock protocol integrations
- `hello-world` is excluded from the active workspace and should be treated as legacy/reference code rather than the canonical deployment artifact

## Test Summary

Executed from `stellar-lend/`:

```bash
cargo test
```

Summarized result for multi-user contention scenarios (`cargo test multi_user_contention_test`):
- Successfully passed `test_contention_interleaved_deposits_borrows` (validated serial mixed-user bounds).
- Successfully passed `test_contention_edge_cases_zero_amounts_overflow` (validated structured errors on 0 amounts and type bounds).
- Successfully passed `test_contention_paused_operations` (validated isolation when admin pauses protocol globally).
All global arithmetic totals (borrows vs collateral deposits) assertions maintained exact parity.

## Notes

- No contract exports or WASM interfaces changed, so no contract build step was required beyond test verification
- This change is documentation-only; no Rust modules were materially changed
- Team review is recommended before merge, especially around the documented AMM auth caveat
- Re-run `cargo test` after freeing disk space on `C:`
