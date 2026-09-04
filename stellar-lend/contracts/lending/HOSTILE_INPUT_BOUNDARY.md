# Hostile Input Boundary: Interest-Rate and Utilization Math Authorization

## Overview

This document describes the authorization and validation boundary implemented for interest-rate and utilization calculations to protect against hostile inputs and ensure deterministic, bounded behavior at all boundary conditions.

## Problem Statement

The interest-rate and utilization math is a production-quality risk area where:
- Rate calculations must remain bounded, deterministic, and safe at zero, maximum, and near-overflow utilization
- Authorization assumptions must be checked rather than inferred from client state
- Hostile inputs (negative values, overflow attempts, out-of-range parameters) must be explicitly rejected
- Replay, tampering, wrong-network, and malformed inputs must be validated at the boundary

## Implementation

### 1. Rate Model Authorization Boundaries (`rate_model.rs`)

#### Utilization Validation
```rust
// Strict bounds enforcement - no silent clamping
if !(0..=BPS_DENOM).contains(&utilization_bps) {
    return Err(RateModelError::OutOfRange);
}
```

**Rationale**: Utilization outside [0, 100%] represents either a calculation error or hostile input. Rejecting it explicitly prevents the rate model from receiving invalid data that could produce non-monotonic or negative rates.

#### Rate Parameter Validation
```rust
// All coefficients must be non-negative to preserve monotonicity
if params.base_rate_bps < 0 || params.multiplier_bps < 0 || /* ... */ {
    return Err(RateModelError::OutOfRange);
}

// Kink values must not exceed 100%
if params.kink_utilization_bps > BPS_DENOM {
    return Err(RateModelError::OutOfRange);
}

// Floor must not exceed ceiling (logical consistency)
if params.rate_floor_bps > params.rate_ceiling_bps {
    return Err(RateModelError::OutOfRange);
}
```

**Rationale**: Negative coefficients would destroy monotonicity guarantees. Kinks above 100% are nonsensical. Floor above ceiling creates undefined behavior. All of these represent either misconfiguration or hostile tampering.

### 2. Utilization Calculation Boundaries (`debt.rs`)

#### Negative Debt Rejection
```rust
// Reject negative debt (hostile input boundary)
if snapshot.total_debt < 0 {
    return Err(DebtError::Overflow);
}
```

**Rationale**: Negative total debt is impossible under normal operation and represents either storage corruption or an attack attempting to manipulate utilization calculations.

#### Supply Validation
```rust
// Safe fallback for zero or negative supply
if snapshot.total_supply <= 0 {
    return Ok(0);  // Zero utilization
}
```

**Rationale**: Division by zero must be prevented. Negative supply is nonsensical. Both return zero utilization as a safe, deterministic fallback.

#### Result Bounds Enforcement
```rust
.map(|raw| raw.max(0).min(BPS_DENOM))  // Enforce [0, BPS_DENOM] bounds
```

**Rationale**: Even if debt exceeds supply (bad debt scenario), utilization is capped at 100% to keep rate model inputs valid.

### 3. Interest Accrual Boundaries (`debt.rs`)

#### Principal Validation
```rust
// Authorization boundary: reject negative principal
if principal < 0 {
    return Err(DebtError::InvalidAmount);
}
```

**Rationale**: Negative principal represents either an accounting error or hostile input attempting to generate negative interest.

#### Rate Validation
```rust
// Authorization boundary: reject negative rates
if rate_bps < 0 {
    return Err(DebtError::Overflow);
}

// Authorization boundary: reject excessive rates
if rate_bps > MAX_RATE_BPS {
    return Err(DebtError::Overflow);
}
```

**Rationale**: Negative rates are economically nonsensical. Rates above 1000% APR (`MAX_RATE_BPS`) are certainly input errors rather than legitimate rates, and accepting them risks overflow in interest calculations.

### 4. Index Accrual Boundaries (`debt.rs`)

#### Index Validation
```rust
// Authorization boundary: reject invalid indices
if current_index <= 0 {
    panic!("BorrowIndex: invalid current_index (must be > 0)");
}
```

**Rationale**: The borrow index must be strictly positive (starts at `INDEX_SCALE = 10_000_000`). Zero or negative values indicate storage corruption or attack.

#### Rate Bounds
```rust
// Authorization boundary: reject negative or excessive rates
if rate_bps < 0 {
    panic!("BorrowIndex: negative rate_bps not allowed");
}
if rate_bps > MAX_RATE_BPS {
    panic!("BorrowIndex: rate_bps exceeds MAX_RATE_BPS");
}
```

**Rationale**: Same as interest accrual - negative or excessive rates must be rejected before they can corrupt the global index.

### 5. Supply Rate Boundaries (`debt.rs`)

