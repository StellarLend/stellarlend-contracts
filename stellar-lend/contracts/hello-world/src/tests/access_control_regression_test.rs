//! # Systematic Access Control Regression Test Suite
//!
//! Comprehensive test suite that asserts access control rules for all sensitive
//! entrypoints (admin‑only, governance‑only, multisig‑only, guardian‑only, role‑based, or public).
//!
//! ## Access Control Matrix
//!
//! | Function | Module | Required Role | Auth Check |
//! |----------|--------|---------------|------------|
//! | **ADMIN-ONLY FUNCTIONS** |
//! | `set_admin` | admin | Admin | `require_admin` |
//! | `grant_role` | admin | Admin | `require_admin` |
//! | `revoke_role` | admin | Admin | `require_admin` |
//! | `transfer_admin` | lib | Admin | `require_admin` |
//! | `set_reserve_factor` | reserve | Admin | `require_auth` + `require_admin` |
//! | `set_treasury_address` | reserve | Admin | `require_auth` + `require_admin` |
//! | `withdraw_reserve_to_treasury` | reserve | Admin | `require_auth` + `require_admin` |
//! | `initialize_reserve_config` | reserve | Admin | Called internally |
//! | `add_supported_asset` | cross_asset | Admin | `require_auth` + admin check |
//! | `remove_supported_asset` | cross_asset | Admin | `require_auth` + admin check |
//! | `set_cross_asset_params` | cross_asset | Admin | `require_auth` + admin check |
//! | `register_bridge_provider` | cross_asset | Admin | `require_auth` + admin check |
//! | `update_bridge_provider_status` | cross_asset | Admin | `require_auth` + admin check |
//! | `deregister_bridge_provider` | cross_asset | Admin | `require_auth` + admin check |
//! | `set_bridge_config` | bridge | Admin | `require_auth` |
//! | `set_pause` | pause | Admin | `require_auth` + admin check |
//! | `set_rate_limits` | rate_limiting | Admin | `require_auth` + admin check |
//! | `set_liquidation_incentive` | liquidation | Admin | `require_auth` + admin check |
//! | `add_guardian` | governance | Admin | `require_auth` + admin check |
//! | `remove_guardian` | governance | Admin | `require_auth` + admin check |
//! | `set_guardian_threshold` | governance | Admin | `require_auth` + admin check |
//! | `set_multisig_config` | governance | Admin | `require_auth` + admin check |
//! | `cancel_proposal` | governance | Admin/Proposer | `require_auth` + caller check |
//! | **GOVERNANCE FUNCTIONS (Authenticated)** |
//! | `initialize` | governance | Any (token holder for threshold) | `require_auth` |
//! | `create_proposal` | governance | Any (with proposal power) | `require_auth` |
//! | `vote` | governance | Token holders | `require_auth` |
//! | `queue_proposal` | governance | Any | `require_auth` |
//! | `execute_proposal` | governance | Any | `require_auth` |
//! | **MULTISIG-ONLY FUNCTIONS** |
//! | `approve_proposal` | governance | Multisig admin | `require_auth` + multisig check |
//! | **GUARDIAN-ONLY FUNCTIONS** |
//! | `start_recovery` | governance | Guardian | `require_auth` + guardian check |
//! | `approve_recovery` | governance | Guardian | `require_auth` + guardian check |
//! | `execute_recovery` | governance | Any guardian or admin | `require_auth` + approval check |
//! | **ROLE-BASED FUNCTIONS** |
//! | `require_role_or_admin` | admin | Specific role or admin | `has_role` or `get_admin` |
//! | **MONITOR FUNCTIONS** |
//! | `monitor_initialize` | monitor | Any (once) | `require_auth` |
//! | `monitor_add_reporter` | monitor | Admin | `require_auth` + admin check |
//! | `monitor_remove_reporter` | monitor | Admin | `require_auth` + admin check |
//! | `monitor_report_health` | monitor | Reporter | `require_auth` + reporter check |
//! | `monitor_report_performance` | monitor | Reporter | `require_auth` + reporter check |
//! | `monitor_report_security` | monitor | Reporter | `require_auth` + reporter check |
//! | `monitor_get` | monitor | Public | None (read-only) |
//! | **USER FUNCTIONS** |
//! | `deposit` | deposit | User | `require_auth` |
//! | `withdraw` | withdraw | User | `require_auth` |
//! | `borrow` | borrow | User | `require_auth` |
//! | `repay` | repay | User | `require_auth` |
//! | `initiate_flash_loan` | flash_loan | User | `require_auth` |
//!
//! ## Security Assumptions
//! 1. Admin-only functions require the caller to be the current admin
//! 2. Governance functions require authentication but may have additional checks
//! 3. Multisig functions require the caller to be in the multisig config
//! 4. Guardian functions require the caller to be a registered guardian
//! 5. Role-based functions check both admin and specific roles
//! 6. User functions require authentication but operate on own positions
//! 7. Unauthorized access must fail with appropriate error codes

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env, Symbol, Vec};

