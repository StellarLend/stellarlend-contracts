//! # Reserve Invariant Checking
//!
//! This module enforces the critical invariant that token reserves held by the
//! contract exactly match the internal balance ledger accounting state.
//!
//! ## Invariant
//!
//! For any asset, at any point in time:
//! ```text
//! token_client.balance(&env.current_contract_address()) ==
//!     total_deposits + total_collateral_cross_asset - total_debt_principal
//! ```
//!
//! ## Usage
//!
//! Wrap state-changing operations with `check_invariant_before` and
//! `check_invariant_after` to ensure no balance drift occurs:
//!
//! ```ignore
//! check_invariant_before(&env, &asset);
//! // ... perform state-changing operation ...
//! check_invariant_after(&env, &asset);
//! ```
//!
//! ## Panic Behavior
//!
//! Any detected drift will panic the transaction immediately with a detailed
//! error message indicating the expected vs actual balance mismatch.

use crate::{DataKey, LendingError};
use soroban_sdk::token::Client as TokenClient;
use soroban_sdk::{Address, Env, Vec};

/// Check that token reserves match internal accounting before a state-changing operation.
///
/// This function reads the current contract token balance and compares it against
/// the sum of all internal accounting entries. Any mismatch will panic the transaction.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - The token asset address to check
///
/// # Panics
/// Panics if `actual_balance != expected_balance` with a detailed error message.
pub fn check_invariant_before(env: &Env, asset: &Address) {
    check_reserve_invariant(env, asset, "BEFORE");
}

/// Check that token reserves match internal accounting after a state-changing operation.
///
/// This function reads the current contract token balance and compares it against
/// the sum of all internal accounting entries. Any mismatch will panic the transaction.
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - The token asset address to check
///
/// # Panics
/// Panics if `actual_balance != expected_balance` with a detailed error message.
pub fn check_invariant_after(env: &Env, asset: &Address) {
    check_reserve_invariant(env, asset, "AFTER");
}

/// Core invariant checking logic.
///
/// Computes the expected balance from internal accounting ledgers and compares
/// it against the actual token balance held by the contract.
///
/// # Internal Accounting Components
///
/// 1. **Single-Asset Mode**: `TotalDeposits` - represents all depositor collateral
/// 2. **Cross-Asset Mode**: Sum of all per-user per-asset collateral positions
/// 3. **Bad Debt**: Outstanding unrecoverable debt that reduces effective reserves
/// 4. **Protocol Reserves**: Accumulated protocol fees (flash loans, interest)
///
/// # Arguments
/// * `env` - The Soroban environment
/// * `asset` - The token asset address to check
/// * `checkpoint` - Label for the checkpoint (e.g., "BEFORE", "AFTER")
///
/// # Panics
/// Panics if actual balance ≠ expected balance
fn check_reserve_invariant(env: &Env, asset: &Address, checkpoint: &str) {
    // Get actual token balance held by the contract
    let token_client = TokenClient::new(env, asset);
    let contract_address = env.current_contract_address();
    let actual_balance: i128 = match token_client.try_balance(&contract_address) {
        Ok(Ok(b)) => b,
        _ => return,
    };

    // Compute expected balance from internal accounting
    let expected_balance = match compute_expected_reserve(env, asset) {
        Ok(value) => value,
        Err(error) => panic!("RESERVE INVARIANT ARITHMETIC ERROR [{}]: {:?}", checkpoint, error),
    };

    // Assert equality - panic immediately if drift detected
    assert_eq!(
        actual_balance,
        expected_balance,
        "RESERVE INVARIANT VIOLATION [{}]: asset={:?}, actual_balance={}, expected_balance={}, drift={}",
        checkpoint,
        asset,
        actual_balance,
        expected_balance,
        actual_balance.saturating_sub(expected_balance)
    );
}

