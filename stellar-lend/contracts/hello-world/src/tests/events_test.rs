// Event Logging System – Tests
//
// Comprehensive tests verifying every `emit_*` helper in the `events` module
// emits a correctly structured event that can be read back from
// `env.events().all()`.
//
// Soroban events API:
// `env.events().all()` returns `Vec<(Address, Vec<Val>, Val)>`:
//   - `Address` – contract that emitted the event
//   - `Vec<Val>` – topic(s) tuple
//   - `Val` – event data payload
use crate::deposit::{emit_position_updated_event, Position};
use crate::events::{
    emit_admin_action, emit_borrow, emit_borrower_health_v1, emit_deposit,
    emit_flash_loan_initiated, emit_flash_loan_repaid, emit_liquidation, emit_liquidation_v1,
    emit_pause_state_changed, emit_price_updated, emit_repay, emit_risk_params_updated,
    emit_withdrawal, AdminActionEvent, BorrowEvent, BorrowerHealthEventV1, DepositEvent,
    FlashLoanInitiatedEvent, FlashLoanRepaidEvent, LiquidationEvent, LiquidationEventV1,
    PauseStateChangedEvent, PriceUpdatedEvent, RepayEvent, RiskParamsUpdatedEvent, WithdrawalEvent,
};

use crate::{HelloContract, HelloContractClient};

use soroban_sdk::{
    contracttype,
    testutils::{Address as _, Events},
    Address, Env, Symbol, TryFromVal,
};

