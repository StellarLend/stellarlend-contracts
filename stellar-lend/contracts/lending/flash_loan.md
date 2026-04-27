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