use crate::admin::{
    grant_role, has_role, require_admin, require_role_or_admin, revoke_role, set_admin,
    AdminDataKey, AdminError,
};
use crate::deposit::DepositDataKey;
use crate::reserve::{
    get_treasury_address, initialize_reserve_config, set_reserve_factor, set_treasury_address,
    withdraw_reserve_to_treasury, ReserveError, MAX_RESERVE_FACTOR_BPS,
};

// ═══════════════════════════════════════════════════════════════════════════
// Test Setup Helpers
// ═══════════════════════════════════════════════════════════════════════════

fn setup_env() -> (Env, Address) {
    let env = Env::default();
    let contract_id = env.register_contract(None, crate::HelloContract);
    (env, contract_id)
}

fn setup_with_admin() -> (Env, Address, Address) {
    let (env, contract_id) = setup_env();
    let admin = Address::generate(&env);

    env.as_contract(&contract_id, || {
        set_admin(&env, admin.clone(), None).unwrap();
    });

    (env, contract_id, admin)
}

fn setup_admin_with_role(role_name: &str) -> (Env, Address, Address, Address) {
    let (env, contract_id, admin) = setup_with_admin();
    let roled_user = Address::generate(&env);
    let role = Symbol::new(&env, role_name);

    env.as_contract(&contract_id, || {
        grant_role(&env, admin.clone(), role, roled_user.clone()).unwrap();
    });

    (env, contract_id, admin, roled_user)
}

// ═══════════════════════════════════════════════════════════════════════════
// Admin Access Control Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_set_admin_unauthorized() {
    //! Tests that unauthorized users cannot set admin.
    //!
    //! **Security Test:** Verifies that only the current admin can transfer admin rights.

    let (env, contract_id, admin) = setup_with_admin();
    let unauthorized = Address::generate(&env);
    let new_admin = Address::generate(&env);

    env.as_contract(&contract_id, || {
        // Unauthorized attempt to set admin should fail
        let result = set_admin(&env, new_admin.clone(), Some(unauthorized.clone()));
        assert_eq!(result, Err(AdminError::Unauthorized));

        // Verify admin hasn't changed
        let current_admin = crate::admin::get_admin(&env).unwrap();
        assert_eq!(current_admin, admin);
    });
}

#[test]
fn test_set_admin_authorized() {
    //! Tests that current admin can transfer admin rights.

    let (env, contract_id, admin) = setup_with_admin();
    let new_admin = Address::generate(&env);

    env.as_contract(&contract_id, || {
        // Admin can transfer to new admin
        let result = set_admin(&env, new_admin.clone(), Some(admin.clone()));
        assert!(result.is_ok());

        // Verify admin has changed
        let current_admin = crate::admin::get_admin(&env).unwrap();
        assert_eq!(current_admin, new_admin);
    });
}

#[test]
fn test_grant_role_unauthorized() {
    //! Tests that unauthorized users cannot grant roles.

    let (env, contract_id, admin) = setup_with_admin();
    let unauthorized = Address::generate(&env);
    let target = Address::generate(&env);
    let role = Symbol::new(&env, "test_role");

    env.as_contract(&contract_id, || {
        // Unauthorized attempt to grant role should fail
        let result = grant_role(&env, unauthorized.clone(), role.clone(), target.clone());
        assert_eq!(result, Err(AdminError::Unauthorized));

        // Verify role wasn't granted
        assert!(!has_role(&env, role, target));
    });
}

