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
- `payload`: A vector of values with the format `[version: u32, action: Symbol, ...optional_data]`
  - `version`: Must be `1` (current payload version)
  - `action`: A `Symbol` indicating the action (`deposit` or `repay`)
  - `optional_data`: Additional data fields (ignored in current version)

## Actions

### Deposit

To deposit collateral via `receive`, the user provides the payload `[1, "deposit"]`, approves the lending contract as a spender, and invokes the entrypoint.

**Mechanism**:

1. User approves the lending contract on the token contract.
2. User calls `receive(token_asset, from, amount, [1, "deposit"])`.
3. Lending contract validates the action and current pause state.
4. Lending contract pulls `amount` from `from` into the lending contract with `transfer_from`.
5. Lending contract updates the user's borrow-collateral position and emits a deposit event.

### Repay

To repay debt via `receive`, the user provides the payload `[1, "repay"]`, approves the lending contract as a spender, and invokes the entrypoint.

**Mechanism**:

1. User approves the lending contract on the token contract.
2. User calls `receive(token_asset, from, amount, [1, "repay"])`.
3. Lending contract validates the action and current pause state.
4. Lending contract pulls `amount` from `from` into the lending contract with `transfer_from`.
5. Lending contract accrues interest, repays interest first, then repays principal.
6. Updates protocol-wide `TotalDebt`.
7. Emits a `repay` event.

## Security Considerations

### Core Security Model

1. **Authorization**: `from.require_auth()` is required, so a third party cannot trigger a pull from another user's balance just because an allowance exists.
2. **Token Transfer Flow**: The contract checks allowance and balance before calling `transfer_from`. The state mutation only occurs after the token pull succeeds.
3. **Pause Enforcement**: Unlike the earlier optimistic-receiver approach, `receive` now validates protocol pause state before any funds move, so paused operations stay paused.
4. **Admin and Guardian Powers**: Admins can pause deposit/repay flows or trigger emergency lifecycle transitions through the normal protocol controls. Guardians do not have any special power over `receive` beyond the protocol-wide emergency states they can help initiate.
5. **Reentrancy**: `receive` performs only a single token-contract call and then mutates local state; there is no callback path or user-supplied external call during processing.
6. **Deposit Cap Enforcement**: The `deposit` action in the `receive` hook is subject to the same global deposit cap as direct deposits. Transactions that would exceed the cap are rejected and any pending token transfers are rolled back.
7. **Checked Arithmetic**: Deposits, debt accrual, and repayments continue to use checked arithmetic in the underlying borrow logic, so overflow paths are explicit and tested.

### Enhanced Security Validations (v1)

8. **Payload Versioning**: All payloads must include version `1`. Legacy payloads without versioning are rejected to prevent protocol downgrade attacks.
9. **Strict Payload Structure**: Payloads must have exactly `[version, action]` structure with optional additional data. Malformed payloads are rejected.
10. **Payload Length Limits**: Maximum payload length is enforced to prevent DoS attacks through oversized payloads.
11. **Sender Validation**: The sender cannot be the token contract itself or the lending contract (prevents self-call attacks).
12. **Action Whitelisting**: Only `deposit` and `repay` actions are allowed. All other actions are rejected.
13. **Asset Registry Validation**: Token contracts must be registered in the asset registry before they can be used.

### Attack Scenarios Prevented

- **Malformed Payload Attacks**: Rejected with `MalformedPayload` error
- **Version Downgrade Attacks**: Rejected with `InvalidPayloadVersion` error
- **Unauthorized Sender Attacks**: Token contracts and self-calls rejected with `UnauthorizedSender` error
- **DoS via Large Payloads**: Rejected with `MalformedPayload` error
- **Action Injection**: Invalid actions rejected with `AssetNotSupported` error

## Usage Example

### Via Token Approval + Receive

```rust
token_client.approve(&user, &lending_contract_id, &100_000_000, &200);
let payload = vec![&env, 1u32.into_val(&env), symbol_short!("deposit").into_val(&env)];
lending_contract_client.receive(
    &usdc_asset,
    &user,
    &100_000_000,
    &payload,
);
```

### Direct Call (Alternative)

The contract also exposes direct `deposit_collateral` and `repay` functions for protocol-managed flows.

```rust
lending_contract_client.deposit_collateral(&user, &usdc_asset, &100_000_000);
```
