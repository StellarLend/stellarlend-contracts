// Tests for oracle price bounds
#![cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{Env, BytesN, Address, testutils::Address as TestAddress};

    fn setup_env() -> (Env, Address, BytesN<32>, BytesN<64>) {
        let env = Env::default();
        let admin = Address::generate(&env);
        let pubkey = BytesN::from_array(&env, &[0; 32]);
        let signature = BytesN::from_array(&env, &[0; 64]); // dummy signature
        (env, admin, pubkey, signature)
    }

    #[test]
    fn test_price_within_bounds() {
        let (env, admin, pubkey, signature) = setup_env();
        LendingContract::initialize(&env, admin.clone());
        LendingContract::set_oracle_pubkey(&env, pubkey.clone());
        let asset = Address::generate(&env);
        LendingContract::set_price_bounds(&env, asset.clone(), 1, 1_000_000).unwrap();
        let price = 500_000i128;
        let ts = env.ledger().timestamp();
        // Assuming signature verification passes in this test environment
        LendingContract::set_price(&env, admin.clone(), asset.clone(), price, ts, signature.clone()).unwrap();
        let record = LendingContract::get_price_record(&env, asset).unwrap();
        assert_eq!(record.price, price);
    }

    #[test]
    fn test_price_below_min_rejects() {
        let (env, admin, pubkey, signature) = setup_env();
        LendingContract::initialize(&env, admin.clone());
        LendingContract::set_oracle_pubkey(&env, pubkey.clone());
        let asset = Address::generate(&env);
        LendingContract::set_price_bounds(&env, asset.clone(), 100, 1_000).unwrap();
        let price = 50i128;
        let ts = env.ledger().timestamp();
        let res = LendingContract::set_price(&env, admin.clone(), asset.clone(), price, ts, signature.clone());
        assert_eq!(res, Err(LendingError::PriceOutOfBounds));
    }

    #[test]
    fn test_price_above_max_rejects() {
        let (env, admin, pubkey, signature) = setup_env();
        LendingContract::initialize(&env, admin.clone());
        LendingContract::set_oracle_pubkey(&env, pubkey.clone());
        let asset = Address::generate(&env);
        LendingContract::set_price_bounds(&env, asset.clone(), 1, 1_000).unwrap();
        let price = 2_000i128;
        let ts = env.ledger().timestamp();
        let res = LendingContract::set_price(&env, admin.clone(), asset.clone(), price, ts, signature.clone());
        assert_eq!(res, Err(LendingError::PriceOutOfBounds));
    }

    #[test]
    fn test_price_zero_rejects() {
        let (env, admin, pubkey, signature) = setup_env();
        LendingContract::initialize(&env, admin.clone());
        LendingContract::set_oracle_pubkey(&env, pubkey.clone());
        let asset = Address::generate(&env);
        let price = 0i128;
        let ts = env.ledger().timestamp();
        let res = LendingContract::set_price(&env, admin.clone(), asset.clone(), price, ts, signature.clone());
        assert_eq!(res, Err(LendingError::InvalidAmount));
    }
}
