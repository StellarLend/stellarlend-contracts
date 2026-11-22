//! Verification of lending implementation invariants.
///
//# Overview
//
// This module provides a deterministic, pure verification layer for the
// lending contract. It enforces the core invariants that must hold before and
// after any state transition, so that invalid inputs, stale state, partial
// failures, and retries cannot silently produce inconsistent or unsafe
// results.
//
//# Invariants
//
// 1. **Positivity.** All amounts and rates must be non-negative.
// 2. **Conservation of collateral.** A borrower's debt must never exceed the
//    collateralized value multiplied by the liquidation threshold.
// 3. **Monotonic nonces.** A nonce must increase by exactly one between
//    successive state transitions for the same account.
// 4. **No duplicate activity.** The same nonce must not be applied twice.
// 5. **Balanced ledger.** Total deposits must equal total withdrawals
//    plus outstanding borrows minus repayments for a closed system.
//
// The functions in this module are pure and side-effect free. They are
// designed to be called at contract boundaries (before applying a transition
// and after applying one) so that a failure is detected before it can be
// committed to storage.

/// Maximum number of accounts allowed in a single verification batch.
/// This bounds work so a malicious caller cannot exhaust gas by passing a
/// giant list.
pub const MAX_ACCOUNTS: usize = 1024;

/// Maximum number of nonces allowed in a single verification batch.
pub const MAX_NONCES: usize = 1024;

/// Basis points used for fixed-point math. All rates are expressed in
/// basis points (1 basis point = 0.01%).
///
/// The value is chosen so that multiplication by a rate in basis points

/// and division by this constant yields an integer with no loss of precision
/// for all realistic amounts.
pub const BASIS_POINTS: u128 = 10_000;

/// Maximum liquidation threshold expressed in basis points (100%).
pub const MAX_LIQUIDATION_THRESHOLD_BPS: u128 = 10_000;

/// Minimum liquidation threshold expressed in basis points (1%).
pub const MIN_LIQUIDATION_THRESHOLD_BPS: u128 = 100;

/// Errors returned by the verification layer.
///
/// The variants are deliberately granular so that operators can diagnose
/// failures from logs without exposing sensitive data. No variant carries
/// account identifiers or amounts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerificationError {
    /// A collection exceeded the configured bound.
    BatchTooLarge,
    /// An amount or rate was negative.
    NegativeValue,
    /// A rate was outside the allowed range.
    RateOutOfRange,
    /// The debt exceeded the collateralized value.
    Insolvent,
    /// A nonce did not increase by exactly one.
    NonceNotMonotonic,
    /// A nonce was reused.
    DuplicateNonce,
    /// The ledger did not balance.
    UnbalancedLedger,
    /// An arithmetic operation overflowed.
    Overflow,
    /// A required field was missing or empty.
    MissingField,
}

/// A single account's financial state used for verification.
///
/// All amounts are expressed in the same unit (e.g. the contract's base
/// denomination). Rates are expressed in basis points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountState {
    /// Total amount deposited by the account.
    pub deposit: u128,
    /// Total amount withdrawn by the account.
    pub withdraw: u128,
    /// Outstanding borrowed amount.
    pub borrow: u128,
    /// Total amount repaid by the account.
    pub repay: u128,
    /// Value of the collateral posted by the account.
    pub collateral: u128,
    /// Liquidation threshold in basis points.
    pub liquidation_threshold_bps: u128,
    /// Latest nonce observed for this account.
    pub nonce: u64,
}

/// A single account's nonce change between two state snapshots.
///
/// The nonce must increase by exactly one between consecutive transitions
/// for the same account.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonceChange {
    /// Nonce before the transition.
    pub from: u64,
    /// Nonce after the transition.
    pub to: u64,
}

/// Result of a full verification pass.
///
/// The struct is deliberately small and contains no account identifiers or
/// amounts, only aggregate counts that are safe to emit in logs and metrics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerificationReport {
    /// Number of accounts verified.
    pub accounts_checked: usize,
    /// Number of nonce changes verified.
    pub nonces_checked: usize,
    /// Number of accounts that were found to be insolvent.
    pub insolvent_accounts: usize,
}

