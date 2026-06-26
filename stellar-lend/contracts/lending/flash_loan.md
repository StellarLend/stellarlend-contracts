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

- **Setter**: `set_flash_loan_fee_bps(fee_bps: i128)`
- **Default**: 5 bps (0.05%)
- **Maximum**: 1000 bps (10%)

## Security Assumptions

- **Atomicity**: The entire process occurs in a single transaction. If repayment fails, the transaction reverts.
- **Reentrancy**: Standard Soroban protections apply.
- **Fee Caps**: fees are capped at 10% to prevent accidental or malicious misconfiguration.

# Flash Loan Reservation Accounting

## Overview

Flash loans in StellarLend allow users to borrow assets without collateral, provided the borrowed amount (plus a fee) is returned within the same ledger. This document describes the reservation accounting system that prevents deposit cap over-allocation during active flash loans.

## The Problem

When a flash loan moves an asset out of the protocol and back:

1. The contract's asset balance temporarily decreases
2. The `total_deposits` counter does **not** decrease (deposits are still recorded)
3. A deposit during the same ledger could over-allocate because the cap check only sees the reduced balance

**Example:**
- Deposit cap: 10,000 XLM
- Total deposits: 8,000 XLM
- Flash loan: 1,500 XLM (balance drops to 6,500, but deposits still 8,000)
- New deposit: 2,000 XLM
- **Without reservation accounting:** 8,000 + 2,000 = 10,000 (passes, but actual backing is 6,500 + 2,000 = 8,500)
- **With reservation accounting:** 8,000 + 1,500 + 2,000 = 11,500 (correctly fails)

## The Solution

A `reserved_for_flash_loan(asset)` counter is maintained in **Temporary storage** (ledger-scoped):
effective_deposits = total_deposits + reserved_for_flash_loan


### Lifecycle
Flash Loan Initiated
│
▼
┌─────────────────┐
│  Debit Reserve  │  reserved += amount
│   (Temporary)   │
└─────────────────┘
│
▼
Transfer Out
│
▼
Callback Invoke
│
▼
Transfer Back
│
▼
┌─────────────────┐
│ Credit Reserve  │  reserved -= amount
│   (Temporary)   │
└─────────────────┘
│
▼
Verify Repayment
plain


## Invariants

| # | Invariant | Enforcement |
|---|-----------|-------------|
| I-1 | `reserved(asset) <= total_deposits(asset)` | Asserted on every debit |
| I-2 | `release_amount <= reserved_amount` | Asserted on every credit |
| I-3 | `effective_deposits = total_deposits + reserved` | Used in deposit cap check |
| I-4 | Reservation is Temporary storage | Auto-expires at ledger close (failsafe) |

## Deposit Cap Check

```rust
fn check_deposit_cap(env: &Env, asset: &Address, additional_amount: i128) {
    let deposit_cap = get_asset_params(env, asset).deposit_cap;
    let effective = get_total_deposits(env, asset) 
                  + get_reserved_for_flash_loan(env, asset);
    
    assert!(effective + additional_amount <= deposit_cap, "cap exceeded");
}


Same-Ledger Interleaving
The following sequence is safe and tested:
Ledger N:
Flash loan 1,500 XLM (reserved = 1,500)
Deposit 500 XLM (effective = 8,000 + 1,500 + 500 = 10,000, passes)
Flash loan repaid (reserved = 0)
Ledger N (invalid):
Flash loan 1,500 XLM (reserved = 1,500)
Deposit 2,000 XLM (effective = 8,000 + 1,500 + 2,000 = 11,500, fails)
Flash loan repaid (reserved = 0)

Storage Tier
ReservedForFlashLoan(Address) uses Temporary storage because:
The reservation only matters within a single ledger
If the flash loan is not repaid (or the contract panics), the reservation auto-expires
No TTL bump is needed (reduces rent cost)
Provides a natural failsafe against state corruption

Events
| Event                 | Topics                           | Data                           |
| --------------------- | -------------------------------- | ------------------------------ |
| `flash_loan_reserved` | `("flash_loan_reserved", asset)` | `(amount, new_reserved_total)` |
| `flash_loan_released` | `("flash_loan_released", asset)` | `(amount, new_reserved_total)` |
| `flash_loan`          | `("flash_loan", asset)`          | `(amount, fee, initiator)`     |


Security Notes
Reservation overflow: Checked arithmetic prevents overflow on debit
Double-release: Asserted against; cannot release more than reserved
Temporary storage expiry: If a bug prevents release, the reservation expires at ledger close (no permanent state corruption)
Reentrancy: Callback is invoked after debit but before release; the reservation protects against reentrant deposits

