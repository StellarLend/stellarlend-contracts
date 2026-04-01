use hello_world::{HelloContract, HelloContractClient};
use soroban_sdk::token::StellarAssetClient;
use soroban_sdk::{testutils::Address as _, Address, Env, Symbol};

// Invariant:
// - repay should not decrease health factor
// - borrow should not increase health factor

fn setup_env_with_native_asset() -> (
    Env,
    Address,
    HelloContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let user = Address::generate(&env);
    client.initialize(&admin);
    let native_asset = env.register_stellar_asset_contract(admin.clone());
    client.set_native_asset_address(&admin, &native_asset);
    (env, contract_id, client, admin, user, native_asset)
}

#[test]
fn test_repay_monotonicity() {
    let debts = [1i128, 10, 100];
    let repays = [0i128, 1, 5, 10];

    for &debt in debts.iter() {
        for &repay in repays.iter() {
            // fresh isolated env for each scenario
            let (env, contract_id, client, _admin, user, native_asset) =
                setup_env_with_native_asset();
            let token_client = StellarAssetClient::new(&env, &native_asset);

            // Use a fixed, sufficient collateral and cap requested borrow to a safe value
            let collateral = 100_000i128;
            let safe_debt = if debt > 10 { 10 } else { debt };
            // mint collateral to the user and approve the contract to transfer it
            token_client.mint(&user, &collateral);
            token_client.approve(
                &user,
                &contract_id,
                &collateral,
                &(env.ledger().sequence() + 100),
            );
            client.deposit_collateral(&user, &Some(native_asset.clone()), &collateral);

            // perform borrow to reach the desired (capped) debt
            client.borrow_asset(&user, &Some(native_asset.clone()), &safe_debt);

            // read health before repay via get_user_report
            let report_before = client.get_user_report(&user);
            let health_before = report_before.metrics.health_factor;

            // prepare repay funds if needed
            if repay > 0 && safe_debt > 0 {
                token_client.mint(&user, &repay);
                token_client.approve(
                    &user,
                    &contract_id,
                    &repay,
                    &(env.ledger().sequence() + 100),
                );
                // call repay (may be zero)
                client.repay_debt(&user, &Some(native_asset.clone()), &repay);
            }

            let report_after = client.get_user_report(&user);
            let health_after = report_after.metrics.health_factor;

            assert!(
                health_after >= health_before,
                "repay decreased health: debt={:?} repay={:?} before={} after={}",
                debt,
                repay,
                health_before,
                health_after
            );
        }
    }
}

#[test]
fn test_borrow_non_improving() {
    let collateral_values = [10000i128, 50000, 100000];
    let borrows = [1i128, 10, 50];

    for &collateral in collateral_values.iter() {
        for &borrow in borrows.iter() {
            let (env, contract_id, client, _admin, user, native_asset) =
                setup_env_with_native_asset();

            // deposit collateral (mint + approve first so transfer_from succeeds)
            let token_client = StellarAssetClient::new(&env, &native_asset);
            token_client.mint(&user, &collateral);
            token_client.approve(
                &user,
                &contract_id,
                &collateral,
                &(env.ledger().sequence() + 100),
            );
            client.deposit_collateral(&user, &Some(native_asset.clone()), &collateral);

            let report_before = client.get_user_report(&user);
            let health_before = report_before.metrics.health_factor;

            // perform borrow (may be zero)
            client.borrow_asset(&user, &Some(native_asset.clone()), &borrow);

            let report_after = client.get_user_report(&user);
            let health_after = report_after.metrics.health_factor;

            assert!(
                health_after <= health_before,
                "borrow improved health: collateral={:?} borrow={:?} before={} after={}",
                collateral,
                borrow,
                health_before,
                health_after
            );
        }
    }
}