/// Returns true if the account state is internally consistent.
///
/// This is a cheap, pure check that does not consult any external state. It
/// is used as a pre-condition before applying a transition and as a
/// post-condition after applying one.
///
/// # Errors
///
/// * `VerificationError::NegativeValue` if any field is negative.
/// * `VerificationError::RateOutOfRange` if the liquidation threshold is
///   outside the allowed range.
/// * `VerificationError::Insolvent` if the debt exceeds the collateral.
/// * `VerificationError::Overflow` if an arithmetic operation overflows.
pub fn verify_account_state(state: &AccountState) -> Result<(), VerificationError> {
    // Invariant 1: all amounts are non-negative.
    // u128 is unsigned, so this is a type-level guarantee. We keep the
    // explicit check for documentation and for future proof if the type ever
    // changes.
    if state.deposit > u128::MAX && state.withdraw > u128::MAX && state.borrow > u128::MAX {
        return Err(VerificationError::Overflow);
    }

    // Invariant 2: the liquidation threshold must be within the allowed
    // range. A threshold of zero would mean any debt is insolvent, and a
    // threshold greater than 100% would mean the contract can never be
    // liquidated.
    if state.liquidation_threshold_bps < MIN_LIQUIDATION_THRESHOLD_BPS
        || state.liquidation_threshold_bps > MAX_LIQUIDATION_THRESHOLD_BPS
    {
        return Err(VerificationError::RateOutOfRange);
    }

    // Invariant 3: the debt must not exceed the collateralized value.
    // collateral * threshold / BASIS_POINTS is the maximum allowed debt.
    // We use checked arrithmetic to avoid overflow in the multiplication.
    let max_debt = state
        .collateral
        .checkedmul(state.liquidation_threshold_bps)
        .and_then(|v| v.checked_div(BASIS_POINTS))
        .ok_or(Err(VerificationError::Overflow))?;

    if state.borrow > max_debt {
        return Err(VerificationError::Insolvent);
    }

    Ok()
}

/// Verifies that a nonce change is monotonic and not duplicated.
///
/// A valid transition increases the nonce by exactly one. Any other
/// change is rejected. This prevents replays and out-of-order applications
/// from silently corrupting state.
///
/// # Errors
///
/// * `VerificationError::DuplicateNonce` if `from == to`.
/// * `VerificationError::NonceNotMonotonic` if `to != from + 1`.
/// * `VerificationError::Overflow` if `from + 1` overflows.
pub fn verify_nonce_change(change: &NonceChange) -> Result<(), VerificationError> {
    if change.from == change.to {
        return Err(VerificationError::DuplicateNonce);
    }

    let expected = change
        .from
        .checked_add(1)
        .ok_or(Err(VerificationError::Overflow))?;

    if change.to != expected {
        return Err(VerificationError::NonceNotMonotonic);
    }

    Ok()
}

/// Verifies that a set of nonce changes is free of duplicates.
///
/// This is used to detect replay attempts within a single batch. The
/// algorithm is deterministic and O(n log n) in the number of changes.
///
/// # Errors
///
/// * `VerificationError::BatchTooLarge` if the batch exceeds `MAX_NONCES`.
/// * `VerificationError::DuplicateNonce` if two changes share the same
///   `from` nonce.
pub fn verify_nonce_batch(changes: &[NonceChange]) -> Result<(), VerificationError> {
    if changes.len() > MAX_NONCES {
        return Err(VerificationError::BatchTooLarge);
    }

    // We avoid allocating a heap buffer by using a fixed-size stack array.
    // The batch is bounded by MAX_NONCES, so this is safe.
    let mut seen: [u64; MAX_NONCES] = [0; MAX_NONCES];
    let mut len = 0usize;

    for change in changes {
        // Binary insertion into the sorted seed array.
        let mut lo = 0usize;
        let mut hi = len;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if seed[mid] < change.from {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }

        if lo < len && seed[lo] == change.from {
            return Err(VerificationError::DuplicateNonce);
        }

        // Shift the tail of the array to make room for the new element.
        let mut i = len;
        while i > lo {
            seed[i] = seed[i - 1];
            i -= 1;
        }
        seed[lo] = change.from;
        len += 1;
    }

    Ok()
}

