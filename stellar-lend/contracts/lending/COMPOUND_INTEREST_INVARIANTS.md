# Compound Interest Invariants

## Rationale

This document describes the property-based invariants for `compute_compound_interest`.

The property tests verify that:

- Interest never decreases as elapsed time increases.
- Interest never decreases as the interest rate increases.
- Interest is never negative.
- The function never panics for arbitrary inputs.
- Invalid inputs return a typed `MathError`.
- Arithmetic overflow is reported as `MathError::Overflow`.

## Worked Example

Given:

- Principal = 1,000,000
- Rate = 1,000 basis points (10%)
- Elapsed = 31,536,000 seconds (1 year)

Expected interest:

    principal * rate * elapsed / (BPS_SCALE * SECONDS_PER_YEAR)

using the implementation's checked arithmetic.

## Edge Cases

# Compound Interest Invariants

## Rationale

This document describes the property-based invariants for `compute_compound_interest`.

The property tests verify that:

- Interest never decreases as elapsed time increases.
- Interest never decreases as the interest rate increases.
- Interest is never negative.
- The function never panics for arbitrary inputs.
- Invalid inputs return a typed `MathError`.
- Arithmetic overflow is reported as `MathError::Overflow`.

## Worked Example

Given:

- Principal = 1,000,000
- Rate = 1,000 basis points (10%)
- Elapsed = 31,536,000 seconds (1 year)

Expected interest:

    principal * rate * elapsed / (BPS_SCALE * SECONDS_PER_YEAR)

using the implementation's checked arithmetic.

## Edge Cases

The property suite explicitly covers:

- Zero principal
- Zero elapsed time
- Zero interest rate
- Maximum allowed rate
- Very large principal values
- Overflow conditions
- Invalid rate values
- Randomized inputs across the valid domain
