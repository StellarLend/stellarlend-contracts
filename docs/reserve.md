# Insurance Fund

The lending contract exposes a single, protocol-wide insurance fund. It is an
accounting buffer used before liquidation shortfalls become bad debt and before
`write_off_bad_debt` debits any remainder from `DataKey::TotalDeposits`.

The implementation is in
[`stellar-lend/contracts/lending/src/lib.rs`](../stellar-lend/contracts/lending/src/lib.rs).
There is no separate Reserve/Treasury contract API: the lending contract does
not expose per-asset reserve configuration, a treasury address, or a reserve
withdrawal entrypoint.

## Storage and units

The fund uses two instance-storage entries:

| Key | Type | Meaning | Default when unset |
| --- | --- | --- | --- |
| `DataKey::InsuranceFund` | `i128` | Current global insurance accounting balance | `0` |
| `DataKey::InsuranceShareBps` | `i128` | Share of settled interest credited to the fund | `0` bps |

Both values are global to the lending contract; none of the public insurance
functions accepts an asset address. Amounts use the same integer accounting
units as the lending contract.

`InsuranceFund` is an accounting balance. `fund_insurance` and
`credit_insurance_fund` update storage but do not transfer tokens. An operator
crediting the balance is responsible for ensuring that the corresponding
assets are available to the protocol.

## Public API

These are the exact public signatures in `impl LendingContract`.

### `get_insurance_fund`

```rust
pub fn get_insurance_fund(env: Env) -> i128
```

Returns the current `DataKey::InsuranceFund` balance. It is a read-only view,
requires no authorization, and returns `0` when the key is unset, including
before contract initialization.

### `get_insurance_share`

```rust
pub fn get_insurance_share(env: Env) -> i128
```

Returns the configured interest share in basis points. It is a read-only view,
requires no authorization, and returns `0` when the key is unset, including
before contract initialization.

### `fund_insurance`

```rust
pub fn fund_insurance(env: Env, amount: i128) -> Result<(), LendingError>
```

Adds `amount` to `DataKey::InsuranceFund` with checked arithmetic.

- Requires the contract to be initialized.
- Requires authorization from the admin stored in `DataKey::Admin`.
- Requires `amount > 0`.
- Performs no token transfer.
- Does not write a governance audit-log entry in the current implementation.

Errors:

| Condition | Result |
| --- | --- |
| Contract is not initialized | `LendingError::NotInitialized` |
| `amount <= 0` | `LendingError::InvalidAmount` |
| `current_balance + amount` overflows `i128` | `LendingError::Overflow` |
| Admin authorization is missing | Soroban authorization failure |

### `set_insurance_share`

```rust
pub fn set_insurance_share(env: Env, share_bps: i128) -> Result<(), LendingError>
```

Sets the percentage of settled interest credited to the fund. The accepted
range is inclusive: `0..=10_000` basis points (`0%..=100%`).

- Requires the contract to be initialized.
- Requires authorization from the admin stored in `DataKey::Admin`.
- Replaces the previous `DataKey::InsuranceShareBps` value.
- Records a `set_insurance_share` governance audit-log entry after success.

Errors:

| Condition | Result |
| --- | --- |
| Contract is not initialized | `LendingError::NotInitialized` |
| `share_bps < 0` or `share_bps > 10_000` | `LendingError::InvalidAmount` |
| Admin authorization is missing | Soroban authorization failure |

### `credit_insurance_fund`

```rust
pub fn credit_insurance_fund(env: Env, amount: i128) -> Result<(), LendingError>
```

Credits `amount` to `DataKey::InsuranceFund` with checked arithmetic. This is a
pure accounting operation and does not transfer tokens.

- Requires the contract to be initialized.
- Requires authorization from the admin stored in `DataKey::Admin`.
- Requires `amount > 0`.
- Records a `credit_insurance_fund` governance audit-log entry after success.

Errors:

| Condition | Result |
| --- | --- |
| Contract is not initialized | `LendingError::NotInitialized` |
| `amount <= 0` | `LendingError::InvalidAmount` |
| `current_balance + amount` overflows `i128` | `LendingError::Overflow` |
| Admin authorization is missing | Soroban authorization failure |

`fund_insurance` and `credit_insurance_fund` currently apply the same checked
addition to the same balance. The observable difference is that
`credit_insurance_fund` records a governance audit entry while
`fund_insurance` does not.

### `write_off_bad_debt`

```rust
pub fn write_off_bad_debt(env: Env, amount: i128) -> Result<(), LendingError>
```

Clears `amount` from recorded `DataKey::BadDebt` using the insurance balance
first and `DataKey::TotalDeposits` second.

- Requires the contract to be initialized.
- Requires authorization from the admin stored in `DataKey::Admin`.
- Requires `amount > 0`, existing bad debt, and `amount <= bad_debt`.
- Sets `insurance_used = min(amount, insurance_fund)` and deducts it from
  `DataKey::InsuranceFund`.
- Sets `reserve_used = min(amount - insurance_used, total_deposits)` and deducts
  it directly from `DataKey::TotalDeposits`. `reserve_used` is the event field
  name; there is no separate protocol-reserve storage balance in this path.
- Assigns the remainder to `socialized` and checks
  `socialized > new_total_deposits`. With non-negative balances, a positive
  remainder means `TotalDeposits` was fully used and `new_total_deposits == 0`,
  so the check returns `LendingError::Overflow`. A successful call therefore
  has `socialized = 0`; the current implementation does not successfully apply
  an additional depositor-socialization deduction.