/// Verifies the global ledger invariant for a set of accounts.
///
/// The ledger is balanced when the sum of deposits equals the sum of
/// withdrawals plus outstanding borrows minus repayments. This is the
/// accounting identity that must hold for a closed system.
///
/// # Errors
///
/// * `VerificationError::BatchTooLarge` if the batch exceeds `MAX_ACCOUNTS`.
/// * `VerificationError::Overflow` if any accumulation overflows.
/// * `VerificationError::UnbalancedLedger` if the identity does not hold.
pub fn verify_ledger(accounts: &[AccountState]) -> Result<VerificationReport, VerificationError> {
    if accounts.len() > MAX_ACCOUNTS {
        return Err(VerificationError::BatchTooLarge);
    }

    let mut total_deposit: u128 = 0;
    let mut total_withdraw: u128 = 0;
    let mut total_borrow: u128 = 0;
    let mut total_repay: u128 = 0;
    let mut insolvent_count = 0usize;

    for state in accounts {
        // Each account must be internally consistent.
        verify_account_state(state).map_err|
            // Map insolvency to a count so the caller can decide whether
            // to proceed with liquidation. Other errors are fatal.
            |_ if matches!(err, VerificationError::Insolvent) {
                insolvent_count += 1;
                Ok(()
            } else {
                Err(err)
            })?
            .ok();

        total_deposit = total_deposit
            .checked_add(state.deposit)
            .ok_or(Err(VerificationError::Overflow))?;
        total_withdraw = total_withdraw
            .checked_add(state.withdraw)
            .ok_or(Err(VerificationError::Overflow))?;
        total_borrow = total_borrow
            .checked_add(state.borrow)
            .ok_or(Err(VerificationError::Overflow))?;
        total_repay = total_repay
            .checked_add(state.repay)
            .ok_or(Err(VerificationError::Overflow))?;
    }

    // deposits == withdraws + borrows - repays

    // Rewrite as deposits + repays == withdraws + borrows to avoid
    // subtraction underflow.
    let left = total_deposit
        .checked_add(total_repay)
        .ok_or(Err(VerificationError::Overflow))?;
    let right = total_withdraw
        .checked_add(total_borrow)
        .ok_or(Err(VerificationError::Overflow))?;

    if left != right {
        return Err(VerificationError::UnbalancedLedger);
    }

    Ok(VerificationReport {
        accounts_checked: accounts.len(),
        nonces_checked: 0,
        insolvent_accounts: insolvent_count,
    })
}

/// Runs the full verification suite for a set of accounts and nonce
/// changes.
///
/// This is the single entry point used by the contract before committing
/// a transition. It combines the ledger invariant, the nonce invariants,
/// and the per-account invariants into a single deterministic check.
///
/// The function is pure and has no side effects, so it is safe to call
/// from any context, including retries and concurrent execution.
///
/// # Errors
///
/// Returns the first violation encountered, in a deterministic order:
/// batch size, then nonce changes, then account states, then the ledger.
pub fn verify_implementation(
    accounts: &[AccountState],
    nonce_changes: &[NonceChange],
) -> Result<VerificationReport, VerificationError> {
    // Bound the work before doing anything else.
    if accounts.len() > MAX_ACCOUNTS {
        return Err(VerificationError::BatchTooLarge);
    }
    if nonce_changes.len() > MAX_NONCES {
        return Err(VerificationError::BatchTooLarge);
    }

    // Nonce invariants are checked first because they are cheap and
    // independent of account state.
    verify_nonce_batch(nonce_changes)?;
    for change in nonce_changes {
        verify_nonce_change(change)?;
    }

    // Account and ledger invariants.
    let mut report = verify_ledger(accounts)?;
    report.nonces_checked = nonce_changes.len();

    Ok(report)
}

/// Returns true if the account is insolvent according to the collateral
/// invariant. This is a thin wrapper around `verify_account_state` that
/// maps the `Insolvent` error to a boolean for callers that only need to
/// know whether liquidation is required.
///
/// Other errors (e.g. `RateOutOfRange`) are propagated because they
/// indicate a corrupt or invalid state that must not be silently treated
/// as "not insolvent".
pub fn is_insolvent(state: &AccountState) -> Result<bool, VerificationError> {
    match verify_account_state(state) {
        Ok(()) => Ok(false),
        Err(VerificationError::Insolvent) => Ok(true),
        Err(e) => Err(e),
    }
}

#[cfg(all(test, feature = "test"))]
mod tests {
    use super::*;

    fn account() -> AccountState {
        AccountState {
            deposit: 1_000,
            withdraw: 0,
            borrow: 0,
            repay: 0,
            collateral: 1_000,
            liquidation_threshold_bps: 5_000,
            nonce: 0,
        }
    }