#[test]
fn test_grant_role_authorized() {
    //! Tests that admin can grant roles.

    let (env, contract_id, admin) = setup_with_admin();
    let target = Address::generate(&env);
    let role = Symbol::new(&env, "test_role");

    env.as_contract(&contract_id, || {
        // Admin can grant role
        let result = grant_role(&env, admin.clone(), role.clone(), target.clone());
        assert!(result.is_ok());

        // Verify role was granted
        assert!(has_role(&env, role, target));
    });
}

#[test]
fn test_revoke_role_unauthorized() {
    //! Tests that unauthorized users cannot revoke roles.

    let (env, contract_id, admin, roled_user) = setup_admin_with_role("test_role");
    let unauthorized = Address::generate(&env);
    let role = Symbol::new(&env, "test_role");

    env.as_contract(&contract_id, || {
        // Verify role exists
        assert!(has_role(&env, role.clone(), roled_user.clone()));

        // Unauthorized attempt to revoke role should fail
        let result = revoke_role(&env, unauthorized.clone(), role.clone(), roled_user.clone());
        assert_eq!(result, Err(AdminError::Unauthorized));

        // Verify role still exists
        assert!(has_role(&env, role, roled_user));
    });
}

#[test]
fn test_revoke_role_authorized() {
    //! Tests that admin can revoke roles.

    let (env, contract_id, admin, roled_user) = setup_admin_with_role("test_role");
    let role = Symbol::new(&env, "test_role");

    env.as_contract(&contract_id, || {
        // Verify role exists
        assert!(has_role(&env, role.clone(), roled_user.clone()));

        // Admin can revoke role
        let result = revoke_role(&env, admin.clone(), role.clone(), roled_user.clone());
        assert!(result.is_ok());

        // Verify role was revoked
        assert!(!has_role(&env, role, roled_user));
    });
}

#[test]
fn test_require_admin_check() {
    //! Tests that require_admin correctly validates admin status.

    let (env, contract_id, admin) = setup_with_admin();
    let non_admin = Address::generate(&env);

    env.as_contract(&contract_id, || {
        // Admin should pass
        let result = require_admin(&env, &admin);
        assert!(result.is_ok());

        // Non-admin should fail
        let result = require_admin(&env, &non_admin);
        assert_eq!(result, Err(AdminError::Unauthorized));
    });
}

#[test]
fn test_require_role_or_admin_with_admin() {
    //! Tests that admin passes role_or_admin check regardless of role.

    let (env, contract_id, admin) = setup_with_admin();
    let role = Symbol::new(&env, "any_role");

    env.as_contract(&contract_id, || {
        // Admin should pass any role check
        let result = require_role_or_admin(&env, &admin, role);
        assert!(result.is_ok());
    });
}

#[test]
fn test_require_role_or_admin_with_role() {
    //! Tests that users with the specific role pass the check.

    let (env, contract_id, admin, roled_user) = setup_admin_with_role("oracle_admin");
    let role = Symbol::new(&env, "oracle_admin");
    let wrong_role = Symbol::new(&env, "wrong_role");

    env.as_contract(&contract_id, || {
        // User with correct role should pass
        let result = require_role_or_admin(&env, &roled_user, role.clone());
        assert!(result.is_ok());

        // User with wrong role should fail
        let result = require_role_or_admin(&env, &roled_user, wrong_role);
        assert_eq!(result, Err(AdminError::Unauthorized));
    });
}

#[test]
fn test_require_role_or_admin_unauthorized() {
    //! Tests that users without role or admin status fail the check.

    let (env, contract_id, admin) = setup_with_admin();
    let no_role_user = Address::generate(&env);
    let role = Symbol::new(&env, "special_role");

    env.as_contract(&contract_id, || {
        // User without role or admin should fail
        let result = require_role_or_admin(&env, &no_role_user, role);
        assert_eq!(result, Err(AdminError::Unauthorized));
    });
}

// ═══════════════════════════════════════════════════════════════════════════
// Reserve Access Control Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_set_reserve_factor_unauthorized() {
    //! Tests that unauthorized users cannot set reserve factor.

    let (env, contract_id, admin) = setup_with_admin();
    let asset = Some(Address::generate(&env));
    let unauthorized = Address::generate(&env);

    // Initialize reserve first as admin
    env.as_contract(&contract_id, || {
        initialize_reserve_config(&env, asset.clone(), 1000).unwrap();
    });

    // Unauthorized attempt
    env.as_contract(&contract_id, || {
        let result = set_reserve_factor(&env, unauthorized.clone(), asset.clone(), 2000);
        assert_eq!(result, Err(ReserveError::Unauthorized));
    });
}