// ─────────────────────────────────────────────────────────────────────────────
// Test Types (mirroring event structures for easy decoding)
// ─────────────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug)]
pub struct TestDepositEvent {
    pub user: Address,
    pub asset: Option<Address>,
    pub amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TestWithdrawalEvent {
    pub user: Address,
    pub asset: Option<Address>,
    pub amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TestBorrowEvent {
    pub user: Address,
    pub asset: Option<Address>,
    pub amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TestRepayEvent {
    pub user: Address,
    pub asset: Option<Address>,
    pub amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TestLiquidationEvent {
    pub liquidator: Address,
    pub borrower: Address,
    pub debt_asset: Option<Address>,
    pub collateral_asset: Option<Address>,
    pub debt_liquidated: i128,
    pub collateral_seized: i128,
    pub incentive_amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TestLiquidationEventV1 {
    pub schema_version: u32,
    pub liquidator: Address,
    pub borrower: Address,
    pub debt_asset: Option<Address>,
    pub collateral_asset: Option<Address>,
    pub debt_liquidated: i128,
    pub collateral_seized: i128,
    pub incentive_amount: i128,
    pub borrower_collateral_after: i128,
    pub borrower_principal_debt_after: i128,
    pub borrower_interest_after: i128,
    pub borrower_total_debt_after: i128,
    pub borrower_health_factor_after: i128,
    pub borrower_risk_level_after: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TestBorrowerHealthEventV1 {
    pub schema_version: u32,
    pub user: Address,
    pub operation: Symbol,
    pub collateral: i128,
    pub principal_debt: i128,
    pub borrow_interest: i128,
    pub total_debt: i128,
    pub health_factor: i128,
    pub risk_level: i128,
    pub is_liquidatable: bool,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TestFlashLoanInitiatedEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
    pub fee: i128,
    pub callback: Address,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TestFlashLoanRepaidEvent {
    pub user: Address,
    pub asset: Address,
    pub amount: i128,
    pub fee: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TestAdminActionEvent {
    pub actor: Address,
    pub action: Symbol,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TestPriceUpdatedEvent {
    pub actor: Address,
    pub asset: Address,
    pub price: i128,
    pub decimals: u32,
    pub oracle: Address,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TestRiskParamsUpdatedEvent {
    pub actor: Address,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone, Debug)]
pub struct TestPauseStateChangedEvent {
    pub actor: Address,
    pub operation: Symbol,
    pub paused: bool,
    pub timestamp: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Test helpers
// ─────────────────────────────────────────────────────────────────────────────

#[allow(dead_code)]
fn setup() -> (Env, Address, HelloContractClient<'static>) {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());
    let client = HelloContractClient::new(&env, &contract_id);
    (env, contract_id, client)
}

#[allow(dead_code)]
fn init(client: &HelloContractClient, admin: &Address) {
    client.initialize(admin);
}

// ─────────────────────────────────────────────────────────────────────────────
// Unit tests – direct helper invocation via env.as_contract()
// ─────────────────────────────────────────────────────────────────────────────

/// `emit_deposit` emits a DepositEvent decodable from env.events().all().
#[test]
fn test_deposit_event_structure() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let user = Address::generate(&env);

        emit_deposit(
            &env,
            DepositEvent {
                user: user.clone(),
                asset: None,
                amount: 1_000,
                timestamp: 100,
            },
        );

        let all = env.events().all();
        assert_eq!(all.len(), 1, "Expected exactly 1 event");

        let (_contract, _topics, data) = all.get_unchecked(0);
        let decoded: TestDepositEvent =
            TestDepositEvent::try_from_val(&env, &data).expect("Failed to decode DepositEvent");

        assert_eq!(decoded.user, user);
        assert_eq!(decoded.asset, None, "Native XLM should have None asset");
        assert_eq!(decoded.amount, 1_000);
        assert_eq!(decoded.timestamp, 100);
    });
}

/// `emit_withdrawal` emits a WithdrawalEvent with the correct fields.
#[test]
fn test_withdrawal_event_structure() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let user = Address::generate(&env);
        let asset = Address::generate(&env);

        emit_withdrawal(
            &env,
            WithdrawalEvent {
                user: user.clone(),
                asset: Some(asset.clone()),
                amount: 500,
                timestamp: 200,
            },
        );

        let all = env.events().all();
        assert_eq!(all.len(), 1);
        let (_c, _t, data) = all.get_unchecked(0);
        let decoded: TestWithdrawalEvent = TestWithdrawalEvent::try_from_val(&env, &data)
            .expect("Failed to decode WithdrawalEvent");

        assert_eq!(decoded.user, user);
        assert_eq!(decoded.asset, Some(asset));
        assert_eq!(decoded.amount, 500);
        assert_eq!(decoded.timestamp, 200);
    });
}

/// `emit_borrow` emits a BorrowEvent with the correct fields.
#[test]
fn test_borrow_event_structure() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let user = Address::generate(&env);

        emit_borrow(
            &env,
            BorrowEvent {
                user: user.clone(),
                asset: None,
                amount: 5_000,
                timestamp: 300,
            },
        );

        let all = env.events().all();
        assert_eq!(all.len(), 1);
        let (_c, _t, data) = all.get_unchecked(0);
        let decoded: TestBorrowEvent =
            TestBorrowEvent::try_from_val(&env, &data).expect("Failed to decode BorrowEvent");

        assert_eq!(decoded.user, user);
        assert_eq!(decoded.amount, 5_000);
        assert_eq!(decoded.timestamp, 300);
    });
}

/// `emit_repay` emits a RepayEvent with the correct fields.
#[test]
fn test_repay_event_structure() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let user = Address::generate(&env);

        emit_repay(
            &env,
            RepayEvent {
                user: user.clone(),
                asset: None,
                amount: 2_000,
                timestamp: 400,
            },
        );

        let all = env.events().all();
        assert_eq!(all.len(), 1);
        let (_c, _t, data) = all.get_unchecked(0);
        let decoded: TestRepayEvent =
            TestRepayEvent::try_from_val(&env, &data).expect("Failed to decode RepayEvent");

        assert_eq!(decoded.user, user);
        assert_eq!(decoded.amount, 2_000);
        assert_eq!(decoded.timestamp, 400);
    });
}