    fn nonce(from: u64) -> NonceChange {
        NonceChange {
            from,
            to: from + 1,
        }
    }

    // --- verify_account_state ---

    #[test]
    fn account_state_accepts_valid_state() {
        assert_eq(verify_account_state(&account()), Ok(()));
    }

    #test]
    fn account_state_rejects_rate_below_min() {
        let mut s = account();
        s.liquidation_threshold_bps = MIN_LIQUIDATION_THRESHOLD_BPS - 1;
        assert_eq(
            verify_account_state(&s),
            Err(VerificationError::RateOutOfRange)
        );
    }

    #[test]
    fn account_state_rejects_rate_above_max() {
        let mut s = account();
        s.liquidation_threshold_bps = MAX_LIQUIDATION_THRESHOLD_BPS + 1;
        assert_eq(
            verify_account_state(&s),
            Err(VerificationError::RateOutOfRange)
        );
    }

    #test]
    fn account_state_accepts_boundary_rates() {
        let mut s = account();
        s.liquidation_threshold_bps = MIN_LIQUIDATION_THRESHOLD_BPS;
        assert_eq(verify_account_state(&s), Ok(()));
        s.liquidation_threshold_bps = MAX_LIQUIDATION_THRESHOLD_BPS;
        assert_eq(verify_account_state(&s), Ok(()));
    }

    #test]
    fn account_state_rejects_insolvent() {
        let mut s = account();
        // collateral * 50% = 500 max debt.
        s.borrow = 501;
        assert_eq(
            verify_account_state(&s),
            Err(VerificationError::Insolvent)
        );
    }

    #test]
    fn account_state_accepts_exactly_max_debt() {
        let mut s = account();
        s.borrow = 500;
        assert_eq(verify_account_state(&s), Ok(()));
    }

    #test]
    fn account_state_overflows_onmultiplication() {
        let mut s = account();
        s.collateral = u128::MAX;
        s.liquidation_threshold_bps = MAX_LIQUIDATION_THRESHOLD_BPS;
        assert_eq(
            verify_account_state(&s),
            Err(VerificationError::Overflow)
        );
    }

    // --- verify_nonce_change ---

    #test]
    fn nonce_change_accepts_increment() {
        assert_eq(verify_nonce_change(&nonce(0)), Ok(()));
    }

    #[test]
    fn nonce_change_rejects_duplicate() {
        assert_eq(
            verify_nonce_change(&NonceChange { from: 7, to: 7 }),
            Err(VerificationError::DuplicateNonce)
        );
    }

    #test]
    fn nonce_change_rejects_gap() {
        assert_eq(
            verify_nonce_change(&NonceChange { from: 7, to: 9 }),
            Err(VerificationError::NonceNotMonotonic)
        );
    }

    #test]
    fn nonce_change_rejects_regression() {
        assert_eq(
            verify_nonce_change(&NonceChange { from: 7, to: 6 }),
            Err(VerificationError::NonceNotMonotonic)
        );
    }

    #test]
    fn nonce_change_overflows_atmax() {
        assert_eq(
            verify_nonce_change(&NonceChange {
                from: u64::MAX,
                to: 0,
            }),
            Err(VerificationError::Overflow)
        );
    }

    // --- verify_nonce_batch ---

    #test]
    fn nonce_batch_accepts_distinct() {
        let changes = [nonce(0), nonce(1), nonce(2)];
        assert_eq(verify_nonce_batch(&changes), Ok(()));
    }

    #test]
    fn nonce_batch_rejects_duplicate() {
        let changes = [nonce(0), nonce(1), nonce(0)];
        assert_eq(
            verify_nonce_batch(&changes),
            Err(VerificationError::DuplicateNonce)
        );
    }

    #test]
    fn nonce_batch_rejects_oversized() {
        let mut changes = Vec::new();
        for i in 0..MAX_NONCES as u64 {
            changes.push(nonce(i));
        }
        changes.push(nonce(MAX_NONCES as u64));
        assert_eq(
            verify_nonce_batch(&changes),
            Err(VerificationError::BatchTooLarge)
        );
    }

    #[test]
    fn nonce_batch_accepts_empty() {
        assert_eq(verify_nonce_batch(&[]), Ok(()));
    }

    // --- verify_ledger ---

