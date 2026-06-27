//! Doctest verifying the worked example from `cross_asset.md`

#[cfg(test)]
mod doctest_worked_example {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use crate::cross_asset_test::setup;

    #[test]
    fn test_worked_example_health_factor() {
        // Setup environment and client using existing test fixtures
        let (_env, client, _id, admin, user, asset_a, asset_b) = setup();
        // Configure assets to match worked example parameters
        client.set_asset_params(&admin, &asset_a, 7500, 9000, 1_000_000_000_000i128);
        client.set_asset_params(&admin, &asset_b, 8000, 8000, 1_000_000_000_000i128);
        // Deposit collateral: 100 units of asset_a (USDC), 1 unit of asset_b (ETH)
        client.deposit_collateral_asset(&user, &asset_a, 100i128);
        client.deposit_collateral_asset(&user, &asset_b, 1i128);
        // Borrow 1,000 units of asset_a (USDC)
        client.borrow_asset(&user, &asset_a, 1_000i128);
        // Compute health factor and verify it matches the documented worked example
        let hf = client.get_cross_health_factor(&user);
        assert_eq!(hf, 16_900); // 1.69 * HEALTH_FACTOR_SCALE (10_000)
    }
}