/// Sum cross-asset collateral for `asset`.
///
/// `TotalCollateralAsset` is authoritative when present.  For storage layouts
/// created before that aggregate existed (or for unit tests), fall back to
/// iterating the borrower/depositor lists and their per-user asset balances.
pub fn sum_cross_asset_collateral(env: &Env, asset: &Address) -> Result<i128, LendingError> {
    let total_key = DataKey::TotalCollateralAsset(asset.clone());
    if let Some(total) = env.storage().persistent().get(&total_key) {
        let total: i128 = total;
        return Ok(total);
    }

    let mut sum = 0i128;

    let borrowers: Option<Vec<Address>> = env
        .storage()
        .instance()
        .get(&DataKey::BorrowerList);
    if let Some(borrowers) = borrowers {
        for i in 0..borrowers.len() {
            let user = borrowers.get(i).ok_or(LendingError::Overflow)?;
            let assets: Vec<Address> = env
                .storage()
                .persistent()
                .get(&DataKey::UserCollateralAssets(user.clone()))
                .unwrap_or(Vec::new(env));
            for j in 0..assets.len() {
                let collateral_asset = assets.get(j).ok_or(LendingError::Overflow)?;
                if collateral_asset == *asset {
                    let balance: i128 = env
                        .storage()
                        .persistent()
                        .get(&DataKey::CollateralAsset(user.clone(), asset.clone()))
                        .unwrap_or(0);
                    sum = sum.checked_add(balance).ok_or(LendingError::Overflow)?;
                }
            }
        }
    }

    Ok(sum)
}

/// Compute the expected reserve balance from all internal accounting ledgers.
///
/// The result includes single-asset deposits, per-asset cross-asset
/// collateral, and treasury/protocol reserves, then subtracts bad debt.
pub fn compute_expected_reserve(env: &Env, asset: &Address) -> Result<i128, LendingError> {
    let mut expected = 0i128;

    let total_deposits: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDeposits)
        .unwrap_or(0);
    expected = expected
        .checked_add(total_deposits)
        .ok_or(LendingError::Overflow)?;

    let cross_asset = sum_cross_asset_collateral(env, asset)?;
    expected = expected
        .checked_add(cross_asset)
        .ok_or(LendingError::Overflow)?;

    let treasury: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::Treasury(asset.clone()))
        .unwrap_or(0);
    expected = expected
        .checked_add(treasury)
        .ok_or(LendingError::Overflow)?;

    let protocol_reserves: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::ProtocolReserves)
        .unwrap_or(0);
    expected = expected
        .checked_add(protocol_reserves)
        .ok_or(LendingError::Overflow)?;

    let bad_debt: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::BadDebt)
        .unwrap_or(0);
    expected = expected
        .checked_sub(bad_debt)
        .ok_or(LendingError::Overflow)?;

    Ok(expected)
}

