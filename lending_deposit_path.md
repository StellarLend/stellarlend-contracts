# Lending Deposit Path and Token Receiver Hardening

This document summarizes the implementation for Issue #473: "Lending deposit path and token receiver safety".

## Implementation Overview

The hardening focus was on two primary entry points for collateral movement: the direct `deposit` function in `deposit.rs` and the `receive` hook in `token_receiver.rs`.

### 1. Reentrancy Protection
A new `reentrancy.rs` module was introduced to the `lending` contract, providing a standard RAII `ReentrancyGuard`.
- **Mechanism**: Uses `env.storage().temporary()` to set a `LockV1` key.
- **Scope**: Applied to all fund-moving and state-mutating paths in `deposit.rs` and `token_receiver.rs`.
- **Benefit**: Blocks nested synchronous call attacks (e.g., a token contract's hook calling back into the lending contract to multiply balances).

### 2. Token Receiver Safety (`token_receiver.rs`)
- **Verified Caller**: Added `token_asset.require_auth()`. This confirms that the `token_asset` provided in the arguments is the actual contract calling the `receive` hook.
- **Pause Enforcement**: Explicitly check `is_paused` for `Deposit` and `Repay` actions before dispatching to underlying logic.
- **Validation**: Added positive amount checks and payload integrity verification.

### 3. Direct Deposit Hardening (`deposit.rs`)
- **Physical Asset Transfer**: Updated `deposit()` to perform a real `token::Client.transfer(user, contract, amount)`. This ensures that accounting matches actual liquidity.
- **Authorization**: Enforced `user.require_auth()` to allow the contract to pull tokens on behalf of the user.
- **Position Consistency**: Added a check to prevent overwriting a user's collateral position with a different asset (enforcing single-asset positions for the simple lending module).
- **Audit trail**: Enhanced Rustdoc with security assumptions and error conditions.

## API Changes

### Error Variants
Added `Reentrancy` variant to both `BorrowError` and `DepositError` enums (Code `10` and `7` respectively).

### Storage
Reentrancy locks are stored in **Temporary** storage, which is gas-efficient and automatically cleared after the transaction, ensuring it never persists into future sessions.

## Security Considerations

- **SAC Compatibility**: The implementation follows standard Soroban Asset Contract (SAC) patterns. Users must authorize the `deposit` call to allow the contract to execute the transfer.
- **Trust Boundaries**: The contract now explicitly verifies the identity of calling token contracts in the receiver hook, preventing spoofing of the `token_asset` parameter.
- **Checked Arithmetic**: All balance updates continue to use `checked_add` and `checked_sub` to prevent overflow.

## Testing Results

The test suites in `deposit_test.rs` and `token_receiver_test.rs` have been updated to use real `StellarAssetContract` instances instead of mock addresses. This validates:
- [x] Authorization requirements for `deposit` and `receive`.
- [x] Correct token balance movement after a successful deposit.
- [x] Rejection of zero or negative amounts.
- [x] Prevention of asset mismatch in collateral positions.
- [x] Enforcement of pause states.

## Implementation Highlights (Rust)

### Token Receiver (`token_receiver.rs`)

```rust
pub fn receive(
    env: Env,
    token_asset: Address,
    from: Address,
    amount: i128,
    payload: Vec<Val>,
) -> Result<(), BorrowError> {
    // Verified Caller: token_asset.require_auth()
    token_asset.require_auth();

    if amount <= 0 {
        return Err(BorrowError::InvalidAmount);
    }

    // Reentrancy protection
    let _guard = ReentrancyGuard::new(&env).map_err(|_| BorrowError::Reentrancy)?;

    // ... Dispatch with Pause checks ...
    if action == Symbol::new(&env, "deposit") {
        if pause::is_paused(&env, PauseType::Deposit) {
            return Err(BorrowError::ProtocolPaused);
        }
        deposit(&env, from, token_asset, amount)
    }
    // ...
}
```

### Lending Deposit (`deposit.rs`)

```rust
pub fn deposit(
    env: &Env,
    user: Address,
    asset: Address,
    amount: i128,
) -> Result<i128, DepositError> {
    user.require_auth();

    if pause::is_paused(env, PauseType::Deposit) {
        return Err(DepositError::DepositPaused);
    }

    let _guard = ReentrancyGuard::new(env).map_err(|_| DepositError::Reentrancy)?;

    // ... Validation ...

    let mut position = get_deposit_position(env, &user, &asset);
    
    // Enforcement: Simple position model - one asset only
    if position.amount > 0 && position.asset != asset {
        return Err(DepositError::AssetNotSupported);
    }

    // Execution: Move tokens (SAC Transfer)
    let token_client = token::Client::new(env, &asset);
    token_client.transfer(&user, &env.current_contract_address(), &amount);

    // ... Storage updates ...
}
```
