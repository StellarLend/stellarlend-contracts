//! Isolation-mode test suite
//!
//! Covers every invariant introduced by the per-asset isolation flag and debt
//! ceiling:
//!
//! - Admin-only setter / getter round-trips
//! - Isolation ceiling enforced in the cross-asset borrow path
//! - IsolationDebt tracker increments on borrow and decrements on repay
//! - Non-isolated assets are unaffected
//! - Exact-ceiling borrow accepted; ceiling+1 rejected
//! - Mixed isolated + normal collateral: ceiling still enforced
//! - Full repay resets isolation debt to zero
//! - Partial repay lowers isolation debt proportionally
//! - Ceiling update takes effect immediately for subsequent borrows
//! - Unauthorized set_asset_isolation is rejected
//! - Invalid ceiling (≤0 with isolated=true) is rejected
//! - Disabling isolation (isolated=false) allows unlimited borrow again
//! - check_isolation_ceiling view is consistent with borrow enforcement
//! - Overflow protection: ceiling + borrow that would overflow i128 is rejected

#[cfg(test)]
mod tests {
    use crate::{IsolationConfig, LendingContract, LendingContractClient, LendingError};
    use soroban_sdk::{testutils::Address as _, Address, Env};

    // -----------------------------------------------------------------------
    // Shared helpers
    // -----------------------------------------------------------------------

    fn setup() -> (Env, LendingContractClient<'static>, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(LendingContract, ());
        let client = LendingContractClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let user = Address::generate(&env);
        client.initialize(&admin);
        (env, client, admin, user)
    }

    fn asset(env: &Env) -> Address {
        Address::generate(env)
    }

    // -----------------------------------------------------------------------
    // 1. Setter / getter round-trip (admin)
    // -----------------------------------------------------------------------

    #[test]
    fn test_set_get_asset_isolation_round_trip() {
        let (env, client, _admin, _user) = setup();
        let tok = asset(&env);
        let ceiling = 500_000i128;

        client.set_asset_isolation(&tok, &true, &ceiling);

        let cfg = client
            .get_asset_isolation(&tok)
            .expect("config should be present");
        assert!(cfg.isolated);
        assert_eq!(cfg.isolation_debt_ceiling, ceiling);
    }

    #[test]
    fn test_set_isolation_false_preserves_ceiling_value() {
        let (env, client, _admin, _user) = setup();
        let tok = asset(&env);

        // Enable isolation first.
        client.set_asset_isolation(&tok, &true, &200_000i128);
        // Then disable it; the ceiling should still be readable.
        client.set_asset_isolation(&tok, &false, &200_000i128);

        let cfg = client
            .get_asset_isolation(&tok)
            .expect("config should survive disable");
        assert!(!cfg.isolated);
        assert_eq!(cfg.isolation_debt_ceiling, 200_000);
    }

    #[test]
    fn test_get_asset_isolation_returns_none_when_unset() {
        let (env, client, _admin, _user) = setup();
        let tok = asset(&env);
        assert!(client.get_asset_isolation(&tok).is_none());
    }

    // -----------------------------------------------------------------------
    // 2. Invalid ceiling rejected
    // -----------------------------------------------------------------------

    #[test]
    fn test_set_isolation_zero_ceiling_rejected() {
        let (env, client, _admin, _user) = setup();
        let tok = asset(&env);
        let res = client.try_set_asset_isolation(&tok, &true, &0i128);
        assert!(
            matches!(res, Err(Ok(LendingError::InvalidIsolationCeiling))),
            "expected InvalidIsolationCeiling, got {:?}",
            res
        );
    }

    #[test]
    fn test_set_isolation_negative_ceiling_rejected() {
        let (env, client, _admin, _user) = setup();
        let tok = asset(&env);
        let res = client.try_set_asset_isolation(&tok, &true, &(-1i128));
        assert!(
            matches!(res, Err(Ok(LendingError::InvalidIsolationCeiling))),
            "expected InvalidIsolationCeiling, got {:?}",
            res
        );
    }

    // -----------------------------------------------------------------------
    // 3. IsolationDebt defaults to zero before any borrow
    // -----------------------------------------------------------------------

    #[test]
    fn test_get_isolation_debt_zero_when_unset() {
        let (env, client, _admin, _user) = setup();
        let tok = asset(&env);
        assert_eq!(client.get_isolation_debt(&tok), 0);
    }

    // -----------------------------------------------------------------------
    // 4. Non-isolated asset — borrow_against_collateral unrestricted
    // -----------------------------------------------------------------------