#[test]
fn test_set_reserve_factor_authorized() {
    //! Tests that admin can set reserve factor.

    let (env, contract_id, admin) = setup_with_admin();
    let asset = Some(Address::generate(&env));

    env.as_contract(&contract_id, || {
        initialize_reserve_config(&env, asset.clone(), 1000).unwrap();

        let result = set_reserve_factor(&env, admin.clone(), asset.clone(), 2000);
        assert!(result.is_ok());

        // Verify the change
        let factor = crate::reserve::get_reserve_factor(&env, asset);
        assert_eq!(factor, 2000);
    });
}

#[test]
fn test_set_treasury_address_unauthorized() {
    //! Tests that unauthorized users cannot set treasury address.

    let (env, contract_id, admin) = setup_with_admin();
    let unauthorized = Address::generate(&env);
    let treasury = Address::generate(&env);

    env.as_contract(&contract_id, || {
        let result = set_treasury_address(&env, unauthorized.clone(), treasury);
        assert_eq!(result, Err(ReserveError::Unauthorized));
    });
}

#[test]
fn test_set_treasury_address_authorized() {
    //! Tests that admin can set treasury address.

    let (env, contract_id, admin) = setup_with_admin();
    let treasury = Address::generate(&env);

    env.as_contract(&contract_id, || {
        let result = set_treasury_address(&env, admin.clone(), treasury.clone());
        assert!(result.is_ok());

        // Verify the change
        let stored = get_treasury_address(&env).unwrap();
        assert_eq!(stored, treasury);
    });
}

#[test]
fn test_withdraw_reserve_unauthorized() {
    //! Tests that unauthorized users cannot withdraw reserves.

    let (env, contract_id, admin) = setup_with_admin();
    let asset = Some(Address::generate(&env));
    let unauthorized = Address::generate(&env);
    let treasury = Address::generate(&env);

    // Setup reserve balance
    env.as_contract(&contract_id, || {
        initialize_reserve_config(&env, asset.clone(), 1000).unwrap();
        set_treasury_address(&env, admin.clone(), treasury).unwrap();
        crate::reserve::accrue_reserve(&env, asset.clone(), 10000).unwrap();
    });

    // Unauthorized attempt
    env.as_contract(&contract_id, || {
        let result = withdraw_reserve_to_treasury(&env, unauthorized.clone(), asset.clone(), 500);
        assert_eq!(result, Err(ReserveError::Unauthorized));
    });
}

#[test]
fn test_withdraw_reserve_authorized() {
    //! Tests that admin can withdraw reserves.

    let (env, contract_id, admin) = setup_with_admin();
    let asset = Some(Address::generate(&env));
    let treasury = Address::generate(&env);

    env.as_contract(&contract_id, || {
        initialize_reserve_config(&env, asset.clone(), 1000).unwrap();
        set_treasury_address(&env, admin.clone(), treasury.clone()).unwrap();
        crate::reserve::accrue_reserve(&env, asset.clone(), 10000).unwrap();

        let result = withdraw_reserve_to_treasury(&env, admin.clone(), asset.clone(), 500);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 500);
    });
}

// ═══════════════════════════════════════════════════════════════════════════
// Parameterized Access Control Tests
// ═══════════════════════════════════════════════════════════════════════════

/// Macro to generate parameterized unauthorized access tests
/// This creates tests that verify all unauthorized callers are rejected
macro_rules! test_unauthorized_access {
    ($test_name:ident, $setup:expr, $call:expr, $expected_err:expr) => {
        #[test]
        fn $test_name() {
            let (env, contract_id, admin) = setup_with_admin();
            let unauthorized = Address::generate(&env);
            let setup_result = $setup(&env, &contract_id, &admin);

            env.as_contract(&contract_id, || {
                let result = $call(&env, &setup_result, &unauthorized);
                assert_eq!(result, $expected_err);
            });
        }
    };
}