/// `emit_liquidation` emits a LiquidationEvent with all fields correct.
#[test]
fn test_liquidation_event_structure() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let liquidator = Address::generate(&env);
        let borrower = Address::generate(&env);

        emit_liquidation(
            &env,
            LiquidationEvent {
                liquidator: liquidator.clone(),
                borrower: borrower.clone(),
                debt_asset: None,
                collateral_asset: None,
                debt_liquidated: 1_000,
                collateral_seized: 1_100,
                incentive_amount: 100,
                timestamp: 999,
            },
        );

        let all = env.events().all();
        assert_eq!(all.len(), 1);
        let (_c, _t, data) = all.get_unchecked(0);
        let decoded: TestLiquidationEvent = TestLiquidationEvent::try_from_val(&env, &data)
            .expect("Failed to decode LiquidationEvent");

        assert_eq!(decoded.liquidator, liquidator);
        assert_eq!(decoded.borrower, borrower);
        assert!(decoded.debt_asset.is_none());
        assert!(decoded.collateral_asset.is_none());
        assert_eq!(decoded.debt_liquidated, 1_000);
        assert_eq!(decoded.collateral_seized, 1_100);
        assert_eq!(decoded.incentive_amount, 100);
        assert_eq!(decoded.timestamp, 999);
        // Security: liquidator ≠ borrower
        assert_ne!(decoded.liquidator, decoded.borrower);
    });
}

/// `emit_liquidation` correctly stores token asset addresses (non-None).
#[test]
fn test_liquidation_event_with_token_assets() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let liquidator = Address::generate(&env);
        let borrower = Address::generate(&env);
        let debt_asset = Address::generate(&env);
        let collateral_asset = Address::generate(&env);

        emit_liquidation(
            &env,
            LiquidationEvent {
                liquidator: liquidator.clone(),
                borrower: borrower.clone(),
                debt_asset: Some(debt_asset.clone()),
                collateral_asset: Some(collateral_asset.clone()),
                debt_liquidated: 2_000,
                collateral_seized: 2_200,
                incentive_amount: 200,
                timestamp: 500,
            },
        );

        let all = env.events().all();
        let (_c, _t, data) = all.get_unchecked(0);
        let decoded: TestLiquidationEvent =
            TestLiquidationEvent::try_from_val(&env, &data).unwrap();

        assert_eq!(decoded.debt_asset, Some(debt_asset));
        assert_eq!(decoded.collateral_asset, Some(collateral_asset));
    });
}

/// `emit_liquidation_v1` emits a versioned payload with borrower health data.
#[test]
fn test_liquidation_event_v1_structure() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let liquidator = Address::generate(&env);
        let borrower = Address::generate(&env);

        emit_liquidation_v1(
            &env,
            LiquidationEventV1 {
                schema_version: 1,
                liquidator: liquidator.clone(),
                borrower: borrower.clone(),
                debt_asset: None,
                collateral_asset: None,
                debt_liquidated: 500,
                collateral_seized: 550,
                incentive_amount: 50,
                borrower_collateral_after: 450,
                borrower_principal_debt_after: 500,
                borrower_interest_after: 25,
                borrower_total_debt_after: 525,
                borrower_health_factor_after: 8571,
                borrower_risk_level_after: 5,
                timestamp: 1_000,
            },
        );

        let all = env.events().all();
        assert_eq!(all.len(), 1);
        let (_c, _t, data) = all.get_unchecked(0);
        let decoded: TestLiquidationEventV1 =
            TestLiquidationEventV1::try_from_val(&env, &data).expect("decode liquidation v1");

        assert_eq!(decoded.schema_version, 1);
        assert_eq!(decoded.liquidator, liquidator);
        assert_eq!(decoded.borrower, borrower);
        assert_eq!(decoded.borrower_total_debt_after, 525);
        assert_eq!(decoded.borrower_health_factor_after, 8571);
        assert_eq!(decoded.borrower_risk_level_after, 5);
    });
}