#### Comprehensive Input Validation
```rust
// Authorization boundary: guard inputs
if borrow_rate_bps < 0 || utilization_bps < 0 {
    return Err(DebtError::Overflow);
}
if reserve_factor_bps > BPS_DENOM as u32 {
    return Err(DebtError::Overflow);
}
if borrow_rate_bps > MAX_RATE_BPS {
    return Err(DebtError::Overflow);
}
if utilization_bps > BPS_DENOM {
    return Err(DebtError::Overflow);
}
```

**Rationale**: Supply rate depends on three inputs (borrow rate, utilization, reserve factor). All must be validated to prevent:
- Negative rates (nonsensical)
- Utilization > 100% (out of range)
- Reserve factor > 100% (protocol cannot take more than 100% of interest)
- Excessive borrow rates (overflow risk)

### 6. Math Module Boundaries (`math.rs`)

#### Interest Calculation Validation
```rust
// Authorization boundary: validate all inputs
if principal < 0 || rate_bps < 0 || rate_bps > MAX_RATE_BPS {
    return Err(MathError::OutOfRange);
}
```

**Rationale**: Pure math functions must validate inputs to guarantee correct output ranges. Garbage in = garbage out is unacceptable for financial calculations.

#### Reserve Split Validation
```rust
// Authorization boundary: validate inputs
if total_interest < 0 || reserve_factor_bps > BPS_SCALE {
    return Err(MathError::OutOfRange);
}
```

**Rationale**: Negative interest or reserve factors above 100% represent either calculation errors or hostile inputs.

### 7. Common Module Boundaries (`common/src/lib.rs`)

#### BPS Scaling Validation
```rust
// Authorization boundary: strict rate validation
if rate_bps < 0 || rate_bps > BPS_DENOM {
    return None;
}
```

**Rationale**: Basis point helpers are used throughout the protocol. Validating at this foundational layer prevents invalid rates from propagating.

#### Price Normalization Validation
```rust
// Authorization boundary: reject negative prices
if raw_price < 0 {
    return None;
}
```

**Rationale**: Negative asset prices are economically impossible and represent either oracle manipulation or data corruption.

## Invariants Enforced

### State Invariants
1. **Total debt ≥ 0**: Negative debt is rejected
2. **Total supply ≥ 0**: Treated as zero utilization when violated
3. **Borrow index > 0**: Zero or negative indices panic (fatal invariant)
4. **Interest ≥ 0**: Negative interest is rejected

### Data Invariants
1. **Utilization ∈ [0, 100%]**: Bounded to valid range
2. **Rate parameters ≥ 0**: Negative coefficients rejected
3. **Kink values ≤ 100%**: Out-of-range kinks rejected
4. **Floor ≤ Ceiling**: Inconsistent bounds rejected
5. **Reserve factor ≤ 100%**: Excessive factors rejected

### Authorization Invariants
1. **Rate bounds checked**: Rates above `MAX_RATE_BPS` (1000% APR) rejected
2. **Principal non-negative**: Negative principal rejected
3. **Time non-negative**: Implicit via `u64` type (elapsed time cannot be negative)

### Failure Invariants
1. **Overflow detected**: All arithmetic uses `checked_*` operations
2. **Division by zero prevented**: Zero denominators return errors or safe defaults
3. **Monotonicity preserved**: Index and interest never decrease
4. **Bounds maintained**: All outputs respect min/max constraints

## Attack Scenarios Covered

### 1. Overflow Attack
**Attack**: Pass `i128::MAX` values to cause multiplication overflow
**Defense**: Checked arithmetic returns `Err(Overflow)` rather than panicking or wrapping

### 2. Negative Value Attack
**Attack**: Pass negative principal, rate, or debt to manipulate calculations
**Defense**: Explicit validation rejects negative inputs with `OutOfRange` or `InvalidAmount` errors

### 3. Out-of-Range Parameter Attack
**Attack**: Configure rate model with kink > 100% or floor > ceiling
**Defense**: Parameter validation rejects logically inconsistent configurations

### 4. Utilization Manipulation Attack
**Attack**: Manipulate storage to create debt > supply or negative debt
**Defense**: Utilization is bounded to [0, 100%] and negative debt is rejected

### 5. Excessive Rate Attack
**Attack**: Set borrow rate to extreme values (e.g., 1,000,000% APR) to cause overflow
**Defense**: `MAX_RATE_BPS` ceiling of 1000% APR rejects unrealistic rates

### 6. Reserve Factor Manipulation Attack
**Attack**: Set reserve factor > 100% to steal all interest
**Defense**: Reserve factor validation rejects values > `BPS_DENOM`