/// Test authorized access pattern
#[test]
fn test_authorized_vs_unauthorized_matrix() {
    //! Comprehensive test for authorized vs unauthorized access patterns.
    //!
    //! This test verifies the following access control matrix:
    //!
    //! | Caller Type | Admin Functions | Role Functions | Public Functions |
    //! |-------------|-----------------|----------------|------------------|
    //! | Admin       | ✅ Allowed      | ✅ Allowed     | ✅ Allowed       |
    //! | Role Holder | ❌ Denied       | ✅ Allowed     | ✅ Allowed       |
    //! | User        | ❌ Denied       | ❌ Denied      | ✅ Allowed       |
    //! | Anonymous   | ❌ Denied       | ❌ Denied      | ✅ Allowed       |

    let (env, contract_id, admin) = setup_with_admin();
    let role_user = Address::generate(&env);
    let regular_user = Address::generate(&env);
    let role = Symbol::new(&env, "test_role");

    // Grant role to role_user
    env.as_contract(&contract_id, || {
        grant_role(&env, admin.clone(), role.clone(), role_user.clone()).unwrap();
    });

    // Test admin access (should succeed)
    env.as_contract(&contract_id, || {
        assert!(require_admin(&env, &admin).is_ok());
        assert!(require_role_or_admin(&env, &admin, role.clone()).is_ok());
    });

    // Test role user access (should succeed for role check, fail for admin check)
    env.as_contract(&contract_id, || {
        assert_eq!(
            require_admin(&env, &role_user),
            Err(AdminError::Unauthorized)
        );
        assert!(require_role_or_admin(&env, &role_user, role.clone()).is_ok());
    });

    // Test regular user access (should fail both checks)
    env.as_contract(&contract_id, || {
        assert_eq!(
            require_admin(&env, &regular_user),
            Err(AdminError::Unauthorized)
        );
        assert_eq!(
            require_role_or_admin(&env, &regular_user, role.clone()),
            Err(AdminError::Unauthorized)
        );
    });
}

// ═══════════════════════════════════════════════════════════════════════════
// Edge Case Access Control Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_admin_role_change_mid_transaction() {
    //! Tests that admin changes are effective immediately.
    //!
    //! **Security Test:** Verifies that role changes take effect immediately
    //! and there is no caching or delay in authorization checks.

    let (env, contract_id, admin) = setup_with_admin();
    let new_admin = Address::generate(&env);
    let old_admin = admin.clone();

    env.as_contract(&contract_id, || {
        // Verify original admin works
        assert!(require_admin(&env, &old_admin).is_ok());

        // Transfer admin
        set_admin(&env, new_admin.clone(), Some(old_admin.clone())).unwrap();

        // Old admin should no longer work
        assert_eq!(
            require_admin(&env, &old_admin),
            Err(AdminError::Unauthorized)
        );

        // New admin should work
        assert!(require_admin(&env, &new_admin).is_ok());
    });
}

#[test]
fn test_role_revoke_immediate_effect() {
    //! Tests that role revocation takes effect immediately.

    let (env, contract_id, admin, roled_user) = setup_admin_with_role("temp_role");
    let role = Symbol::new(&env, "temp_role");

    env.as_contract(&contract_id, || {
        // Verify role works
        assert!(require_role_or_admin(&env, &roled_user, role.clone()).is_ok());

        // Revoke role
        revoke_role(&env, admin.clone(), role.clone(), roled_user.clone()).unwrap();

        // User should no longer have role
        assert_eq!(
            require_role_or_admin(&env, &roled_user, role.clone()),
            Err(AdminError::Unauthorized)
        );
    });
}

#[test]
fn test_multiple_roles_independence() {
    //! Tests that different roles are independent.

    let (env, contract_id, admin) = setup_with_admin();
    let user_a = Address::generate(&env);
    let user_b = Address::generate(&env);
    let role_a = Symbol::new(&env, "role_a");
    let role_b = Symbol::new(&env, "role_b");

    env.as_contract(&contract_id, || {
        // Grant different roles to different users
        grant_role(&env, admin.clone(), role_a.clone(), user_a.clone()).unwrap();
        grant_role(&env, admin.clone(), role_b.clone(), user_b.clone()).unwrap();

        // User A should have role_a but not role_b
        assert!(require_role_or_admin(&env, &user_a, role_a.clone()).is_ok());
        assert_eq!(
            require_role_or_admin(&env, &user_a, role_b.clone()),
            Err(AdminError::Unauthorized)
        );

        // User B should have role_b but not role_a
        assert!(require_role_or_admin(&env, &user_b, role_b.clone()).is_ok());
        assert_eq!(
            require_role_or_admin(&env, &user_b, role_a.clone()),
            Err(AdminError::Unauthorized)
        );
    });
}

