//! Rounding strategy for lending math operations.
///
//# Invariants
//- All rounding operations are deterministic and pure (no state, no time, no environment dependencies).
//- Rounding is always applied in the direction that protects the protocol (favor the pool):
//  - debt increases round up,
//  - collateral/balance decreases round down,
//  - interest accrual rounds up.
//- Zero denominators are rejected with a deterministic error rather than panicking.
//- Overflow is detected and reported as an error (no silent wrapping).
///
//# Failure modes
//- `RoundingError::DivisionByZero` when a denominator is zero.
//- `RoundingError::Overflow` when an intermediate or final result exceeds the representable range.
///
//# Retries / concurrency
///This module is stateless and pure. Retries and concurrent invocations produce identical results for identical inputs.

use core::fmt;

/// Errors returned by rounding operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
put error attr here