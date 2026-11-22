# Repay Semantics

StellarLend exposes two separate repay paths — one for the single-asset borrow system and one
for the cross-asset system. Their handling of overpayment, interest ordering, and dust differs.
Integrators must understand which path they are calling.

---

## 1. Single-Asset Borrow System (`borrow::repay`)

### Call signature
```rust
pub fn repay(env: &Env, user: Address, asset: Address, amount: i128) -> Result<(), BorrowError>
```

### Overpay behaviour: **Error on overpay**
If `amount` exceeds the total outstanding debt (principal + accrued interest after settlement),
`repay` returns `LendingError::RepayAmountTooHigh` (error code 1012). The position is not
modified and no funds are consumed.

Integrators **must** query the exact current debt before submitting a repay:

```rust
let debt = client.get_debt_position(&user);
// derive effective balance including accrued interest
let effective = debt::effective_debt(&debt, now, rate_bps)?;
client.repay(&user, effective);
```

Attempting to pass any amount larger than the effective balance will be rejected.

### Interest ordering
Interest is settled **before** principal on every repay:

1. `settle_accrual` is called at repay time and accrued interest is added to the principal.
2. The repay amount is consumed against the settled total (principal + interest).
3. If repay amount exceeds settled total, the principal is clamped to 0 and the excess is refunded.

This means an overpayment always results in zero debt, and interest is never carried forward.

### Dust prevention
`calculate_interest` uses **ceiling division**:

```
interest = ceil(principal × INTEREST_RATE_PER_YEAR × elapsed / (BPS_SCALE × SECONDS_PER_YEAR))
```

For any non-zero principal and any elapsed time ≥ 1 second, interest ≥ 1. Therefore
`get_debt_position()` (with the interest-inclusive balance derived via `debt::effective_debt`) at the moment of repay equals the exact amount `repay` will consume.
No sub-unit dust can remain after a correctly-sized repay call.

### Recovery mode
Repay is **permitted** during `EmergencyState::Recovery`. Users must be able to unwind positions
even when high-risk operations (new borrows, withdrawals) are paused. The gate is:

```
if is_paused(PauseType::Repay) || (!is_recovery && blocks_high_risk_ops)
```

### Summary of error codes

| Condition                              | Error                             |
|----------------------------------------|-----------------------------------|
| `amount ≤ 0`                           | `LendingError::InvalidAmount`     |
| No outstanding debt                    | `LendingError::InvalidAmount`     |
| `amount > interest + principal`        | `LendingError::RepayAmountTooHigh` (1012) |
| Protocol paused for repay              | (contract panics / pause guard)   |

---

## 2. Cross-Asset System (`cross_asset::repay_asset`)

### Call signature
```rust
pub fn repay_asset(env: &Env, user: Address, asset: Address, amount: i128) -> Result<(), CrossAssetError>
```

### Overpay behaviour: **Silent clamp**
If `amount` exceeds the current debt balance for that asset, the repay amount is silently
clamped to the outstanding balance:

```rust
let repay_amount = amount.min(current_debt);
```

This means callers may safely pass an amount larger than the debt (including `i128::MAX - 1`)
without triggering an error. The position will be fully cleared.

### No interest accrual
The cross-asset system does not accrue interest. Debt balances change only through explicit
borrow and repay operations.

### Per-asset isolation
Repaying one asset's debt does not affect any other asset's balance in the user's position.
Each asset key is independent in the `debt_balances` map.

### Summary of error codes

| Condition                  | Error                              |
|----------------------------|------------------------------------|
| `amount ≤ 0`               | `CrossAssetError::InvalidAmount`   |
| Protocol paused for repay  | `CrossAssetError::ProtocolPaused`  |

---

## 3. Comparison table

| Dimension               | `borrow::repay`                      | `cross_asset::repay_asset`    |
|-------------------------|--------------------------------------|-------------------------------|
| Overpay handling        | Error (`RepayAmountTooHigh`, 1012)   | Silent clamp to balance       |
| Interest accrual        | Yes (ceiling-rounded)                | No                            |
| Repay ordering          | Interest first, then principal       | N/A                           |
| Dust risk               | None (ceiling rounding)              | None (clamp floors at 0)      |
| Recovery-mode repay     | Allowed                              | Allowed                       |
| Asset isolation         | Single asset per position            | Per-asset in a shared map     |

---

## 4. Security notes for integrators

**Do not overpay the borrow system.** Unlike many DeFi protocols that silently refund excess,
`borrow::repay` returns `LendingError::RepayAmountTooHigh` (1012). Always query `get_debt_position()`
for the raw debt state and derive the effective balance with `debt::effective_debt` before
submitting a repay.

**Dust debt cannot block withdrawals.** Because interest uses ceiling division, the effective
balance derived from `get_debt_position()` (via `debt::effective_debt`) at any timestamp is
exactly what a full repay consumes. There is no scenario where a tiny
residual interest amount (< 1 unit) blocks a user's withdrawal.

**Cross-asset overpay is safe by design.** A malicious caller cannot use an overpay to extract
excess credit — the clamp is a floor operation, not a refund. `repay_asset` will never reduce
a balance below zero.

**Recovery-mode positions can always be unwound.** If a user is in a recovery-mode scenario,
they retain the ability to repay and reduce their health-factor risk regardless of what other
operations are paused.
