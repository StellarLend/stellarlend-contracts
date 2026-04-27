# Cross-Contract Testing Framework

This document outlines the testing framework for verifying interactions between the core `hello-world` protocol contract and external contracts (Tokens, Flash Loan Receivers, AMMs).

## Architecture

The framework leverages Soroban SDK's `testutils` to simulate a multi-contract environment within a single test execution context.

### Key Components

1.  **Mock Contracts**:
    *   `MockFlashLoanReceiver`: Implements the `receive_flash_loan` callback interface. Configurable to test success, failure (panic), and malicious behavior (re-entrancy).
    *   `MockToken`: Standard Stellar Asset Contract provided by `testutils` to simulate assets like USDC.

2.  **Test Scenarios**:
    *   **Happy Path**: Verify successful execution of complex flows (Flash Loan -> Callback -> Repay).
    *   **Error Propagation**: Verify that failures in external contracts (e.g., token transfer failure) correctly revert the entire transaction.
    *   **Security Checks**:
        *   **Re-entrancy**: Attempt to call back into the protocol during a flash loan execution.
        *   **Liquidity Checks**: Verify operations fail when protocol lacks liquidity.
        *   **Auth**: Verify authorization failures (though handled mostly by SDK mock auth).

## Assumptions & Trust Model

1.  **Token Contracts**: We assume standard Stellar Asset Contract behavior. Custom token implementations with malicious `transfer` logic are out of scope for *protocol* correctness unless they exploit re-entrancy.
2.  **Callback Trust**: The protocol does *not* trust the callback contract. All state changes must be validated *after* the callback returns (e.g., balance checks).
3.  **Atomicity**: Soroban transactions are atomic. If any part fails (panic), the whole transaction rolls back. This simplifies error handling but requires precise `try_` calls if partial failure handling is desired (not common in this protocol).

## Running Tests

To run the cross-contract test suite:

```bash
cargo test --package hello-world --lib cross_contract_test
```

## Security Rationale

### Re-entrancy Protection
The protocol uses a `FlashLoanRecord` to track active loans.
*   **Mechanism**: `execute_flash_loan` checks if a loan is active for the (user, asset) pair. If so, it reverts with `FlashLoanError::Reentrancy`.
*   **Verification**: The `test_flash_loan_reentrancy_block` test confirms that a second call to `execute_flash_loan` within the callback fails.

### Flash Loan Safety
*   **Repayment Check**: The protocol currently relies on the user (or receiver) calling `repay_flash_loan` within the same transaction.
    *   *Note*: The current implementation allows a user to take a flash loan and *not* repay if the transaction succeeds without calling `repay`. This is a known design choice in the current version (likely for simplicity or specific use case), but the test framework highlights this by manually calling `repay`. A production version should enforce repayment via `Require(Auth)` or a callback return value check.
*   **Liquidity**: The protocol checks `balance >= amount` before transfer. Verified by `test_flash_loan_insufficient_liquidity`.

## Gas & Resource Limits
Tests run in the Soroban test environment which simulates ledger limits.
*   **Depth Limit**: Cross-contract calls consume stack depth. The framework tests nested calls (Provider -> Receiver -> Provider).
*   **Budget**: Tests will fail if they exceed the default test budget. Complex tests implicitly verify we are within limits.
