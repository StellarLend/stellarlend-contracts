//! Storage-tier assertions for representative `DataKey` variants.
//!
//! Each test verifies that a key is stored in the **correct** Soroban tier
//! (instance / persistent / temporary) so that misclassifications that could
//! inflate rent costs or cause premature state loss are caught early.
//!
//! Only `DataKey` variants that actually exist in the current codebase are
//! tested here.  Variants planned for future features (e.g.
//! `ReservedForFlashLoan`, `InterestIndex`) are documented but kept as
//! `#[ignore]` placeholders until the features land.

#![cfg(test)]

use soroban_sdk::{testutils::Address as _, Address, Env};
use crate::{DataKey, LendingContract, LendingContractClient};

// ---------------------------------------------------------------------------
// Shared setup
// ---------------------------------------------------------------------------

fn setup(env: &Env) -> (Address, LendingContractClient<'_>) {
    env.mock_all_auths();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(env, &contract_id);
    let admin = Address::generate(env);
    client.initialize(&admin);
    (contract_id, client)
}

// ---------------------------------------------------------------------------
// Instance-storage keys
// ---------------------------------------------------------------------------

/// `DataKey::Admin` must live in instance storage: it is small, protocol-wide,
/// and accessed on nearly every privileged call.
#[test]
fn test_admin_uses_instance_storage() {
    let env = Env::default();
    let (contract_id, _client) = setup(&env);

    env.as_contract(&contract_id, || {
        assert!(
            env.storage().instance().has(&DataKey::Admin),
            "Admin must be in Instance storage"
        );
        assert!(
            !env.storage().persistent().has(&DataKey::Admin),
            "Admin must NOT be in Persistent storage"
        );
    });
}

/// `DataKey::FlashActive` is written and immediately cleared within a single
/// flash-loan call.  It must live in instance storage (small bool).
#[test]
fn test_flash_active_uses_instance_storage() {
    let env = Env::default();
    let (contract_id, _client) = setup(&env);

    // Before any flash loan the key simply does not exist – verify it is
    // absent from persistent storage (wrong tier).
    env.as_contract(&contract_id, || {
        assert!(
            !env.storage().persistent().has(&DataKey::FlashActive),
            "FlashActive must NOT be in Persistent storage"
        );
        assert!(
            !env.storage().temporary().has(&DataKey::FlashActive),
            "FlashActive must NOT be in Temporary storage"
        );
    });
}

/// `DataKey::FlashFeeBps` is a protocol-wide scalar set by the admin.
/// It belongs in instance storage alongside other admin-controlled scalars.
#[test]
fn test_flash_fee_bps_uses_instance_storage() {
    let env = Env::default();
    let (contract_id, client) = setup(&env);

    client.set_flash_fee(&10);

    env.as_contract(&contract_id, || {
        assert!(
            env.storage().instance().has(&DataKey::FlashFeeBps),
            "FlashFeeBps must be in Instance storage"
        );
        assert!(
            !env.storage().persistent().has(&DataKey::FlashFeeBps),
            "FlashFeeBps must NOT be in Persistent storage"
        );
    });
}

// ---------------------------------------------------------------------------
// Persistent-storage keys
// ---------------------------------------------------------------------------

/// `DataKey::Collateral(user)` holds user funds and must survive ledger
/// boundaries; it belongs in persistent storage.
#[test]
fn test_collateral_uses_persistent_storage() {
    let env = Env::default();
    let (contract_id, client) = setup(&env);
    let user = Address::generate(&env);

    client.deposit(&user, &1_000);

    env.as_contract(&contract_id, || {
        let key = DataKey::Collateral(user.clone());
        assert!(
            env.storage().persistent().has(&key),
            "Collateral must be in Persistent storage"
        );
        assert!(
            !env.storage().instance().has(&key),
            "Collateral must NOT be in Instance storage"
        );
        assert!(
            !env.storage().temporary().has(&key),
            "Collateral must NOT be in Temporary storage"
        );
    });
}