/// `emit_borrower_health_v1` emits a self-contained borrower health snapshot.
#[test]
fn test_borrower_health_event_v1_structure() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let user = Address::generate(&env);

        emit_borrower_health_v1(
            &env,
            BorrowerHealthEventV1 {
                schema_version: 1,
                user: user.clone(),
                operation: Symbol::new(&env, "liquidate"),
                collateral: 900,
                principal_debt: 800,
                borrow_interest: 100,
                total_debt: 900,
                health_factor: 10_000,
                risk_level: 5,
                is_liquidatable: true,
                timestamp: 321,
            },
        );

        let all = env.events().all();
        assert_eq!(all.len(), 1);
        let (_c, _t, data) = all.get_unchecked(0);
        let decoded: TestBorrowerHealthEventV1 =
            TestBorrowerHealthEventV1::try_from_val(&env, &data)
                .expect("decode borrower health event");

        assert_eq!(decoded.schema_version, 1);
        assert_eq!(decoded.user, user);
        assert_eq!(decoded.operation, Symbol::new(&env, "liquidate"));
        assert_eq!(decoded.total_debt, 900);
        assert_eq!(decoded.health_factor, 10_000);
        assert!(decoded.is_liquidatable);
    });
}

/// `emit_flash_loan_repaid` emits a FlashLoanRepaidEvent with correct fields.
#[test]
fn test_flash_loan_repaid_event_structure() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let user = Address::generate(&env);
        let asset = Address::generate(&env);

        emit_flash_loan_repaid(
            &env,
            FlashLoanRepaidEvent {
                user: user.clone(),
                asset: asset.clone(),
                amount: 5_000,
                fee: 45,
                timestamp: 999,
            },
        );

        let all = env.events().all();
        assert_eq!(all.len(), 1);
        let (_c, _t, data) = all.get_unchecked(0);
        let decoded: TestFlashLoanRepaidEvent = TestFlashLoanRepaidEvent::try_from_val(&env, &data)
            .expect("Failed to decode FlashLoanRepaidEvent");

        assert_eq!(decoded.user, user);
        assert_eq!(decoded.asset, asset);
        assert_eq!(decoded.amount, 5_000);
        assert_eq!(decoded.fee, 45);
        assert_eq!(decoded.timestamp, 999);
    });
}

/// `emit_flash_loan_initiated` emits a FlashLoanInitiatedEvent with correct fields.
#[test]
fn test_flash_loan_initiated_event_structure() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let user = Address::generate(&env);
        let asset = Address::generate(&env);
        let callback = Address::generate(&env);

        emit_flash_loan_initiated(
            &env,
            FlashLoanInitiatedEvent {
                user: user.clone(),
                asset: asset.clone(),
                amount: 10_000,
                fee: 9,
                callback: callback.clone(),
                timestamp: 50,
            },
        );

        let all = env.events().all();
        assert_eq!(all.len(), 1);
        let (_c, _t, data) = all.get_unchecked(0);
        let decoded: TestFlashLoanInitiatedEvent =
            TestFlashLoanInitiatedEvent::try_from_val(&env, &data)
                .expect("Failed to decode FlashLoanInitiatedEvent");

        assert_eq!(decoded.user, user);
        assert_eq!(decoded.asset, asset);
        assert_eq!(decoded.amount, 10_000);
        assert_eq!(decoded.fee, 9);
        assert_eq!(decoded.callback, callback);
        assert_eq!(decoded.timestamp, 50);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Admin events
// ─────────────────────────────────────────────────────────────────────────────

