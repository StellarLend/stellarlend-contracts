#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        symbol_short, testutils::Address as _, vec, Address, Env,
    };

    #[test]
    fn test_config_round_trip() {
        let env = Env::default();
        let asset = AssetKey::Token(Address::generate(&env));

        // Test initial state: absent key
        assert!(
            env.storage()
                .persistent()
                .get::<CrossAssetDataKey, AssetConfig>(&CrossAssetDataKey::Config(asset.clone()))
                .is_none()
        );

        // Write config
        let config = AssetConfig {
            collateral_factor_bps: 7500,
            liquidation_threshold: 8000,
            max_supply: 1_000_000,
            max_borrow: 500_000,
            can_collateralize: true,
            can_borrow: true,
            price: 1_000_000,
            price_decimals: 6,
        };
        env.storage()
            .persistent()
            .set(&CrossAssetDataKey::Config(asset.clone()), &config);

        // Read back and verify
        let read_config = env
            .storage()
            .persistent()
            .get::<CrossAssetDataKey, AssetConfig>(&CrossAssetDataKey::Config(asset.clone()))
            .unwrap();
        assert_eq!(read_config.collateral_factor_bps, config.collateral_factor_bps);
        assert_eq!(read_config.liquidation_threshold, config.liquidation_threshold);
        assert_eq!(read_config.max_supply, config.max_supply);
        assert_eq!(read_config.max_borrow, config.max_borrow);
        assert_eq!(read_config.can_collateralize, config.can_collateralize);
        assert_eq!(read_config.can_borrow, config.can_borrow);
        assert_eq!(read_config.price, config.price);
        assert_eq!(read_config.price_decimals, config.price_decimals);
    }

    #[test]
    fn test_asset_list_round_trip() {
        let env = Env::default();
        let asset1 = AssetKey::Token(Address::generate(&env));
        let asset2 = AssetKey::Token(Address::generate(&env));

        // Test initial state: absent key returns empty
        let initial_list = env
            .storage()
            .persistent()
            .get::<CrossAssetDataKey, Vec<AssetKey>>(&CrossAssetDataKey::AssetList)
            .unwrap_or_else(|| vec![&env]);
        assert_eq!(initial_list.len(), 0);

        // Write list
        let list = vec![&env, asset1.clone(), asset2.clone()];
        env.storage()
            .persistent()
            .set(&CrossAssetDataKey::AssetList, &list);

        // Read back and verify
        let read_list = env
            .storage()
            .persistent()
            .get::<CrossAssetDataKey, Vec<AssetKey>>(&CrossAssetDataKey::AssetList)
            .unwrap();
        assert_eq!(read_list.len(), 2);
        assert_eq!(read_list.get(0).unwrap(), asset1);
        assert_eq!(read_list.get(1).unwrap(), asset2);
    }

    #[test]
    fn test_user_supply_round_trip_and_default() {
        let env = Env::default();
        let user = Address::generate(&env);
        let asset = AssetKey::Token(Address::generate(&env));

        // Test default: 0 when absent
        let initial_supply = env
            .storage()
            .persistent()
            .get::<CrossAssetDataKey, i128>(&CrossAssetDataKey::UserSupply(
                asset.clone(),
                user.clone(),
            ))
            .unwrap_or(0);
        assert_eq!(initial_supply, 0);

        // Write value
        let amount = 1000;
        env.storage().persistent().set(
            &CrossAssetDataKey::UserSupply(asset.clone(), user.clone()),
            &amount,
        );

        // Read back and verify
        let read_supply = env
            .storage()
            .persistent()
            .get::<CrossAssetDataKey, i128>(&CrossAssetDataKey::UserSupply(
                asset.clone(),
                user.clone(),
            ))
            .unwrap();
        assert_eq!(read_supply, amount);
    }

    #[test]
    fn test_user_debt_round_trip_and_default() {
        let env = Env::default();
        let user = Address::generate(&env);
        let asset = AssetKey::Token(Address::generate(&env));

        // Test default: 0 when absent
        let initial_debt = env
            .storage()
            .persistent()
            .get::<CrossAssetDataKey, i128>(&CrossAssetDataKey::UserDebt(
                asset.clone(),
                user.clone(),
            ))
            .unwrap_or(0);
        assert_eq!(initial_debt, 0);

        // Write value
        let amount = 500;
        env.storage().persistent().set(
            &CrossAssetDataKey::UserDebt(asset.clone(), user.clone()),
            &amount,
        );

        // Read back and verify
        let read_debt = env
            .storage()
            .persistent()
            .get::<CrossAssetDataKey, i128>(&CrossAssetDataKey::UserDebt(
                asset.clone(),
                user.clone(),
            ))
            .unwrap();
        assert_eq!(read_debt, amount);
    }

    #[test]
    fn test_total_supply_round_trip_and_default() {
        let env = Env::default();
        let asset = AssetKey::Token(Address::generate(&env));

        // Test default: 0 when absent
        let initial_total = env
            .storage()
            .persistent()
            .get::<CrossAssetDataKey, i128>(&CrossAssetDataKey::TotalSupply(asset.clone()))
            .unwrap_or(0);
        assert_eq!(initial_total, 0);

        // Write value
        let amount = 10_000;
        env.storage()
            .persistent()
            .set(&CrossAssetDataKey::TotalSupply(asset.clone()), &amount);

        // Read back and verify
        let read_total = env
            .storage()
            .persistent()
            .get::<CrossAssetDataKey, i128>(&CrossAssetDataKey::TotalSupply(asset.clone()))
            .unwrap();
        assert_eq!(read_total, amount);
    }

    #[test]
    fn test_total_debt_round_trip_and_default() {
        let env = Env::default();
        let asset = AssetKey::Token(Address::generate(&env));

        // Test default: 0 when absent
        let initial_total = env
            .storage()
            .persistent()
            .get::<CrossAssetDataKey, i128>(&CrossAssetDataKey::TotalDebt(asset.clone()))
            .unwrap_or(0);
        assert_eq!(initial_total, 0);

        // Write value
        let amount = 5_000;
        env.storage()
            .persistent()
            .set(&CrossAssetDataKey::TotalDebt(asset.clone()), &amount);

        // Read back and verify
        let read_total = env
            .storage()
            .persistent()
            .get::<CrossAssetDataKey, i128>(&CrossAssetDataKey::TotalDebt(asset.clone()))
            .unwrap();
        assert_eq!(read_total, amount);
    }
}
