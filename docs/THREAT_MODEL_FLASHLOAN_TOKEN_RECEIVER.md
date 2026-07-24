# Threat Model: Flash Loans & Token Receiver Composability

## 1. Overview

This document outlines the threat model for two high-risk composability surfaces in the StellarLend protocol:

- Flash loan callbacks
- Token receiver hooks

These components allow external contract interaction, increasing the attack surface for reentrancy, state manipulation, and unexpected execution paths.


## 2. Attack Surfaces

### 2.1 Flash Loan Callback

Flash loans allow a borrower to:
1. Receive assets
2. Execute arbitrary logic via a callback
3. Repay within the same transaction

Risk arises because external contracts gain temporary control of funds and execution flow.


### 2.2 Token Receiver Hooks

Token transfers may trigger:
- Receiver contract logic
- External execution during transfer flows

This introduces risk of:
- Reentrancy
- Malicious contract execution
- State inconsistency


## 3. Attacker Goals

An attacker may attempt to:

- Re-enter contract functions before state updates finalize
- Manipulate internal accounting during callbacks
- Avoid repayment in flash loan flows
- Trigger unexpected execution via token receiver hooks
- Exploit ordering of state changes


## 4. Attack Trees

### Flash Loan Attack Path

- Borrow funds
  - Trigger callback
    - Attempt reentrancy into lending functions
    - Modify collateral/debt state
    - Avoid repayment

### Token Receiver Attack Path

- Initiate token transfer
  - Trigger receiver hook
    - Call back into protocol
    - Exploit partial state updates
    - Cause inconsistent balances


## 5. Invariants Enforced

The protocol enforces the following invariants:

- Flash loans must be repaid within the same transaction
- No persistent state changes remain if repayment fails
- Reentrancy into sensitive functions is restricted
- Internal accounting must remain consistent before and after external calls
- State transitions are validated before finalization


## 6. Defenses & Mitigations

### Reentrancy Protection

- Guard mechanisms prevent nested calls into sensitive functions
- Critical sections avoid unsafe external calls before state updates

Refer to:
- `REENTRANCY_GUARANTEES.md`


### Atomic Execution

- Flash loan operations must complete within one transaction
- Failure to repay results in full transaction revert


### Explicit Authorization

- Sensitive operations require authenticated callers
- Prevents unauthorized state mutation during callbacks


### Controlled External Calls

- External contract execution is minimized and carefully ordered
- State is updated before or protected during external interactions


## 7. Residual Risks

- Complex composability may introduce unforeseen edge cases
- External contract behavior cannot be fully controlled
- Improper integration by third-party contracts may create vulnerabilities


## 8. Out-of-Scope

This threat model does NOT cover:

- Oracle manipulation attacks
- Governance exploits
- Off-chain infrastructure risks
- Key management failures


## 9. Test References

Security assumptions are validated through:

- Flash loan tests in lending contract modules
- Reentrancy and callback-related test scenarios
- Adversarial simulations for repayment enforcement

(Developers should reference relevant test files in `contracts/` for validation.)


## 10. Summary

Flash loan callbacks and token receiver hooks introduce high composability risk.  
The protocol mitigates these risks through:

- Strict repayment enforcement
- Reentrancy protections
- Atomic execution guarantees
- Strong invariants and validation checks

These protections ensure protocol safety even under adversarial conditions.