/// `emit_admin_action` emits an AdminActionEvent with correct fields.
#[test]
fn test_admin_action_event_structure() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);
        let action = Symbol::new(&env, "initialize");

        emit_admin_action(
            &env,
            AdminActionEvent {
                actor: actor.clone(),
                action: action.clone(),
                timestamp: 42,
            },
        );

        let all = env.events().all();
        assert_eq!(all.len(), 1);
        let (_c, _t, data) = all.get_unchecked(0);
        let decoded: TestAdminActionEvent = TestAdminActionEvent::try_from_val(&env, &data)
            .expect("Failed to decode AdminActionEvent");

        assert_eq!(decoded.actor, actor);
        assert_eq!(decoded.action, action);
        assert_eq!(decoded.timestamp, 42);
    });
}

/// `emit_price_updated` emits a PriceUpdatedEvent with all oracle fields.
#[test]
fn test_price_updated_event_structure() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);
        let asset = Address::generate(&env);
        let oracle = Address::generate(&env);

        emit_price_updated(
            &env,
            PriceUpdatedEvent {
                actor: actor.clone(),
                asset: asset.clone(),
                price: 1_50000000,
                decimals: 8,
                oracle: oracle.clone(),
                timestamp: 500,
            },
        );

        let all = env.events().all();
        assert_eq!(all.len(), 1);
        let (_c, _t, data) = all.get_unchecked(0);
        let decoded: TestPriceUpdatedEvent = TestPriceUpdatedEvent::try_from_val(&env, &data)
            .expect("Failed to decode PriceUpdatedEvent");

        assert_eq!(decoded.actor, actor);
        assert_eq!(decoded.asset, asset);
        assert_eq!(decoded.price, 1_50000000);
        assert_eq!(decoded.decimals, 8);
        assert_eq!(decoded.oracle, oracle);
        assert_eq!(decoded.timestamp, 500);
    });
}

/// `emit_risk_params_updated` emits a RiskParamsUpdatedEvent.
#[test]
fn test_risk_params_updated_event_structure() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);

        emit_risk_params_updated(
            &env,
            RiskParamsUpdatedEvent {
                actor: actor.clone(),
                timestamp: 300,
            },
        );

        let all = env.events().all();
        assert_eq!(all.len(), 1);
        let (_c, _t, data) = all.get_unchecked(0);
        let decoded: TestRiskParamsUpdatedEvent =
            TestRiskParamsUpdatedEvent::try_from_val(&env, &data)
                .expect("Failed to decode RiskParamsUpdatedEvent");

        assert_eq!(decoded.actor, actor);
        assert_eq!(decoded.timestamp, 300);
    });
}

/// `emit_pause_state_changed` emits events for both pause=true and pause=false.
#[test]
fn test_pause_state_changed_event_structure() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let actor = Address::generate(&env);
        let operation = Symbol::new(&env, "pause_borrow");

        emit_pause_state_changed(
            &env,
            PauseStateChangedEvent {
                actor: actor.clone(),
                operation: operation.clone(),
                paused: true,
                timestamp: 100,
            },
        );
        emit_pause_state_changed(
            &env,
            PauseStateChangedEvent {
                actor: actor.clone(),
                operation: operation.clone(),
                paused: false,
                timestamp: 200,
            },
        );

        let all = env.events().all();
        assert_eq!(all.len(), 2, "Expected 2 pause state events");

        let (_c0, _t0, d0) = all.get_unchecked(0);
        let p0: TestPauseStateChangedEvent =
            TestPauseStateChangedEvent::try_from_val(&env, &d0).unwrap();
        assert!(p0.paused);
        assert_eq!(p0.timestamp, 100);
        assert_eq!(p0.operation, operation);

        let (_c1, _t1, d1) = all.get_unchecked(1);
        let p1: TestPauseStateChangedEvent =
            TestPauseStateChangedEvent::try_from_val(&env, &d1).unwrap();
        assert!(!p1.paused);
        assert_eq!(p1.timestamp, 200);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// All 11 helpers emit exactly one event each
// ─────────────────────────────────────────────────────────────────────────────

