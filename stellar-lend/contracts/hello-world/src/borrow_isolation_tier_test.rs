/// Tests for the cross-asset per-user borrow-isolation tier.
///
/// Coverage:
/// - Cap unset (unlimited): users may accumulate any number of debt assets
/// - Borrow at the cap boundary (exactly at cap is rejected, one below is allowed)
/// - Borrowing more of an *existing* debt asset while at the cap (always allowed)
/// - Unauthorized setter is rejected
/// - Invalid cap value (0) is rejected
/// - Native XLM (None) tracked as distinct asset slot
/// - Empty debt list for new users
#[cfg(test)]
mod borrow_isolation_tier_tests {
    use soroban_sdk::{testutils::Address as _, Address, Env};

    use crate::{HelloContract, HelloContractClient};

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    /// Create an env with a deployed HelloContract and an initialised admin.
    /// Returns (env, contract_id, admin).
    fn setup() -> (Env, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(HelloContract, ());
        let admin = Address::generate(&env);
        // Initialise risk management so the admin key is stored
        let client = HelloContractClient::new(&env, &contract_id);
        client.initialize(&admin);
        (env, contract_id, admin)
    }

    fn make_asset(env: &Env) -> Option<Address> {
        Some(Address::generate(env))
    }

    // -----------------------------------------------------------------------
    // Cap getter / setter via contract API
    // -----------------------------------------------------------------------

    #[test]
    fn cap_unset_by_default() {
        let (env, contract_id, _admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);
        assert_eq!(client.get_max_debt_assets_per_user(), None);
    }

    #[test]
    fn admin_can_set_and_read_cap() {
        let (env, contract_id, admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);
        client.set_max_debt_assets_per_user(&admin, &Some(3));
        assert_eq!(client.get_max_debt_assets_per_user(), Some(3));
    }

    #[test]
    fn admin_can_remove_cap() {
        let (env, contract_id, admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);
        client.set_max_debt_assets_per_user(&admin, &Some(5));
        client.set_max_debt_assets_per_user(&admin, &None);
        assert_eq!(client.get_max_debt_assets_per_user(), None);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #2)")]
    fn invalid_cap_zero_rejected() {
        let (env, contract_id, admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);
        client.set_max_debt_assets_per_user(&admin, &Some(0));
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #1)")]
    fn unauthorized_setter_rejected() {
        let (env, contract_id, _admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);
        let attacker = Address::generate(&env);
        client.set_max_debt_assets_per_user(&attacker, &Some(2));
    }

    // -----------------------------------------------------------------------
    // Debt-list management via contract API
    // -----------------------------------------------------------------------

    #[test]
    fn cap_unset_allows_many_assets() {
        let (env, contract_id, _admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);
        let user = Address::generate(&env);

        // No cap — adding 10 distinct assets must succeed
        for _ in 0..10 {
            client.add_to_user_debt_list(&user, &make_asset(&env));
        }
        assert_eq!(client.get_user_debt_assets(&user).len(), 10);
    }

    #[test]
    fn borrow_up_to_cap_allowed() {
        let (env, contract_id, admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);
        client.set_max_debt_assets_per_user(&admin, &Some(3));
        let user = Address::generate(&env);

        client.add_to_user_debt_list(&user, &make_asset(&env));
        client.add_to_user_debt_list(&user, &make_asset(&env));
        let count = client.add_to_user_debt_list(&user, &make_asset(&env));
        assert_eq!(count, 3);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #3)")]
    fn borrow_new_asset_at_cap_rejected() {
        let (env, contract_id, admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);
        client.set_max_debt_assets_per_user(&admin, &Some(2));
        let user = Address::generate(&env);

        client.add_to_user_debt_list(&user, &make_asset(&env));
        client.add_to_user_debt_list(&user, &make_asset(&env));
        // Third (new) asset must be rejected
        client.add_to_user_debt_list(&user, &make_asset(&env));
    }

    #[test]
    fn existing_asset_at_cap_is_allowed() {
        let (env, contract_id, admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);
        client.set_max_debt_assets_per_user(&admin, &Some(2));
        let user = Address::generate(&env);

        let asset_a = make_asset(&env);
        let asset_b = make_asset(&env);

        client.add_to_user_debt_list(&user, &asset_a);
        client.add_to_user_debt_list(&user, &asset_b);

        // Re-borrow asset_a (already tracked) — must succeed even at cap
        let count = client.add_to_user_debt_list(&user, &asset_a);
        assert_eq!(count, 2); // List length unchanged
    }

    #[test]
    fn native_xlm_asset_tracked_as_none() {
        let (env, contract_id, admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);
        client.set_max_debt_assets_per_user(&admin, &Some(1));
        let user = Address::generate(&env);

        // Borrow native XLM (None)
        client.add_to_user_debt_list(&user, &None);
        // Second borrow of native XLM must not add a duplicate
        let count = client.add_to_user_debt_list(&user, &None);
        assert_eq!(count, 1);
    }

    #[test]
    #[should_panic(expected = "Error(Contract, #3)")]
    fn new_token_rejected_when_native_xlm_fills_cap() {
        let (env, contract_id, admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);
        client.set_max_debt_assets_per_user(&admin, &Some(1));
        let user = Address::generate(&env);

        client.add_to_user_debt_list(&user, &None);
        // A new token asset should be rejected (cap = 1, already at limit)
        client.add_to_user_debt_list(&user, &make_asset(&env));
    }

    #[test]
    fn get_user_debt_assets_returns_empty_for_new_user() {
        let (env, contract_id, _admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);
        let user = Address::generate(&env);
        let assets = client.get_user_debt_assets(&user);
        assert_eq!(assets.len(), 0);
    }

    #[test]
    fn cap_of_one_allows_single_asset() {
        let (env, contract_id, admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);
        client.set_max_debt_assets_per_user(&admin, &Some(1));
        let user = Address::generate(&env);

        let count = client.add_to_user_debt_list(&user, &make_asset(&env));
        assert_eq!(count, 1);
    }

    #[test]
    fn large_cap_allows_many_borrows() {
        let (env, contract_id, admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);
        client.set_max_debt_assets_per_user(&admin, &Some(10));
        let user = Address::generate(&env);

        for _ in 0..10 {
            client.add_to_user_debt_list(&user, &make_asset(&env));
        }
        assert_eq!(client.get_user_debt_assets(&user).len(), 10);
    }
}
