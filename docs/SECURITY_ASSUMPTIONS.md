# Security Assumptions and Trust Boundaries

## Overview

This document outlines the security architecture of the StellarLend protocol, defining how trust is distributed across various actors and how token flows are secured.

## Trust Boundaries

The protocol defines several critical trust boundaries where authorization and validation are enforced:

1.  **User vs. Protocol**: All user-facing operations (`deposit`, `borrow`, `withdraw`, `repay`) require explicit authorization using Soroban's `require_auth()` mechanism. The protocol assumes the underlying Stellar network and Soroban runtime correctly enforce these signatures.
2.  **Protocol vs. Oracle**: The protocol trusts designated Oracle contracts to provide price feeds. However, it implements safeguards:
    - **Price Validation**: Checks for stale or outlier prices.
    - **Fallback Mechanisms**: Uses cached prices if the primary feed is unavailable.
3.  **Protocol vs. Bridge**: Cross-chain operations depend on authorized bridge contracts. The protocol verifies that the caller is a registered bridge before processing cross-chain deposits or withdrawals.
4.  **Admin vs. System**: The admin has significant power to adjust risk parameters and pause the system. This power is intended to be protected by multisig or governance processes.

## Admin & Guardian Powers

### Admin Capabilities
- **Risk Configuration**: Setting `min_collateral_ratio`, `base_rate`, `kink_utilization`, `multiplier`, and `reserve_factor`.
- **System Control**: Pausing or unpausing specific protocol actions (e.g., pausing borrowing during market volatility).
- **Oracle Management**: Updating the trusted oracle address and price cache TTL.
- **Contract Upgrades**: Proposing and executing contract upgrades (restricted to `UpgradeManager` constraints).

### Guardian Capabilities (Social Recovery)
- **Identity Recovery**: Guardians can approve and execute social recovery for a user who has lost access to their primary account.
- **Timelock Constraints**: Recovery actions are subject to timelocks to allow for cancellation in case of malicious guardian behavior.

## Token Transfer Flows

The protocol manages tokens through standardized flows:

### Deposit Collateral
1.  **Authorization**: User calls `deposit_collateral(user, asset, amount)` and authorizes the transfer.
2.  **Transfer**: The protocol invokes `transfer(user, protocol, amount)` for the specific asset.
3.  **Record Keeping**: The protocol updates internal storage to track the user's collateral balance and updates global analytics.

### Borrow Assets
1.  **Health Check**: The protocol calculates the user's current collateral ratio using Oracle prices.
2.  **Invariant**: Ensures `total_borrow_value * min_collateral_ratio <= total_collateral_value`.
3.  **Transfer**: The protocol invokes `transfer(protocol, user, amount)` for the borrowed asset.
4.  **Record Keeping**: Increases the user's liability and updates utilization rates.

### Repay Debt
1.  **Interest Accrual**: Interest is calculated based on elapsed time and current rates.
2.  **Transfer**: User transfers `principal + interest` back to the protocol.
3.  **Record Keeping**: Reduces the user's liability and updates protocol reserves.

### Withdraw Collateral
1.  **Health Check**: Ensures that the withdrawal does not push the user's collateral ratio below the minimum required for their outstanding debt.
2.  **Transfer**: The protocol invokes `transfer(protocol, user, amount)` for the collateral asset.
3.  **Record Keeping**: Decreases the user's collateral balance and updates analytics.

## Security Controls

- **Reentrancy**: Atomic operations and state-update-before-transfer patterns (Checks-Effects-Interactions) are used to prevent reentrancy.
- **Checked Arithmetic**: All calculations (interest, ratios, balances) utilize Rust's checked arithmetic or safe math abstractions to prevent overflows and underflows.
- **Authorization**: `require_auth()` is called on every entry point that modifies user state or admin configuration.
- **Validation**: Strict input validation is performed on all protocol parameters (e.g., ensuring interest rates are within reasonable bounds).