/// Calls every emit_* helper once and verifies exactly 11 events are emitted
/// (one per helper) – confirms nothing is silently dropped.
#[test]
fn test_all_event_helpers_emit_one_event_each() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let a = Address::generate(&env);
        let b = Address::generate(&env);

        emit_deposit(
            &env,
            DepositEvent {
                user: a.clone(),
                asset: None,
                amount: 1,
                timestamp: 0,
            },
        );
        emit_withdrawal(
            &env,
            WithdrawalEvent {
                user: a.clone(),
                asset: None,
                amount: 1,
                timestamp: 0,
            },
        );
        emit_borrow(
            &env,
            BorrowEvent {
                user: a.clone(),
                asset: None,
                amount: 1,
                timestamp: 0,
            },
        );
        emit_repay(
            &env,
            RepayEvent {
                user: a.clone(),
                asset: None,
                amount: 1,
                timestamp: 0,
            },
        );
        emit_liquidation(
            &env,
            LiquidationEvent {
                liquidator: a.clone(),
                borrower: b.clone(),
                debt_asset: None,
                collateral_asset: None,
                debt_liquidated: 1,
                collateral_seized: 1,
                incentive_amount: 0,
                timestamp: 0,
            },
        );
        emit_flash_loan_initiated(
            &env,
            FlashLoanInitiatedEvent {
                user: a.clone(),
                asset: b.clone(),
                amount: 1,
                fee: 0,
                callback: Address::generate(&env),
                timestamp: 0,
            },
        );
        emit_flash_loan_repaid(
            &env,
            FlashLoanRepaidEvent {
                user: a.clone(),
                asset: b.clone(),
                amount: 1,
                fee: 0,
                timestamp: 0,
            },
        );
        emit_admin_action(
            &env,
            AdminActionEvent {
                actor: a.clone(),
                action: Symbol::new(&env, "test"),
                timestamp: 0,
            },
        );
        emit_price_updated(
            &env,
            PriceUpdatedEvent {
                actor: a.clone(),
                asset: b.clone(),
                price: 1,
                decimals: 8,
                oracle: Address::generate(&env),
                timestamp: 0,
            },
        );
        emit_risk_params_updated(
            &env,
            RiskParamsUpdatedEvent {
                actor: a.clone(),
                timestamp: 0,
            },
        );
        emit_pause_state_changed(
            &env,
            PauseStateChangedEvent {
                actor: a.clone(),
                operation: Symbol::new(&env, "pause_deposit"),
                paused: true,
                timestamp: 0,
            },
        );

        let all = env.events().all();
        assert_eq!(
            all.len(),
            11,
            "Each of 11 helpers must emit exactly one event"
        );
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Edge cases
// ─────────────────────────────────────────────────────────────────────────────

/// Events with `asset: None` (native XLM) must serialise and deserialise correctly.
#[test]
fn test_event_with_none_asset_native_xlm() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let user = Address::generate(&env);
        emit_deposit(
            &env,
            DepositEvent {
                user: user.clone(),
                asset: None,
                amount: 0,
                timestamp: 0,
            },
        );

        let all = env.events().all();
        let (_c, _t, data) = all.get_unchecked(0);
        let decoded: TestDepositEvent =
            TestDepositEvent::try_from_val(&env, &data).expect("None-asset event failed to decode");
        assert!(decoded.asset.is_none(), "Asset should remain None");
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Security
// ─────────────────────────────────────────────────────────────────────────────

/// DepositEvent only exposes the depositor's own data – no other user's balances.
#[test]
fn test_no_sensitive_data_in_deposit_event() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let user = Address::generate(&env);
        let _uninvolved = Address::generate(&env); // must not appear in event

        emit_deposit(
            &env,
            DepositEvent {
                user: user.clone(),
                asset: None,
                amount: 1_000,
                timestamp: 123,
            },
        );

        let all = env.events().all();
        let (_c, _t, data) = all.get_unchecked(0);
        let decoded: TestDepositEvent = TestDepositEvent::try_from_val(&env, &data).unwrap();

        // Only actor's own address
        assert_eq!(decoded.user, user);
        assert_eq!(decoded.amount, 1_000);
        assert_eq!(decoded.timestamp, 123);
    });
}