- On success, subtracts `amount` from `DataKey::BadDebt`, publishes
  `BadDebtWrittenOffEvent { amount, insurance_used, reserve_used, socialized }`,
  and records a `write_off_bad_debt` governance audit-log entry.

Errors:

| Condition | Result |
| --- | --- |
| Contract is not initialized | `LendingError::NotInitialized` |
| `amount <= 0` | `LendingError::InvalidAmount` |
| Recorded bad debt is `0` | `LendingError::NoBadDebt` |
| `amount > bad_debt` | `LendingError::WriteOffExceedsBadDebt` |
| Insurance plus `TotalDeposits` cannot cover `amount`, or checked arithmetic fails | `LendingError::Overflow` |
| Admin authorization is missing | Soroban authorization failure |

The three accounting values are persisted before the event is published. The
governance audit entry is recorded afterward, and no external call occurs. An
error aborts the invocation, so the calculated deductions are not persisted.

## Automatic funding from interest

Interest settlement calls the internal `settle_and_accrue_insurance` helper.
When both settled interest and the configured share are positive, it computes:

```text
insurance_credit = settled_interest * insurance_share_bps / 10_000
```

The multiplication and addition are checked. Integer division rounds down for
these non-negative values. A zero result leaves the balance unchanged. The
helper credits the accounting balance directly; it does not call either public
admin entrypoint and does not create a governance audit entry.

## How the fund is consumed

The internal `draw_insurance` helper uses:

```text
drawn = min(requested_amount, insurance_fund_balance)
new_balance = insurance_fund_balance - drawn
```

This prevents a draw from making the balance negative.

- During liquidation, the fund covers as much of a collateral shortfall as it
  can. Any residual shortfall is added to recorded bad debt.
- During `write_off_bad_debt`, insurance is deducted first and any remainder is
  deducted directly from `DataKey::TotalDeposits`. If the combined balances are
  insufficient, the call returns `LendingError::Overflow`.

There is no public function that withdraws the insurance balance to a treasury
address.

## Authorization model

The four mutating entrypoints first require successful contract
initialization and then require the stored admin address to authorize the
invocation. The admin address is read from storage; it is not supplied as a
function parameter.

An absent admin returns the typed `LendingError::NotInitialized`. A present
admin that did not authorize the invocation is rejected by Soroban's
`Address::require_auth()` mechanism rather than by a typed
`LendingError::Unauthorized` return.

The two getters are intentionally available before initialization and return
their zero defaults.

## Client example

The generated Soroban client omits the `Env` argument from its call methods:

```rust
// Admin-authorized configuration: route 10% of settled interest to insurance.
client.set_insurance_share(&1_000);

// Accounting-only explicit credit.
client.credit_insurance_fund(&500);

assert_eq!(client.get_insurance_share(), 1_000);
assert_eq!(client.get_insurance_fund(), 500);
```

Use the generated `try_*` methods when callers need to inspect typed contract
errors instead of panicking on a failed invocation.

## Invariants

1. `0 <= InsuranceShareBps <= 10_000` for values written through the public
   setter.
2. Public credits require a strictly positive amount.
3. Explicit and automatic credits use checked `i128` addition.
4. Draws are capped at the current fund balance, so a draw cannot make the
   balance negative.
5. The accounting balance is global, not keyed by asset.
6. Crediting the accounting balance does not itself prove that matching tokens
   were transferred to the contract.
7. A successful `write_off_bad_debt` is fully covered by the insurance fund and
   `TotalDeposits`; a positive residual returns `LendingError::Overflow`.

## Tests

Relevant source tests are:

- [`insurance_fund_test.rs`](../stellar-lend/contracts/lending/src/insurance_fund_test.rs):
  share bounds, explicit funding, automatic interest credits, and liquidation
  coverage.
- [`bad_debt_write_off_test.rs`](../stellar-lend/contracts/lending/src/bad_debt_write_off_test.rs):
  insurance and `TotalDeposits` accounting, errors, and event fields.
- [`initialization_guard_test.rs`](../stellar-lend/contracts/lending/src/initialization_guard_test.rs):
  pre-initialization behavior for getters and mutators.
- [`governance_audit_test.rs`](../stellar-lend/contracts/lending/src/governance_audit_test.rs):
  governance audit records for `set_insurance_share`.

Run the focused lending tests from `stellar-lend/`:

```bash
cargo test -p stellarlend-lending insurance_fund --lib
cargo test -p stellarlend-lending credit_insurance_fund --lib
cargo test -p stellarlend-lending write_off_bad_debt --lib
cargo test -p stellarlend-lending initialization_guard --lib
cargo test -p stellarlend-lending governance_audit --lib
```

## Source references

- [`src/lib.rs`](../stellar-lend/contracts/lending/src/lib.rs): public API,
  storage keys, interest accrual, draw-down logic, and error definitions.
- [`INSURANCE_FUND.md`](../stellar-lend/contracts/lending/INSURANCE_FUND.md):
  liquidation funding model and worked examples.
- [`docs/interface_quick_reference.md`](interface_quick_reference.md): broader
  lending-interface reference. It does not currently enumerate these
  insurance-fund and bad-debt entrypoints, so use `src/lib.rs` as the source of
  truth for this API.