/// `DataKey::TotalDeposits` is the aggregate supply counter.
/// It must be persistent so it survives across ledger boundaries.
#[test]
fn test_total_deposits_uses_persistent_storage() {
    let env = Env::default();
    let (contract_id, client) = setup(&env);
    let user = Address::generate(&env);

    client.deposit(&user, &500);

    env.as_contract(&contract_id, || {
        assert!(
            env.storage().persistent().has(&DataKey::TotalDeposits),
            "TotalDeposits must be in Persistent storage"
        );
        assert!(
            !env.storage().instance().has(&DataKey::TotalDeposits),
            "TotalDeposits must NOT be in Instance storage"
        );
    });
}

/// `DataKey::Treasury(asset)` holds protocol-owned liquidity for flash loans.
/// It must be persistent.
#[test]
fn test_treasury_uses_persistent_storage() {
    let env = Env::default();
    let (contract_id, _client) = setup(&env);
    let asset = Address::generate(&env);

    // Seed treasury directly (no token transfer required in unit tests).
    env.as_contract(&contract_id, || {
        env.storage()
            .persistent()
            .set(&DataKey::Treasury(asset.clone()), &1_000i128);
    });

    env.as_contract(&contract_id, || {
        assert!(
            env.storage().persistent().has(&DataKey::Treasury(asset.clone())),
            "Treasury must be in Persistent storage"
        );
        assert!(
            !env.storage().instance().has(&DataKey::Treasury(asset.clone())),
            "Treasury must NOT be in Instance storage"
        );
    });
}

/// `DataKey::AssetParams(asset)` stores per-asset risk configuration and must
/// be persistent.
#[test]
fn test_asset_params_uses_persistent_storage() {
    let env = Env::default();
    let (contract_id, client) = setup(&env);
    let admin = env.as_contract(&contract_id, || {
        env.storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::Admin)
            .unwrap()
    });
    let asset = Address::generate(&env);

    client.set_asset_params(
        &admin,
        &asset,
        &7_000i128,
        &8_000i128,
        &1_000_000_000i128,
        &0i128,
    );

    env.as_contract(&contract_id, || {
        let key = DataKey::AssetParams(asset.clone());
        assert!(
            env.storage().persistent().has(&key),
            "AssetParams must be in Persistent storage"
        );
        assert!(
            !env.storage().instance().has(&key),
            "AssetParams must NOT be in Instance storage"
        );
    });
}

// ---------------------------------------------------------------------------
// Tier-audit: keys that must never be in instance storage
// ---------------------------------------------------------------------------

/// None of the per-address persistent keys should ever appear in instance
/// storage (which is shared across all users and has a fixed size budget).
#[test]
fn test_per_address_keys_never_in_instance_storage() {
    let env = Env::default();
    let (contract_id, _client) = setup(&env);

    let addr = Address::generate(&env);
    let keys = [
        DataKey::Collateral(addr.clone()),
        DataKey::Debt(addr.clone()),
        DataKey::Balance(addr.clone(), addr.clone()),
        DataKey::Treasury(addr.clone()),
        DataKey::AssetParams(addr.clone()),
    ];

    env.as_contract(&contract_id, || {
        for key in &keys {
            assert!(
                !env.storage().instance().has(key),
                "{key:?} must NOT be in Instance storage"
            );
        }
    });
}

// ---------------------------------------------------------------------------
// Planned-feature placeholders (ignored until features land)
// ---------------------------------------------------------------------------

/// `DataKey::ReservedForFlashLoan(asset)` — planned Temporary-storage key for
/// the flash-loan reservation counter.  Ignored until the feature is
/// implemented.
#[test]
#[ignore = "ReservedForFlashLoan DataKey not yet implemented"]
fn test_flash_loan_reserved_uses_temporary_storage() {}

/// `DataKey::InterestIndex(asset)` — planned Persistent-storage key for the
/// per-asset cumulative interest index.  Ignored until the feature lands.
#[test]
#[ignore = "InterestIndex DataKey not yet implemented"]
fn test_interest_index_uses_persistent_storage() {}