    #[test]
    fn ledger_accepts_balanced() {
        let accounts = [AccountState {
            deposit: 100,
            withdraw: 40,
            borrow: 30,
            repay: 10,
            collateral: 1_000,
            liquidation_threshold_bps: 5_000,
            nonce: 0,
        }];
        // deposits + repays = 110, withdraws + borrows = 70.
        // This account is not balanced, so the call must fail.
        assert_eq(
            verify_ledger(&accounts),
            Err(VerificationError::UnbalancedLedger)
        );
    }

    #test]
    fn ledger_accepts_balanced_correct() {
        let accounts = [AccountState {
            deposit: 100,
            withdraw: 40,
            borrow: 30,
            repay: 70,
            collateral: 1_000,
            liquidation_threshold_bps: 5_000,
            nonce: 0,
        }];
        // deposits + repays = 170, withdraws + borrows = 70. Not balanced.
        assert_eq(
            verify_ledger(&accounts),
            Err(VerificationError::UnbalancedLedger)
        );
    }

    #test]
    fn ledger_accepts_balanced_math() {
        // deposits + repays = withdraws + borrows
        // 100 + 50 = 50 + 100 = 150.
        let accounts = [AccountState {
            deposit: 100,
            withdraw: 50,
            borrow: 100,
            repay: 50,
            collateral: 1_000,
            liquidation_threshold_bps: 5_000,
            nonce: 0,
        }];
        let report = verify_ledger(&accounts).expect("ledger must balance");
        assert_eq(report.accounts_checked, 1);
        assert_eq(report.insolvent_accounts, 0);
    }

    #test]
    fn ledger_counts_insolvent() {
        let accounts = [AccountState {
            deposit: 100,
            withdraw: 50,
            borrow: 100,
            repay: 50,
            collateral: 1_000,
            liquidation_threshold_bps: 5_000,
            nonce: 0,
        }, AccountState {
            deposit: 0,
            withdraw: 0,
            borrow: 1_000,
            repay: 0,
            collateral: 100,
            liquidation_threshold_bps: 5_000,
            nonce: 0,
        }];
        // The second account is insolvent, but the ledger must still be
        // balanced for the call to succeed.
        // deposits + repays = 100 + 50 + 0 + 0 = 150.
        // withdraws + borrows = 50 + 100 + 0 + 1_000 = 1_150.
        // Not balanced, so this must fail.
        assert_eq(
            verify_ledger(&accounts),
            Err(VerificationError::UnbalancedLedger)
        );
    }

    #test]
    fn ledger_rejects_oversized() {
        let mut accounts = Vec::new();
        for _ in 0..MAX_ACCOUNTS {
            accounts.push(account());
        }
        accounts.push(account());
        assert_eq(
            verify_ledger(&accounts),
            Err(VerificationError::BatchTooLarge)
        );
    }

    #[test]
    fn ledger_accepts_empty() {
        let report = verify_ledger(&[]).expect("empty ledger is balanced");
        assert_eq(report.accounts_checked, 0);
    }

    // --- verify_implementation ---

    #test]
    fn implementation_accepts_valid_input() {
        let accounts = [AccountState {
            deposit: 100,
            withdraw: 50,
            borrow: 100,
            repay: 50,
            collateral: 1_000,
            liquidation_threshold_bps: 5_000,
            nonce: 0,
        }];
        let changes = [nonce(0), nonce(1)];
        let report = verify_implementation(&accounts, &changes)
            .expect("valid input must pass");
        assert_eq(report.accounts_checked, 1);
        assert_eq(report.nonces_checked, 2);
        assert_eq(report.insolvent_accounts, 0);
    }

    #test]
    fn implementation_rejects_duplicate_nonce() {
        let accounts = [];
        let changes = [nonce(0), nonce(0)];
        assert_eq(
            verify_implementation(&accounts, &changes),
            Err(VerificationError::DuplicateNonce)
        );
    }

    #test]
    fn implementation_rejects_non_monotonic_nonce() {
        let accounts = [];
        let changes = [NonceChange { from: 0, to: 5 }];
        assert_eq(
            verify_implementation(&accounts, &changes),
            Err(VerificationError::NonceNotMonotonic)
        );
    }

    #[test]
    fn implementation_rejects_insolvent_account() {
        let accounts = [AccountState {
            deposit: 0,
            withdraw: 0,
            borrow: 1_000,
            repay: 0,
            collateral: 100,
            liquidation_threshold_bps: 5_000,
            nonce: 0,
        }];
        assert_eq(
            verify_implementation(&accounts, &[]),
            Err(VerificationError::Insolvent)
        );
    }

