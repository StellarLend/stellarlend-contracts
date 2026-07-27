# Liquidation token transfers

## Overview

The `liquidate` entry point now performs real SAC-style token movement for both sides of the liquidation:

- the liquidator transfers `actual_repay` of the debt asset into the lending contract
- the lending contract transfers `final_seized` of the collateral asset out to the liquidator

## Ordering

The implementation follows checks-effects-interactions ordering:

1. Validate that the position is liquidatable and compute the repayment and collateral amounts.
2. Update the borrower's debt and collateral storage entries.
3. Execute the token transfers.

Because Soroban executions are atomic, any transfer failure aborts the whole invocation and the storage updates revert along with it.

## Failure semantics

- If the liquidator does not hold enough debt tokens to repay, the transfer fails and the liquidation reverts.
- If the payout transfer fails, the debt/collateral storage updates are rolled back.
- The liquidation amount is capped by the close-factor and collateral availability before any transfers are attempted.

## Security notes

This change keeps the contract's accounting aligned with on-chain balances and ensures the protocol does not claim funds or issue collateral without an actual successful token transfer.
