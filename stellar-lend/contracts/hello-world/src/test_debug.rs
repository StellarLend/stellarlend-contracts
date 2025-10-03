#[cfg(test)]
mod test_debug {
    use super::*;
    use soroban_sdk::{Address, Env};
    use crate::test::TestUtils;

    #[test]
    fn test_multisig_config_debug() {
        let env = TestUtils::create_test_env();
        let admin = Address::generate(&env);
        let signer1 = Address::generate(&env);
        let signer2 = Address::generate(&env);

        let contract_id = env.register(Contract, ());
        env.as_contract(&contract_id, || {
            // Initialize contract
            Contract::initialize(env.clone(), admin.to_string()).unwrap();
            
            // Initialize MultiSig
            let signers = vec![&env, signer1.clone(), signer2.clone()];
            let result = Contract::initialize_multisig(
                env.clone(), 
                admin.to_string(), 
                signers, 
                2, 
                7200
            );
            assert!(result.is_ok());
            
            // Check if configuration is saved
            let config = governance::GovStorage::get_multisig_config(&env);
            println!("Config: {:?}", config);
            assert!(config.is_some());
        });
    }
}