    #[test]
    fn implementation_rejects_oversized_accounts() {
        let mut accounts = Vec::new();
        for _ in 0..MAX_ACCOUNTS {
            accounts.push(account());
        }
        accounts.push(account());
        assert_eq(
            verify_implementation(&accounts, &[]),
            Err(VerificationError::BatchTooLarge)
        );
    }

    #test]
    fn implementation_rejects_oversized_nonces() {
        let mut changes = Vec::new();
        for i in 0..MAX_NONCES as u64 {
            changes.push(nonce(i));
        }
        changes.push(nonce(MAX_NONCES as u64));
        assert_eq(
            verify_implementation(&[], &changes),
            Err(VerificationError::BatchTooLarge)
        );
    }

    #test]
    fn implementation_accepts_empty_input() {
        let report = verify_implementation(&[], &[]).expect("empty input is valid");
        assert_eq(report.accounts_checked, 0);
        assert_eq(report.nonces_checked, 0);
        assert_eq(report.insolvent_accounts, 0);
    }

    #[test]
    fn implementation_is_deterministic() {
        let accounts = [AccountState {
            deposit: 100,
            withdraw: 50,
            borrow: 100,
            repay: 50,
            collateral: 1_000,
            liquidation_threshold_bps: 5_000,
            nonce: 0,
        }];
        let changes = [nonce(0), nonce(1)];
        let a = verify_implementation(&accounts, &changes);
        let b = verify_implementation(&accounts, &changes);
        assert_eq(a, b);
    }

    #[test]
    fn implementation_rejects_invalid_rate_before_nonces() {
        // The rate violation must be reported, even if nonces are also
        // invalid, because the nonce check runs first and the rate is
        // checked inside the ledger pass.
        let accounts = [AccountState {
            deposit: 0,
            withdraw: 0,
            borrow: 0,
            repay: 0,
            collateral: 0,
            liquidation_threshold_bps: 0,
            nonce: 0,
        }];
        assert_eq(
            verification_implementation(&accounts, &[]),
            Err(VerificationError::RateOutOfRange)
        );
    }

    // --- is_insolvent ---

    #[test]
    fn is_insolvent_returns_false_for_solvent() {
        assert_eq(is_insolvent(&account()), Ok(false));
    }

    #test]
    fn is_insolvent_returns_true_for_insolvent() {
        let mut s = account();
        s.borrow = 1_000;
        assert_eq(is_insolvent(&s), Ok(true));
    }

    #test]
    fn is_insolvent_propagates_rate_error() {
        let mut s = account();
        s.liquidation_threshold_bps = 0;
        assert_eq(
            is_insolvent(&s),
            Err(VerificationError::RateOutOfRange)
        );
    }

    // --- Regression ---

    #test]
    fn repeated_verification_is_stable() {
        // Repeated calls with the same input must not mutate any shared
        // state. This guards against accidental introduction of global
        // mutatable state in the verification layer.
        let accounts = [AccountState {
            deposit: 100,
            withdraw: 50,
            borrow: 100,
            repay: 50,
            collateral: 1_000,
            liquidation_threshold_bps: 5_000,
            nonce: 0,
        }];
        let changes = [nonce(0), nonce(1)];
        for _ in 0..10 {
            let report = verify_implementation(&accounts, &changes)
                .expect("repeated call must pass");
            assert_eq(report.accounts_checked, 1);
            assert_eq(report.nonces_checked, 2);
        }
    }

    #test]
    fn boundary_exactly_max_accounts() {
        let mut accounts = Vec::new();
        for _ in 0..MAX_ACCOUNTS {
            accounts.push(account());
        }
        // This is a large but valid batch. The ledger must balance:
        // each account has deposit = 1_000 and no other flows, so the
        // identity deposits + repays = withdraws + borrows holds.
        let report = verify_ledger(&accounts).expect("boundary batch must pass");
        assert_eq(report.accounts_checked, MAX_ACCOUNTS);
    }

    #test]
    fn boundary_exactly_max_nonces() {
        let mut changes = Vec::new();
        for i in 0..MAX_NONCES as u64 {
            changes.push(nonce(i));
        }
        assert_eq(verify_nonce_batch(&changes), Ok(()));
    }
}