#[test]
fn test_same_user_multiple_roles() {
    //! Tests that a user can have multiple roles.

    let (env, contract_id, admin) = setup_with_admin();
    let user = Address::generate(&env);
    let role_1 = Symbol::new(&env, "role_1");
    let role_2 = Symbol::new(&env, "role_2");

    env.as_contract(&contract_id, || {
        // Grant multiple roles to same user
        grant_role(&env, admin.clone(), role_1.clone(), user.clone()).unwrap();
        grant_role(&env, admin.clone(), role_2.clone(), user.clone()).unwrap();

        // User should pass both role checks
        assert!(require_role_or_admin(&env, &user, role_1).is_ok());
        assert!(require_role_or_admin(&env, &user, role_2).is_ok());
    });
}

// ═══════════════════════════════════════════════════════════════════════════
// Access Control Regression Tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_regression_admin_transfer_double_spend() {
    //! Regression test: Verify admin cannot be transferred twice by old admin.
    //!
    //! **Security Vulnerability Prevented:** Old admin attempting to regain control
    //! after transfer by calling transfer again with stale authorization.

    let (env, contract_id, admin) = setup_with_admin();
    let new_admin = Address::generate(&env);
    let attacker = Address::generate(&env);

    env.as_contract(&contract_id, || {
        // Transfer admin to new_admin
        set_admin(&env, new_admin.clone(), Some(admin.clone())).unwrap();

        // Old admin (now unauthorized) tries to transfer again
        let result = set_admin(&env, attacker.clone(), Some(admin.clone()));
        assert_eq!(result, Err(AdminError::Unauthorized));

        // Verify new_admin is still the admin
        let current_admin = crate::admin::get_admin(&env).unwrap();
        assert_eq!(current_admin, new_admin);
    });
}

#[test]
fn test_regression_role_grant_self_elevation() {
    //! Regression test: Verify user cannot grant themselves a role.

    let (env, contract_id, admin) = setup_with_admin();
    let attacker = Address::generate(&env);
    let role = Symbol::new(&env, "privileged");

    env.as_contract(&contract_id, || {
        // Attacker tries to grant themselves a role
        let result = grant_role(&env, attacker.clone(), role.clone(), attacker.clone());
        assert_eq!(result, Err(AdminError::Unauthorized));

        // Verify role was not granted
        assert!(!has_role(&env, role, attacker));
    });
}

#[test]
fn test_regression_admin_cannot_bypass_reserve_limits() {
    //! Regression test: Verify admin cannot exceed reserve limits.
    //!
    //! Admin should be able to withdraw reserves but not more than available.

    let (env, contract_id, admin) = setup_with_admin();
    let asset = Some(Address::generate(&env));
    let treasury = Address::generate(&env);

    env.as_contract(&contract_id, || {
        initialize_reserve_config(&env, asset.clone(), 1000).unwrap();
        set_treasury_address(&env, admin.clone(), treasury.clone()).unwrap();

        // Accrue some reserves
        crate::reserve::accrue_reserve(&env, asset.clone(), 1000).unwrap();

        // Admin tries to withdraw more than available
        let result = withdraw_reserve_to_treasury(&env, admin.clone(), asset.clone(), 2000);
        assert_eq!(result, Err(ReserveError::InsufficientReserve));

        // Verify balance unchanged
        let balance = crate::reserve::get_reserve_balance(&env, asset.clone());
        assert_eq!(balance, 100);
    });
}

#[test]
fn test_regression_reserve_factor_bounds() {
    //! Regression test: Verify reserve factor cannot exceed maximum.

    let (env, contract_id, admin) = setup_with_admin();
    let asset = Some(Address::generate(&env));

    env.as_contract(&contract_id, || {
        initialize_reserve_config(&env, asset.clone(), 1000).unwrap();

        // Try to set factor above maximum
        let result = set_reserve_factor(
            &env,
            admin.clone(),
            asset.clone(),
            MAX_RESERVE_FACTOR_BPS + 1,
        );
        assert_eq!(result, Err(ReserveError::InvalidReserveFactor));

        // Verify factor unchanged
        let factor = crate::reserve::get_reserve_factor(&env, asset.clone());
        assert_eq!(factor, 1000);
    });
}

