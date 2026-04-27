//! Feature-gated AMM liquidation routing smoke tests.
//!
//! These tests intentionally validate only AMM-side behavior that does not
//! depend on the lending crate wiring.

use super::*;
use crate::amm::{AmmProtocolConfig, SwapParams, TokenPair};
use soroban_sdk::{testutils::Address as _, testutils::Ledger, Address, Env, Symbol, Vec};

fn create_amm_contract<'a>(env: &Env) -> AmmContractClient<'a> {
    AmmContractClient::new(env, &env.register(AmmContract {}, ()))
}

fn setup_protocol(
    env: &Env,
    contract: &AmmContractClient<'_>,
    admin: &Address,
) -> (Address, Address) {
    let protocol_addr = env.register(MockAmm, ());
    let token_out = Address::generate(env);

    contract.initialize_amm_settings(admin, &100, &1_000, &10_000);

    let mut supported_pairs = Vec::new(env);
    supported_pairs.push_back(TokenPair {
        token_a: None,
        token_b: Some(token_out.clone()),
        pool_address: Address::generate(env),
    });

    let protocol = AmmProtocolConfig {
        protocol_address: protocol_addr.clone(),
        protocol_name: Symbol::new(env, "LiqAMM"),
        enabled: true,
        fee_tier: 30,
        min_swap_amount: 1_000,
        max_swap_amount: 1_000_000_000,
        supported_pairs,
    };

    contract.add_amm_protocol(admin, &protocol);
    (protocol_addr, token_out)
}

#[test]
fn test_liquidation_swap_path_success() {
    let env = Env::default();
    env.mock_all_auths();

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let (protocol_addr, token_out) = setup_protocol(&env, &contract, &admin);

    let params = SwapParams {
        protocol: protocol_addr,
        token_in: None,
        token_out: Some(token_out),
        amount_in: 20_000,
        min_amount_out: 19_000,
        slippage_tolerance: 100,
        deadline: env.ledger().timestamp() + 3_600,
    };

    let out = contract.execute_swap(&user, &params);
    assert_eq!(out, 19_800);
}

#[test]
fn test_liquidation_swap_path_rejects_expired_deadline() {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_000);

    let contract = create_amm_contract(&env);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    let (protocol_addr, token_out) = setup_protocol(&env, &contract, &admin);

    let params = SwapParams {
        protocol: protocol_addr,
        token_in: None,
        token_out: Some(token_out),
        amount_in: 20_000,
        min_amount_out: 1,
        slippage_tolerance: 100,
        deadline: 999,
    };

    assert!(contract.try_execute_swap(&user, &params).is_err());
}