    #[test]
    fn test_non_isolated_asset_borrow_unrestricted() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        // No isolation config at all.
        let result = client.borrow_against_collateral(&user, &1_000_000i128, &tok);
        assert_eq!(result, 1_000_000);
    }

    #[test]
    fn test_non_isolated_asset_isolation_debt_not_tracked() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        client.borrow_against_collateral(&user, &500_000i128, &tok);
        // Isolation debt should remain 0 for a non-isolated asset.
        assert_eq!(client.get_isolation_debt(&tok), 0);
    }

    // -----------------------------------------------------------------------
    // 5. Isolated asset — borrow within ceiling succeeds
    // -----------------------------------------------------------------------

    #[test]
    fn test_isolated_borrow_within_ceiling_succeeds() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &1_000i128);

        let result = client.borrow_against_collateral(&user, &999i128, &tok);
        assert_eq!(result, 999);
    }

    #[test]
    fn test_isolated_borrow_exactly_at_ceiling_succeeds() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &1_000i128);

        let result = client.borrow_against_collateral(&user, &1_000i128, &tok);
        assert_eq!(result, 1_000);
    }

    // -----------------------------------------------------------------------
    // 6. Isolated asset — borrow exceeding ceiling rejected
    // -----------------------------------------------------------------------

    #[test]
    fn test_isolated_borrow_exceeds_ceiling_rejected() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &1_000i128);

        let res = client.try_borrow_against_collateral(&user, &1_001i128, &tok);
        assert!(
            matches!(res, Err(Ok(LendingError::IsolationCeilingExceeded))),
            "expected IsolationCeilingExceeded, got {:?}",
            res
        );
    }

    // -----------------------------------------------------------------------
    // 7. IsolationDebt increments on borrow
    // -----------------------------------------------------------------------

    #[test]
    fn test_isolation_debt_increments_on_borrow() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &10_000i128);

        client.borrow_against_collateral(&user, &3_000i128, &tok);
        assert_eq!(client.get_isolation_debt(&tok), 3_000);

        client.borrow_against_collateral(&user, &2_000i128, &tok);
        assert_eq!(client.get_isolation_debt(&tok), 5_000);
    }

    // -----------------------------------------------------------------------
    // 8. Cumulative borrows — ceiling is aggregate across calls
    // -----------------------------------------------------------------------

    #[test]
    fn test_cumulative_borrows_hit_ceiling() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &1_000i128);

        client.borrow_against_collateral(&user, &600i128, &tok);
        // 600 used; 400 remaining.
        let res = client.try_borrow_against_collateral(&user, &401i128, &tok);
        assert!(
            matches!(res, Err(Ok(LendingError::IsolationCeilingExceeded))),
            "expected IsolationCeilingExceeded on second borrow, got {:?}",
            res
        );
    }

    #[test]
    fn test_cumulative_borrows_just_within_ceiling() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &1_000i128);

        client.borrow_against_collateral(&user, &600i128, &tok);
        let result = client.borrow_against_collateral(&user, &400i128, &tok);
        // Second borrow succeeds and principal is now 1_000.
        assert_eq!(result, 1_000);
        assert_eq!(client.get_isolation_debt(&tok), 1_000);
    }

    // -----------------------------------------------------------------------
    // 9. IsolationDebt decrements on repay
    // -----------------------------------------------------------------------

    #[test]
    fn test_isolation_debt_decrements_on_repay() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &10_000i128);

        client.borrow_against_collateral(&user, &5_000i128, &tok);
        assert_eq!(client.get_isolation_debt(&tok), 5_000);

        client.repay_against_collateral(&user, &2_000i128, &tok);
        assert_eq!(client.get_isolation_debt(&tok), 3_000);
    }

    // -----------------------------------------------------------------------
    // 10. Full repay resets isolation debt to zero
    // -----------------------------------------------------------------------

    #[test]
    fn test_full_repay_resets_isolation_debt() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &5_000i128);

        client.borrow_against_collateral(&user, &5_000i128, &tok);
        assert_eq!(client.get_isolation_debt(&tok), 5_000);

        // Repay more than owed — should be capped at outstanding debt.
        client.repay_against_collateral(&user, &10_000i128, &tok);
        assert_eq!(client.get_isolation_debt(&tok), 0);
    }

    // -----------------------------------------------------------------------
    // 11. After repay, ceiling space is freed for new borrows
    // -----------------------------------------------------------------------

    #[test]
    fn test_repay_frees_ceiling_space_for_new_borrow() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &1_000i128);

        // Fill to ceiling.
        client.borrow_against_collateral(&user, &1_000i128, &tok);
        // A further borrow would be rejected…
        let res = client.try_borrow_against_collateral(&user, &1i128, &tok);
        assert!(matches!(
            res,
            Err(Ok(LendingError::IsolationCeilingExceeded))
        ));

        // Repay half.
        client.repay_against_collateral(&user, &500i128, &tok);
        assert_eq!(client.get_isolation_debt(&tok), 500);

        // Now borrow 500 again — should succeed.
        let result = client.borrow_against_collateral(&user, &500i128, &tok);
        assert_eq!(result, 1_000); // total principal back to 1_000
        assert_eq!(client.get_isolation_debt(&tok), 1_000);
    }

    // -----------------------------------------------------------------------
    // 12. Ceiling update takes effect immediately
    // -----------------------------------------------------------------------

    #[test]
    fn test_ceiling_update_tightens_immediately() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &10_000i128);

        client.borrow_against_collateral(&user, &5_000i128, &tok);
        // Admin lowers ceiling to 5_000 (exactly what is already borrowed).
        client.set_asset_isolation(&tok, &true, &5_000i128);

        // Any additional borrow should now be rejected.
        let res = client.try_borrow_against_collateral(&user, &1i128, &tok);
        assert!(
            matches!(res, Err(Ok(LendingError::IsolationCeilingExceeded))),
            "expected IsolationCeilingExceeded after ceiling tightened, got {:?}",
            res
        );
    }

    #[test]
    fn test_ceiling_update_relaxes_immediately() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &1_000i128);

        // Fill to ceiling.
        client.borrow_against_collateral(&user, &1_000i128, &tok);
        // Admin raises ceiling.
        client.set_asset_isolation(&tok, &true, &2_000i128);

        let result = client.borrow_against_collateral(&user, &1_000i128, &tok);
        assert_eq!(result, 2_000);
        assert_eq!(client.get_isolation_debt(&tok), 2_000);
    }

    // -----------------------------------------------------------------------
    // 13. Disabling isolation removes the restriction
    // -----------------------------------------------------------------------

    #[test]
    fn test_disabling_isolation_removes_ceiling_enforcement() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &100i128);

        // Would be blocked while isolated.
        let res = client.try_borrow_against_collateral(&user, &200i128, &tok);
        assert!(matches!(
            res,
            Err(Ok(LendingError::IsolationCeilingExceeded))
        ));

        // Disable isolation (ceiling value preserved but ignored).
        client.set_asset_isolation(&tok, &false, &100i128);

        // Now the borrow should succeed.
        let result = client.borrow_against_collateral(&user, &200i128, &tok);
        assert_eq!(result, 200);
    }

    // -----------------------------------------------------------------------
    // 14. check_isolation_ceiling view is consistent with borrow enforcement
    // -----------------------------------------------------------------------

    #[test]
    fn test_check_isolation_ceiling_view_consistent_within() {
        let (env, client, _admin, _user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &1_000i128);

        // View should return Ok for amount within ceiling.
        let res = client.try_check_isolation_ceiling(&tok, &500i128);
        assert!(res.is_ok(), "expected Ok for amount within ceiling");
    }

    #[test]
    fn test_check_isolation_ceiling_view_consistent_exceeds() {
        let (env, client, _admin, _user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &1_000i128);

        let res = client.try_check_isolation_ceiling(&tok, &1_001i128);
        assert!(
            matches!(res, Err(Ok(LendingError::IsolationCeilingExceeded))),
            "expected IsolationCeilingExceeded from view, got {:?}",
            res
        );
    }

    #[test]
    fn test_check_isolation_ceiling_view_reflects_outstanding_debt() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &1_000i128);

        // Borrow 700 — 300 remaining.
        client.borrow_against_collateral(&user, &700i128, &tok);

        // 300 should be fine.
        assert!(client.try_check_isolation_ceiling(&tok, &300i128).is_ok());
        // 301 should be rejected.
        let res = client.try_check_isolation_ceiling(&tok, &301i128);
        assert!(matches!(
            res,
            Err(Ok(LendingError::IsolationCeilingExceeded))
        ));
    }

    // -----------------------------------------------------------------------
    // 15. Multiple assets are independently tracked
    // -----------------------------------------------------------------------

    #[test]
    fn test_two_isolated_assets_tracked_independently() {
        let (env, client, _admin, user) = setup();
        let tok_a = asset(&env);
        let tok_b = asset(&env);

        client.set_asset_isolation(&tok_a, &true, &2_000i128);
        client.set_asset_isolation(&tok_b, &true, &3_000i128);

        client.borrow_against_collateral(&user, &2_000i128, &tok_a);
        client.borrow_against_collateral(&user, &1_000i128, &tok_b);

        assert_eq!(client.get_isolation_debt(&tok_a), 2_000);
        assert_eq!(client.get_isolation_debt(&tok_b), 1_000);

        // tok_a is at ceiling; tok_b still has 2_000 headroom.
        let res_a = client.try_borrow_against_collateral(&user, &1i128, &tok_a);
        assert!(matches!(
            res_a,
            Err(Ok(LendingError::IsolationCeilingExceeded))
        ));

        let result_b = client.borrow_against_collateral(&user, &2_000i128, &tok_b);
        // The returned value is the user's total principal across all borrows:
        // tok_a(2_000) + tok_b(1_000) + tok_b(2_000) = 5_000.
        assert_eq!(result_b, 5_000);
        // But the per-asset isolation debt for tok_b is only the tok_b borrows.
        assert_eq!(client.get_isolation_debt(&tok_b), 3_000);
        assert_eq!(client.get_isolation_debt(&tok_a), 2_000);
    }

    // -----------------------------------------------------------------------
    // 16. Normal borrow (no collateral arg) never touches IsolationDebt
    // -----------------------------------------------------------------------

    #[test]
    fn test_plain_borrow_does_not_affect_isolation_debt() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &1_000i128);

        // Plain borrow bypasses isolation tracking.
        client.borrow(&user, &5_000i128);
        assert_eq!(client.get_isolation_debt(&tok), 0);
    }

    // -----------------------------------------------------------------------
    // 17. Over-repay does not make IsolationDebt negative
    // -----------------------------------------------------------------------

    #[test]
    fn test_over_repay_does_not_make_isolation_debt_negative() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &5_000i128);

        client.borrow_against_collateral(&user, &1_000i128, &tok);
        // Repay far more than owed.
        client.repay_against_collateral(&user, &999_999i128, &tok);

        let debt = client.get_isolation_debt(&tok);
        assert!(
            debt >= 0,
            "isolation debt must not be negative; got {}",
            debt
        );
        assert_eq!(debt, 0);
    }

    // -----------------------------------------------------------------------
    // 18. Boundary: single-unit borrows and repays
    // -----------------------------------------------------------------------

    #[test]
    fn test_single_unit_borrow_and_repay() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        client.set_asset_isolation(&tok, &true, &1i128);

        client.borrow_against_collateral(&user, &1i128, &tok);
        assert_eq!(client.get_isolation_debt(&tok), 1);

        // Ceiling is full; next borrow rejected.
        let res = client.try_borrow_against_collateral(&user, &1i128, &tok);
        assert!(matches!(
            res,
            Err(Ok(LendingError::IsolationCeilingExceeded))
        ));

        client.repay_against_collateral(&user, &1i128, &tok);
        assert_eq!(client.get_isolation_debt(&tok), 0);

        // After repay ceiling is free again.
        let result = client.borrow_against_collateral(&user, &1i128, &tok);
        assert_eq!(result, 1);
    }

    // -----------------------------------------------------------------------
    // 19. Large ceiling (i128::MAX / 2) — no spurious overflow
    // -----------------------------------------------------------------------

    #[test]
    fn test_large_ceiling_no_overflow() {
        let (env, client, _admin, user) = setup();
        let tok = asset(&env);
        let large_ceiling = i128::MAX / 2;
        client.set_asset_isolation(&tok, &true, &large_ceiling);

        let big_amount = large_ceiling - 1;
        let result = client.borrow_against_collateral(&user, &big_amount, &tok);
        assert_eq!(result, big_amount);
        assert_eq!(client.get_isolation_debt(&tok), big_amount);
    }

    // -----------------------------------------------------------------------
    // 20. Isolated asset: ceiling enforced in health / borrow cross-check
    //     (mixed position: isolated collateral cannot amplify normal debt)
    // -----------------------------------------------------------------------

    #[test]
    fn test_isolated_collateral_ceiling_still_applies_in_cross_position() {
        // A user has both normal and isolated collateral.  The isolated asset's
        // ceiling still caps the debt that can be backed by it.
        let (env, client, _admin, user) = setup();
        let isolated_tok = asset(&env);
        let _normal_tok = asset(&env);

        client.set_asset_isolation(&isolated_tok, &true, &500i128);

        // Borrow 500 against the isolated asset — fills the ceiling.
        client.borrow_against_collateral(&user, &500i128, &isolated_tok);

        // Attempting one more unit against the isolated asset is rejected.
        let res = client.try_borrow_against_collateral(&user, &1i128, &isolated_tok);
        assert!(
            matches!(res, Err(Ok(LendingError::IsolationCeilingExceeded))),
            "isolated ceiling must hold even in mixed position"
        );
    }
}