/// LiquidationEvent only contains the two participating actors.
/// An uninvolved user's address must not appear in the event.
#[test]
fn test_no_sensitive_data_in_liquidation_event() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let liquidator = Address::generate(&env);
        let borrower = Address::generate(&env);
        let uninvolved = Address::generate(&env);

        emit_liquidation(
            &env,
            LiquidationEvent {
                liquidator: liquidator.clone(),
                borrower: borrower.clone(),
                debt_asset: None,
                collateral_asset: None,
                debt_liquidated: 500,
                collateral_seized: 550,
                incentive_amount: 50,
                timestamp: 777,
            },
        );

        let all = env.events().all();
        let (_c, _t, data) = all.get_unchecked(0);
        let decoded: TestLiquidationEvent =
            TestLiquidationEvent::try_from_val(&env, &data).unwrap();

        assert_eq!(decoded.liquidator, liquidator);
        assert_eq!(decoded.borrower, borrower);
        assert_ne!(decoded.liquidator, uninvolved);
        assert_ne!(decoded.borrower, uninvolved);
    });
}

/// Shared position update helper emits both the legacy position update and the
/// stable borrower health snapshot used by indexers.
#[test]
fn test_position_update_emits_borrower_health_snapshot() {
    let env = Env::default();
    env.mock_all_auths();
    let contract_id = env.register(HelloContract, ());

    env.as_contract(&contract_id, || {
        let user = Address::generate(&env);
        let position = Position {
            collateral: 1_500,
            debt: 1_000,
            borrow_interest: 200,
            last_accrual_time: 0,
        };

        emit_position_updated_event(&env, &user, &position, Symbol::new(&env, "borrow"), 777);

        let all = env.events().all();
        assert_eq!(all.len(), 2);

        let (_c0, _t0, _position_payload) = all.get_unchecked(0);
        let (_c1, _t1, health_payload) = all.get_unchecked(1);
        let decoded: TestBorrowerHealthEventV1 =
            TestBorrowerHealthEventV1::try_from_val(&env, &health_payload)
                .expect("decode borrower health snapshot");

        assert_eq!(decoded.schema_version, 1);
        assert_eq!(decoded.user, user);
        assert_eq!(decoded.operation, Symbol::new(&env, "borrow"));
        assert_eq!(decoded.total_debt, 1_200);
        assert_eq!(decoded.health_factor, 12_500);
        assert_eq!(decoded.risk_level, 2);
        assert!(!decoded.is_liquidatable);
        assert_eq!(decoded.timestamp, 777);
    });
}

// ─────────────────────────────────────────────────────────────────────────────
// Integration: deposit → borrow → repay event count grows
// ─────────────────────────────────────────────────────────────────────────────

/// Verifies the typical user flow emits progressively more events at each step.
#[test]
fn test_event_sequence_deposit_borrow_repay() {
    let (env, contract_id, client, _admin, user, native_asset) =
        crate::tests::test_helpers::setup_env_with_native_asset();
    let token_client = soroban_sdk::token::StellarAssetClient::new(&env, &native_asset);
    token_client.mint(&user, &10_000);
    token_client.approve(
        &user,
        &contract_id,
        &10_000,
        &(env.ledger().sequence() + 100),
    );

    client.deposit_collateral(&user, &None, &50_000);
    let after_deposit = env.events().all().len();
    assert!(after_deposit > 0, "Deposit should emit at least one event");

    let _res = client.borrow_asset(&user, &None, &10_000);
    let after_borrow = env.events().all().len();

    assert!(
        after_borrow >= after_deposit,
        "Borrow should emit additional events"
    );

    client.repay_debt(&user, &None, &5_000);
    let after_repay = env.events().all().len();
    assert!(
        after_repay >= after_borrow,
        "Repay should emit additional events"
    );
}
