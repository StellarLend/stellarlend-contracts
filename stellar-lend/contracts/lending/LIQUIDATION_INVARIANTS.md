# Liquidation Sequence Invariants

This note documents the new sequence-level invariant coverage for the lending liquidation path.

## Proven invariants

- The cumulative collateral seized across a sequence of liquidations never exceeds the cumulative repaid amount multiplied by the liquidation incentive factor.
- The borrower collateral balance and debt balance remain non-negative throughout the sequence.
- Repeated liquidations converge to a zero or fully unwinded position without introducing collateral or debt underflow.

## Test harness

The test suite in `src/liquidation_sequence_invariant_test.rs` runs seeded, multi-step operation sequences against the live Soroban contract state. Each case starts from a fresh contract instance and exercises at least three liquidations per run so the invariant is checked across a real sequence rather than a single call.
