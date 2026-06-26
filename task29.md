Guard repay against reducing debt below zero
Repo Avatar
StellarLend/stellarlend-contracts
Guard repay against reducing debt below zero
Description
repay in stellar-lend/contracts/lending/src/lib.rs computes new_debt = current - amount and stores it directly, so an overpayment results in a negative debt value persisted under the ("debt", user) key. Negative debt can later be misinterpreted as a credit balance by views and downstream indexers. Overpayment should be clamped to zero (or refunded) with explicit semantics.

Requirements and context
Must be secure, tested, and documented
Should be efficient and easy to review
Clamp new_debt to a floor of zero in repay in stellar-lend/contracts/lending/src/lib.rs
Decide and document overpay behavior (clamp vs reject) consistent with docs/ZERO_AMOUNT_SEMANTICS.md
Ensure get_position never reports negative debt
Add an explicit return value indicating remaining debt after repay
Suggested execution
Fork the repo and create a branch
git checkout -b bug/repay-debt-floor
Implement changes
Modify repay in stellar-lend/contracts/lending/src/lib.rs
Add tests: exact repay, overpay clamps to zero, repay with no debt
Update repay docs in docs/ and entrypoint doc comments
Validate the no-negative-debt invariant
Test and commit
Run cargo test, cover edge cases
Include test output and security notes
Example commit message
fix: clamp repay to prevent negative debt balances

Guidelines
Minimum 95 percent test coverage
Clear documentation
Timeframe: 96 hours


readthrough the project readmefile to understand the project context and structure

implement everything to the letter and make sure it passes all external checks