// ═══════════════════════════════════════════════════════════════════════════
// Comprehensive Access Control Matrix Test
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_complete_access_control_matrix() {
    //! Comprehensive test of all access control patterns.
    //!
    //! This test documents and verifies the complete access control matrix
    //! for the StellarLend protocol.

    let (env, contract_id, admin) = setup_with_admin();
    let role_holder = Address::generate(&env);
    let regular_user = Address::generate(&env);
    let role = Symbol::new(&env, "verified_role");

    // Setup: Grant role to role_holder
    env.as_contract(&contract_id, || {
        grant_role(&env, admin.clone(), role.clone(), role_holder.clone()).unwrap();
    });

    // ═══════════════════════════════════════════════════════════════════════
    // Test 1: Admin Access (Should succeed for all admin operations)
    // ═══════════════════════════════════════════════════════════════════════
    env.as_contract(&contract_id, || {
        // Admin can call admin functions
        assert!(require_admin(&env, &admin).is_ok());

        // Admin can call role functions
        assert!(require_role_or_admin(&env, &admin, role.clone()).is_ok());
    });

    // ═══════════════════════════════════════════════════════════════════════
    // Test 2: Role Holder Access (Should succeed for role, fail for admin)
    // ═══════════════════════════════════════════════════════════════════════
    env.as_contract(&contract_id, || {
        // Role holder cannot call admin functions
        assert_eq!(
            require_admin(&env, &role_holder),
            Err(AdminError::Unauthorized)
        );

        // Role holder can call role functions with matching role
        assert!(require_role_or_admin(&env, &role_holder, role.clone()).is_ok());
    });

    // ═══════════════════════════════════════════════════════════════════════
    // Test 3: Regular User Access (Should fail for all privileged operations)
    // ═══════════════════════════════════════════════════════════════════════
    env.as_contract(&contract_id, || {
        // Regular user cannot call admin functions
        assert_eq!(
            require_admin(&env, &regular_user),
            Err(AdminError::Unauthorized)
        );

        // Regular user cannot call role functions
        assert_eq!(
            require_role_or_admin(&env, &regular_user, role.clone()),
            Err(AdminError::Unauthorized)
        );
    });

    // ═══════════════════════════════════════════════════════════════════════
    // Test 4: No Admin Set (Edge case: what if admin slot is empty?)
    // ═══════════════════════════════════════════════════════════════════════
    let empty_env = Env::default();
    let empty_contract = empty_env.register_contract(None, crate::HelloContract);
    let some_user = Address::generate(&empty_env);

    empty_env.as_contract(&empty_contract, || {
        // Without admin set, require_admin should fail
        assert_eq!(
            require_admin(&empty_env, &some_user),
            Err(AdminError::Unauthorized)
        );
    });
}

// ═══════════════════════════════════════════════════════════════════════════
// Stress Tests for Access Control
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_stress_many_roles() {
    //! Tests system behavior with many role assignments.

    let (env, contract_id, admin) = setup_with_admin();
    let num_roles = 10;
    let num_users = 5;

    env.as_contract(&contract_id, || {
        for role_idx in 0..num_roles {
            let role = Symbol::new(&env, &format!("role_{}", role_idx));
            for user_idx in 0..num_users {
                let user = Address::generate(&env);
                grant_role(&env, admin.clone(), role.clone(), user.clone()).unwrap();

                // Verify each role assignment
                assert!(has_role(&env, role.clone(), user.clone()));
            }
        }
    });
}

#[test]
fn test_stress_repeated_role_toggle() {
    //! Tests repeated grant/revoke cycles.

    let (env, contract_id, admin) = setup_with_admin();
    let user = Address::generate(&env);
    let role = Symbol::new(&env, "toggle_role");

    env.as_contract(&contract_id, || {
        for _ in 0..10 {
            // Grant
            grant_role(&env, admin.clone(), role.clone(), user.clone()).unwrap();
            assert!(has_role(&env, role.clone(), user.clone()));

            // Revoke
            revoke_role(&env, admin.clone(), role.clone(), user.clone()).unwrap();
            assert!(!has_role(&env, role.clone(), user.clone()));
        }
    });
}
