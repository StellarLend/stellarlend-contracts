# Token Receiver Documentation

## Overview

The `receive` entrypoint allows StellarLend to process collateral deposits and debt repayments using the Soroban token allowance flow. The caller supplies an action payload, the user authorizes the call, and the lending contract pulls tokens from the user's balance with `transfer_from` before updating protocol state.

This is intentionally implemented as a pull-based flow because the Soroban token interface used by this repository exposes `approve` and `transfer_from`, but does not expose a standard authenticated receiver-hook callback.

## Function Signature

```rust
pub fn receive(
    env: Env,
    token_asset: Address,
    from: Address,
    amount: i128,
    payload: Vec<Val>,
) -> Result<(), BorrowError>
```

## Parameters

- `env`: The contract environment
- `token_asset`: The address of the asset contract to debit
- `from`: The address authorizing the token pull and state update
- `amount`: The amount of tokens to transfer into the lending contract
- `payload`: A vector of values, where the first element is a `Symbol` indicating the action (`deposit` or `repay`)

## Actions

### Deposit

To deposit collateral via `receive`, the user provides the payload `"deposit"`, approves the lending contract as a spender, and invokes the entrypoint.

**Mechanism**:

1. User approves the lending contract on the token contract.
2. User calls `receive(token_asset, from, amount, ["deposit"])`.
3. Lending contract validates the action and current pause state.
4. Lending contract pulls `amount` from `from` into the lending contract with `transfer_from`.
5. Lending contract updates the user's borrow-collateral position and emits a deposit event.

### Repay

To repay debt via `receive`, the user provides the payload `"repay"`, approves the lending contract as a spender, and invokes the entrypoint.

**Mechanism**:

1. User approves the lending contract on the token contract.
2. User calls `receive(token_asset, from, amount, ["repay"])`.
3. Lending contract validates the action and current pause state.
4. Lending contract pulls `amount` from `from` into the lending contract with `transfer_from`.
5. Lending contract accrues interest, repays interest first, then repays principal.
6. Updates protocol-wide `TotalDebt`.
7. Emits a `repay` event.

## Security Considerations

1. **Authorization**: `from.require_auth()` is required, so a third party cannot trigger a pull from another user's balance just because an allowance exists.
2. **Token Transfer Flow**: The contract checks allowance and balance before calling `transfer_from`. The state mutation only occurs after the token pull succeeds.
3. **Pause Enforcement**: Unlike the earlier optimistic-receiver approach, `receive` now validates protocol pause state before any funds move, so paused operations stay paused.
4. **Admin and Guardian Powers**: Admins can pause deposit/repay flows or trigger emergency lifecycle transitions through the normal protocol controls. Guardians do not have any special power over `receive` beyond the protocol-wide emergency states they can help initiate.
5. **Reentrancy**: `receive` performs only a single token-contract call and then mutates local state; there is no callback path or user-supplied external call during processing.
6. **Checked Arithmetic**: Deposits, debt accrual, and repayments continue to use checked arithmetic in the underlying borrow logic, so overflow paths are explicit and tested.

## Usage Example

### Via Token Approval + Receive

```rust
token_client.approve(&user, &lending_contract_id, &100_000_000, &200);
lending_contract_client.receive(
    &usdc_asset,
    &user,
    &100_000_000,
    &vec![&env, symbol_short!("deposit").into_val(&env)],
);
```

### Direct Call (Alternative)

The contract also exposes direct `deposit_collateral` and `repay` functions for protocol-managed flows.

```rust
lending_contract_client.deposit_collateral(&user, &usdc_asset, &100_000_000);
```
