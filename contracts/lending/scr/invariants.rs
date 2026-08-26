use soroban_sdk::{token, Address, Env, String};
use crate::DataKey;

/// Macro for wrapping operations with invariant checks
/// 
/// Usage:
/// ```
/// with_invariant_check!(env, asset, {
///     // ... state-changing operation ...
/// });
/// ```
#[macro_export]
macro_rules! with_invariant_check {
    ($env:expr, $asset:expr, $body:block) => {{
        $crate::invariants::check_invariant_before(&$env, &$asset);
        let result = $body;
        $crate::invariants::check_invariant_after(&$env, &$asset);
        result
    }};
}

/// Check reserve invariant BEFORE a state-changing operation
pub fn check_invariant_before(env: &Env, asset: &Address) {
    check_invariant_impl(env, asset, "BEFORE");
}

/// Check reserve invariant AFTER a state-changing operation
pub fn check_invariant_after(env: &Env, asset: &Address) {
    check_invariant_impl(env, asset, "AFTER");
}

/// Core invariant checking implementation
fn check_invariant_impl(env: &Env, asset: &Address, checkpoint: &str) {
    // Skip invariant check during flash loan callback phase
    if env.storage().temporary().has(&DataKey::FlashActive) {
        return;
    }

    // Get actual token balance held by contract
    let token_client = token::Client::new(env, asset);
    let actual_balance = token_client.balance(&env.current_contract_address());

    // Compute expected balance from internal accounting
    let expected_balance = compute_expected_reserve(env, asset);

    // Assert exact equality - any drift is a critical error
    if actual_balance != expected_balance {
        let drift = actual_balance - expected_balance;
        panic!(
            "RESERVE INVARIANT VIOLATION [{}]: asset={:?}, actual_balance={}, expected_balance={}, drift={}",
            checkpoint,
            asset,
            actual_balance,
            expected_balance,
            drift
        );
    }
}

/// Compute expected reserve balance from internal accounting ledgers
pub fn compute_expected_reserve(env: &Env, asset: &Address) -> i128 {
    let mut expected: i128 = 0;

    // 1. Add total deposits (single-asset mode)
    let total_deposits: i128 = env.storage()
        .persistent()
        .get(&DataKey::TotalDeposits(asset.clone()))
        .unwrap_or(0);
    expected = expected.checked_add(total_deposits).unwrap_or_else(|| {
        panic!("Overflow computing expected reserve: total_deposits={}", total_deposits);
    });

    // 2. Add protocol treasury (accumulated fees)
    let treasury_balance: i128 = env.storage()
        .persistent()
        .get(&DataKey::Treasury(asset.clone()))
        .unwrap_or(0);
    expected = expected.checked_add(treasury_balance).unwrap_or_else(|| {
        panic!("Overflow computing expected reserve: treasury_balance={}", treasury_balance);
    });

    // 3. Subtract bad debt (unrecoverable losses)
    let bad_debt: i128 = env.storage()
        .persistent()
        .get(&DataKey::BadDebt(asset.clone()))
        .unwrap_or(0);
    expected = expected.checked_sub(bad_debt).unwrap_or_else(|| {
        panic!("Underflow computing expected reserve: bad_debt={}", bad_debt);
    });

    // Note: Cross-asset collateral would require iterating all users
    // For now, this implementation focuses on single-asset accounting
    // Future enhancement: aggregate CollateralAsset(user, asset) across all users

    expected
}

// ========================================
// TESTS
// ========================================

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{testutils::Address as _, token, Address, Env};

    #[test]
    fn test_invariant_passes_when_balanced() {
        let env = Env::default();
        let asset = Address::generate(&env);
        let contract_addr = Address::generate(&env);
        
        // Mock token client would need to return matching balance
        // This test requires proper mocking infrastructure
        // For now, this demonstrates the test structure
    }

    #[test]
    #[should_panic(expected = "RESERVE INVARIANT VIOLATION")]
    fn test_invariant_panics_on_drift() {
        let env = Env::default();
        let asset = Address::generate(&env);
        
        // Set up scenario where actual != expected
        env.storage().persistent().set(&DataKey::TotalDeposits(asset.clone()), &1000i128);
        
        // This should panic because token balance (0) != expected (1000)
        check_invariant_before(&env, &asset);
    }

    #[test]
    fn test_compute_expected_reserve_aggregates_correctly() {
        let env = Env::default();
        let asset = Address::generate(&env);
        
        // Set up accounting state
        env.storage().persistent().set(&DataKey::TotalDeposits(asset.clone()), &5000i128);
        env.storage().persistent().set(&DataKey::Treasury(asset.clone()), &200i128);
        env.storage().persistent().set(&DataKey::BadDebt(asset.clone()), &50i128);
        
        let expected = compute_expected_reserve(&env, &asset);
        
        // Expected = 5000 + 200 - 50 = 5150
        assert_eq!(expected, 5150);
    }

    #[test]
    fn test_invariant_skipped_during_flash_loan() {
        let env = Env::default();
        let asset = Address::generate(&env);
        
        // Set flash loan guard
        env.storage().temporary().set(&DataKey::FlashActive, &true);
        
        // Set up mismatched state (would normally panic)
        env.storage().persistent().set(&DataKey::TotalDeposits(asset.clone()), &1000i128);
        
        // Should NOT panic because flash loan is active
        check_invariant_before(&env, &asset);
    }
}