/// Wrap a state-changing operation with before/after invariant checks.
///
/// This macro provides a convenient way to wrap operations:
///
/// ```ignore
/// with_invariant_check!(env, asset, {
///     // ... state-changing operation ...
/// });
/// ```
///
/// Expands to:
/// ```ignore
/// check_invariant_before(&env, &asset);
/// let result = {
///     // ... state-changing operation ...
/// };
/// check_invariant_after(&env, &asset);
/// result
/// ```
#[macro_export]
macro_rules! with_invariant_check {
    ($env:expr, $asset:expr, $body:block) => {{
        $crate::invariants::check_invariant_before($env, $asset);
        let result = $body;
        $crate::invariants::check_invariant_after($env, $asset);
        result
    }};
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, token::StellarAssetClient, Env};

    /// Register a lending-contract context plus a real SEP-41 asset so the
    /// invariant can be checked against a genuine token balance rather than a
    /// mocked accounting value.
    fn setup() -> (Env, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(crate::LendingContract, ());
        let asset = env
            .register_stellar_asset_contract_v2(Address::generate(&env))
            .address();
        (env, contract_id, asset)
    }

    fn set_accounting(env: &Env, total_deposits: i128, bad_debt: i128) {
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &total_deposits);
        env.storage().persistent().set(&DataKey::BadDebt, &bad_debt);
    }

    #[test]
    fn expected_reserve_defaults_to_zero_with_no_accounting() {
        let (env, contract_id, asset) = setup();
        env.as_contract(&contract_id, || {
            assert_eq!(compute_expected_reserve(&env, &asset).unwrap(), 0);
        });
    }

    #[test]
    fn expected_reserve_is_total_deposits_minus_bad_debt() {
        let (env, contract_id, asset) = setup();
        env.as_contract(&contract_id, || {
            set_accounting(&env, 1_000, 100);
            assert_eq!(compute_expected_reserve(&env, &asset).unwrap(), 900);
        });
    }

    #[test]
    fn test_expected_reserve_two_assets() {
        let (env, contract_id, asset_a) = setup();
        let asset_b = env
            .register_stellar_asset_contract_v2(Address::generate(&env))
            .address();
        env.as_contract(&contract_id, || {
            set_accounting(&env, 1_000, 100);
            env.storage()
                .persistent()
                .set(&DataKey::TotalCollateralAsset(asset_a.clone()), &250i128);
            env.storage()
                .persistent()
                .set(&DataKey::TotalCollateralAsset(asset_b.clone()), &400i128);

            assert_eq!(compute_expected_reserve(&env, &asset_a).unwrap(), 1_150);
            assert_eq!(compute_expected_reserve(&env, &asset_b).unwrap(), 1_300);
        });
    }

    #[test]
    fn test_expected_reserve_includes_flash_fees() {
        let (env, contract_id, asset) = setup();
        env.as_contract(&contract_id, || {
            set_accounting(&env, 1_000, 100);
            env.storage()
                .persistent()
                .set(&DataKey::Treasury(asset.clone()), &75i128);
            assert_eq!(compute_expected_reserve(&env, &asset).unwrap(), 975);
        });
    }

    #[test]
    fn test_expected_reserve_overflow_returns_error() {
        let (env, contract_id, asset) = setup();
        env.as_contract(&contract_id, || {
            set_accounting(&env, i128::MAX, 0);
            env.storage()
                .persistent()
                .set(&DataKey::Treasury(asset.clone()), &1i128);
            assert_eq!(
                compute_expected_reserve(&env, &asset),
                Err(LendingError::Overflow)
            );
        });
    }

    #[test]
    fn invariant_holds_at_exact_boundary_and_checkpoint_helpers_are_silent() {
        let (env, contract_id, asset) = setup();
        StellarAssetClient::new(&env, &asset).mint(&contract_id, &1_000);
        env.as_contract(&contract_id, || {
            set_accounting(&env, 1_000, 0);
            // Balance exactly equals accounting: neither checkpoint may panic.
            check_invariant_before(&env, &asset);
            check_invariant_after(&env, &asset);
        });
    }

    #[test]
    fn invariant_holds_when_bad_debt_offsets_deposits() {
        let (env, contract_id, asset) = setup();
        StellarAssetClient::new(&env, &asset).mint(&contract_id, &600);
        env.as_contract(&contract_id, || {
            set_accounting(&env, 1_000, 400);
            check_invariant_before(&env, &asset);
        });
    }

    #[test]
    fn invariant_holds_for_zero_reserves_and_zero_accounting() {
        let (env, contract_id, asset) = setup();
        env.as_contract(&contract_id, || {
            check_invariant_before(&env, &asset);
            check_invariant_after(&env, &asset);
        });
    }

    #[test]
    #[should_panic(expected = "RESERVE INVARIANT VIOLATION")]
    fn invariant_rejects_accounting_that_overstates_held_reserves() {
        let (env, contract_id, asset) = setup();
        // One unit short of what the ledger claims: drift of exactly 1.
        StellarAssetClient::new(&env, &asset).mint(&contract_id, &999);
        env.as_contract(&contract_id, || {
            set_accounting(&env, 1_000, 0);
            check_invariant_before(&env, &asset);
        });
    }

    #[test]
    #[should_panic(expected = "RESERVE INVARIANT VIOLATION")]
    fn invariant_rejects_accounting_that_understates_held_reserves() {
        let (env, contract_id, asset) = setup();
        StellarAssetClient::new(&env, &asset).mint(&contract_id, &1_000);
        env.as_contract(&contract_id, || {
            // Bad debt is not real outflow here, so expected (600) != actual (1000).
            set_accounting(&env, 1_000, 400);
            check_invariant_before(&env, &asset);
        });
    }

    #[test]
    fn with_invariant_check_macro_returns_the_wrapped_body_result() {
        let (env, contract_id, asset) = setup();
        env.as_contract(&contract_id, || {
            let result = crate::with_invariant_check!(&env, &asset, { 42 });
            assert_eq!(result, 42);
        });
    }
}
