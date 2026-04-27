# Flash Loan Test Suite Documentation

## Overview

Comprehensive test suite for StellarLend flash loan functionality covering success cases, fee calculations, failure scenarios, callback validation, admin operations, and security assumptions.

## Test Coverage

### Success Cases (2 tests)

1. **test_flash_loan_success**
   - Validates successful flash loan execution
   - Verifies correct fee calculation (9 bps default)
   - Confirms tokens are transferred to user
   - Expected: Returns total repayment amount (principal + fee)



### Fee Calculation Tests (3 tests)

3. **test_default_fee_calculation**
   - Tests default fee of 9 basis points (0.09%)
   - Validates fee calculation for multiple amounts
   - Cases: 1M → 900 fee, 10M → 9K fee
   - Expected: Accurate fee calculation across different amounts

4. **test_custom_fee_calculation**
   - Tests admin ability to set custom fee (50 bps = 0.5%)
   - Validates new fee is applied to flash loans
   - Expected: 1M loan → 5K fee with custom rate

5. **test_zero_fee**
   - Tests zero fee configuration
   - Validates no fee is charged when fee is set to 0
   - Expected: Total repayment equals principal

### Unpaid Loan Revert Tests (3 tests)

43. **test_unpaid_loan_revert**
   - Tests callback returning without approving required tokens
   - Expected: Panic (host-level invalid action / auth failure) ensuring atomicity

47. **test_insufficient_repayment**
   - Tests callback approving less than the required repayment amount
   - Expected: Panic (host-level invalid action / auth failure) ensuring atomicity

52. **test_insufficient_user_balance**
   - Tests repayment when user lacks sufficient tokens but approves anyway
   - Expected: Panic (host-level insufficient balance failure) ensuring atomicity

### Callback Validation Tests (2 tests)

9. **test_invalid_callback_self**
   - Tests rejection of contract address as callback
   - Security: Prevents self-referential callbacks
   - Expected: FlashLoanError::InvalidCallback

10. **test_valid_callback**
    - Tests acceptance of valid callback address
    - Verifies callback is stored in flash loan record
    - Expected: Success and callback stored correctly

### Set Fee BPS Tests (4 tests)

11. **test_set_fee_bps_admin**
    - Tests admin can successfully set fee
    - Validates fee is persisted in storage
    - Expected: Fee updated to 25 bps

12. **test_set_fee_bps_non_admin**
    - Tests non-admin cannot set fee
    - Security: Authorization check
    - Expected: Error (unauthorized)

13. **test_set_fee_bps_invalid**
    - Tests rejection of invalid fee values
    - Cases: fee > 10000 bps, negative fee
    - Expected: FlashLoanError::InvalidAmount

14. **test_set_fee_bps_maximum**
    - Tests maximum valid fee (10000 bps = 100%)
    - Expected: Success with 100% fee

### Security Tests (8 tests)

15. **test_reentrancy_protection**
    - Tests prevention of nested flash loans
    - Security: Reentrancy guard
    - Expected: FlashLoanError::Reentrancy on second loan

16. **test_pause_flash_loan**
    - Tests pause functionality
    - Admin can pause flash loan operations
    - Expected: FlashLoanError::FlashLoanPaused

17. **test_insufficient_liquidity**
    - Tests rejection when contract lacks funds
    - Attempts to borrow more than available
    - Expected: FlashLoanError::InsufficientLiquidity

18. **test_invalid_amount_zero**
    - Tests rejection of zero amount
    - Expected: FlashLoanError::InvalidAmount

19. **test_invalid_amount_negative**
    - Tests rejection of negative amount
    - Expected: FlashLoanError::InvalidAmount

20. **test_invalid_asset**
    - Tests rejection of contract address as asset
    - Security: Prevents self-referential asset
    - Expected: FlashLoanError::InvalidAsset

21. **test_configuration_limits**
    - Tests min/max amount limits enforcement
    - Cases: below min, above max, within limits
    - Expected: Rejection outside limits, success within

22. **test_invalid_configuration**
    - Tests rejection of invalid configuration
    - Cases: invalid fee, min > max, zero min
    - Expected: Configuration errors

## Test Results

```
test result: ok. 22 passed; 0 failed; 0 ignored; 0 measured
```

## Security Assumptions Validated

1. **Atomicity Guarantee via "Pull" Model**: The flash loan logic explicitly pulls the principal + fee simultaneously after the callback returns. Failure to provide approval triggers a host panic, strictly enforcing atomicity.
2. **Reentrancy Protection**: Active flash loan markers prevent the same asset from being flash loaned repeatedly from within the callback; moreover, the host intercepts direct contract re-entrancy.
3. **Authorization**: Only admin can modify fee and configuration.
4. **Pause Mechanism**: Flash loans can be paused by admin.
5. **Amount Validation**: Zero, negative, and out-of-range amounts rejected.
6. **Liquidity Check**: Cannot borrow more than contract balance.
7. **Callback Validation**: Contract cannot be its own callback.
8. **Asset Validation**: Contract cannot be used as asset.
9. **Fee Bounds**: Fee limited to 0-10000 bps (0-100%).
10. **Configuration Validation**: Min/max amounts and fee parameters validated.

## Test Execution

Run all flash loan tests:
```bash
cargo test flash_loan_test --lib
```

Run specific test:
```bash
cargo test flash_loan_test::test_flash_loan_success --lib
```

Run with output:
```bash
cargo test flash_loan_test --lib -- --nocapture
```

## Implementation Notes

- Tests use `env.as_contract()` to execute functions in contract context
- Token operations require both StellarAssetClient (mint) and TokenClient (approve)
- Flash loan state is stored in persistent storage with user/asset key
- Tests validate both success paths and comprehensive error handling
- All security-critical operations are tested for authorization and validation