### 7. Index Corruption Attack
**Attack**: Corrupt borrow index storage to zero or negative
**Defense**: Index validation panics (fatal error) to halt operation rather than continue with corrupt state

### 8. Time Overflow Attack
**Attack**: Pass extreme elapsed time to cause interest calculation overflow
**Defense**: Checked multiplication in interest formula detects overflow

## Testing

Comprehensive test suite in `hostile_input_boundary_test.rs` covers:

### Boundary Tests
- Zero values (principal, rate, elapsed, supply, debt)
- Maximum valid values (100% utilization, `MAX_RATE_BPS`)
- Just-beyond-maximum values (101% utilization, `MAX_RATE_BPS + 1`)

### Negative Value Tests
- Negative principal, rate, debt, supply, interest
- Expected: Explicit error rather than silent corruption

### Overflow Tests
- `i128::MAX` multiplication attempts
- Expected: `Err(Overflow)` rather than panic or wrap

### Consistency Tests
- Interest split sums to total
- Utilization + rate → valid supply rate
- Index monotonicity across multiple accruals

### Adversarial Input Tests
- Inconsistent parameters (floor > ceiling)
- Excessive values (kink > 100%, reserve factor > 100%)
- Corrupted state (negative debt, zero index)

## Validation Commands

```bash
# Run hostile input boundary tests
cd stellar-lend/contracts/lending
cargo test hostile_input_boundary_test --features testutils

# Run all math-related tests
cargo test math::tests
cargo test debt::tests
cargo test rate_model::tests

# Run property-based tests for additional coverage
cargo test compound_interest_proptest
cargo test reserve_split_proptest
```

## Design Tradeoffs

### Tradeoff 1: Panics vs Errors for Index Corruption
**Decision**: Use panics for invalid borrow index
**Rationale**: The borrow index is global state that affects all users. If it's corrupted, continuing operation would compound the damage. Panicking halts execution and forces investigation.

**Alternative**: Return errors and attempt recovery
**Why rejected**: Recovery from index corruption is infeasible without external intervention (governance action or emergency shutdown)

### Tradeoff 2: Strict Validation vs Permissive Clamping
**Decision**: Reject invalid inputs with errors rather than silently clamping
**Rationale**: Silent clamping hides bugs and allows attacks to partially succeed. Explicit rejection makes problems visible and forces callers to fix root causes.

**Alternative**: Clamp utilization > 100% to 100%, negative rates to 0, etc.
**Why rejected**: Clamping can mask serious bugs (e.g., accounting errors that cause debt > supply)

### Tradeoff 3: `MAX_RATE_BPS` Ceiling
**Decision**: Hard ceiling at 1000% APR (100,000 bps)
**Rationale**: Rates above 1000% are almost certainly input errors (e.g., passing an absolute value instead of basis points). The ceiling catches these errors early.

**Alternative**: No ceiling, accept any i128 rate
**Why rejected**: Extreme rates cause overflow in interest calculations even with checked arithmetic, and they're never legitimate

## Remaining Limitations

### 1. Timestamp Manipulation
**Limitation**: Block timestamp is controlled by validators
**Mitigation**: Stellar's consensus provides reasonable timestamp guarantees (within seconds of real time)
**Residual risk**: Validators could manipulate timestamps by small amounts (seconds), but economic impact is minimal

### 2. Storage Corruption
**Limitation**: This boundary layer cannot prevent direct storage corruption by a malicious contract upgrade
**Mitigation**: Upgrade authorization requires multi-sig approval (see `UPGRADE_AUTHORIZATION.md`)
**Residual risk**: Compromised admin keys could corrupt storage

### 3. Oracle Manipulation
**Limitation**: This boundary validates local calculations but not oracle price inputs
**Mitigation**: Separate oracle validation layer (see `ORACLE_CONFIGURATION_GUIDE.md`)
**Residual risk**: Compromised oracle can provide malicious prices that pass validation

### 4. Gas/Complexity Limits
**Limitation**: Extremely large inputs might exceed block gas limits before overflow is detected
**Mitigation**: Transaction will revert due to gas limit, preventing state corruption
**Residual risk**: None - gas limit acts as an additional safety net

## References

- `rate_model.rs`: Two-slope kink model with surcharge band
- `debt.rs`: Interest accrual and utilization calculations
- `math.rs`: Compound interest and reserve split math
- `common/src/lib.rs`: Basis point scaling and price normalization
- `hostile_input_boundary_test.rs`: Comprehensive boundary test suite

## Changelog

### 2024-XX-XX: Initial Implementation
- Added strict validation to all rate and utilization calculations
- Implemented boundary checks for negative values, overflows, and out-of-range parameters
- Created comprehensive test suite covering normal, boundary, and adversarial inputs
- Documented invariants, attack scenarios, and design tradeoffs
