# Flash Loan Feature

The StellarLend flash loan feature allows users to borrow assets and repay them with a fee in the same transaction. This is a powerful tool for arbitrage, liquidations, and other DeFi strategies that require zero-collateral capital.

## How it Works

1.  **Initiation**: A user calls the `flash_loan` function on the lending contract.
2.  **Fund Transfer**: The lending contract transfers the requested amount of assets to the specified `receiver` address.
3.  **Callback**: The lending contract invokes the `on_flash_loan` function on the `receiver` contract.
4.  **Repayment**: After the callback returns, the lending contract transfers the borrowed amount plus a fee back from the `receiver`.

## Interface

### Lending Contract

```rust
pub fn flash_loan(
    env: Env,
    receiver: Address,
    asset: Address,
    amount: i128,
    params: Bytes,
) -> Result<(), FlashLoanError>
```

### Receiver Contract Requirements

The `receiver` address must be a contract that implements the following function:

```rust
pub fn on_flash_loan(
    env: Env,
    initiator: Address,
    asset: Address,
    amount: i128,
    fee: i128,
    params: Bytes,
) -> bool
```

The receiver must return `true` to acknowledge the loan and must have approved the lending contract to transfer back `amount + fee` by the time the function returns.

## Fees

The flash loan fee is configurable by the protocol admin in basis points (1 bp = 0.01%).

- **Setter**: `set_flash_fee(fee_bps: i128)`
- **Default**: 5 bps (0.05%)
- **Maximum**: 1000 bps (10%)

## Max Utilization Circuit Breaker

Flash-loan size is also capped by an admin-configurable utilization ratio. Before the callback fires, the lending contract rejects any request whose principal would exceed:

`max_flash_bps × available_liquidity / 10000`

where `available_liquidity` is the current treasury balance available to the flash-loan path. The cap is enforced before any transfer or callback occurs.

- **Setter**: `set_max_flash_bps(max_flash_bps: i128)`
- **Getter**: `get_max_flash_bps() -> i128`
- **Bounds**: `0..=10000`
- **Example**: with `max_flash_bps = 5000` and `available_liquidity = 1_000`, the maximum flash loan is `500`.

## Security Assumptions

- **Atomicity**: The entire process occurs in a single transaction. If repayment fails, the transaction reverts.
- **Reentrancy**: Standard Soroban protections apply.
- **Fee Caps**: fees are capped at 10% to prevent accidental or malicious misconfiguration.

## Callback Failure Rollback Guarantee

When the `on_flash_loan` callback fails — either by panicking (reverting) or by
returning without calling `repay_flash_loan` — the lending contract guarantees
complete state rollback.

### What is guaranteed

| Condition | Outcome |
|-----------|---------|
| Callback panics / traps | Whole transaction reverts; treasury and receiver balances restored |
| Callback under-repays | `InsufficientRepayment` panic; full rollback via Soroban atomicity |
| `FlashActive` flag | Always `false` after any failed loan — never left stuck |
| Receiver balance | Never retains loaned funds after failure |
| Treasury balance | Identical to pre-loan value after failure |

### Mechanism

Soroban executes every contract invocation atomically. When the lending
contract panics (e.g. on `InsufficientRepayment`), or when the
`invoke_contract` call to the callback returns a trap, the host rolls back
**all** storage mutations made during that invocation frame. This means:

1. The treasury debit (`Treasury -= amount`) is reversed.
2. The receiver credit (`Balance[receiver] += amount`) is reversed.
3. The `FlashActive = true` write is reversed.

Because rollback is a host-level guarantee — not a manual try/catch inside
the contract — there is no code path that can leave the `FlashActive` flag
stuck at `true` after a failed flash loan.

### Verified by integration tests

**`tests/flash_callback_revert_test.rs`** — rollback basics:

- `test_reverting_callback_returns_err_and_rolls_back` — callback panics;
  confirms treasury, receiver balance, and `FlashActive` all restored.
- `test_reverting_callback_panics_on_direct_call` — non-`try_*` variant
  propagates the panic to the caller.
- `test_under_repaying_callback_returns_err_and_rolls_back` — callback
  returns without repaying; confirms full rollback.
- `test_flash_active_not_stuck_after_consecutive_failures` — two consecutive
  failed loans; `FlashActive` is `false` after each, confirming no stuck flag.

**`tests/flash_loan_repayment.rs`** — end-to-end cross-contract repayment:

| Test | Receiver | Expected outcome |
|------|----------|-----------------|
| `test_compliant_receiver_repays_exact` | `CompliantReceiver` | Success; treasury restored |
| `test_compliant_receiver_over_repays` | `OverRepayingReceiver` (+1) | Success; treasury ≥ original |
| `test_compliant_receiver_zero_fee` | `CompliantReceiver` (fee=0) | Success; principal only |
| `test_compliant_receiver_fee_accounting_matches_bps` | `CompliantReceiver` (30 bps) | `fee = amount × 30 / 10_000` |
| `test_consecutive_flash_loans_succeed` | `CompliantReceiver` ×2 | Both succeed; no stuck flag |
| `test_malicious_receiver_under_repays_by_one` | `MaliciousReceiver` (−1) | `InsufficientRepayment` panic |
| `test_malicious_receiver_repays_zero` | `MaliciousReceiver` (×0) | `InsufficientRepayment` panic |
| `test_rollback_on_under_repayment` | `MaliciousReceiver` (−1) | `Err`; treasury/balance/flag rolled back |
| `test_flash_active_blocks_deposit_mid_callback` | `DepositAttempter` | `FlashLoanReentrancy` panic |
| `test_flash_active_blocks_withdraw_mid_callback` | `WithdrawAttempter` | `FlashLoanReentrancy` panic |
| `test_flash_active_cleared_after_success` | `CompliantReceiver` | `FlashActive = false` |
| `test_flash_active_cleared_after_failure` | `MaliciousReceiver` | `FlashActive = false` (rollback) |

### Receiver contract interface

Any contract acting as a flash loan receiver must implement:

```rust
pub fn on_flash_loan(
    env: Env,
    initiator: Address,
    asset: Address,
    amount: i128,
    fee: i128,
    params: Bytes,
)
```

Inside the callback the receiver must call `repay_flash_loan(payer, asset, amount + fee)`
on the lending contract before returning.  The receiver's `Balance(asset, receiver)`
entry is credited with `amount` before the callback fires, so the full repayment
can be made without any external funding as long as the receiver holds no other
obligations.

Receivers that need to over-repay (e.g. to earn yield) may call
`repay_flash_loan` with any amount ≥ `amount + fee`.  The `flash_loan`
check is `final_treasury >= original_treasury + fee` (i.e. `>=`, not `==`),
so over-payment is accepted.

## Reservation Accounting

The current lending contract does not implement a separate flash-loan reservation counter. Deposit-cap checks are enforced against the contract's live balances and tracked state without a dedicated reservation mechanism.

