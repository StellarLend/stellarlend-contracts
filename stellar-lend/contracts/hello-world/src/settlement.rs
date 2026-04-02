//! Link logical transaction ids to settlement batches. Prevents double-settlement.

use crate::risk_management::{require_admin, RiskManagementError};
use soroban_sdk::{contracttype, Address, Env, Vec};

#[contracttype]
#[derive(Clone)]
pub enum SettlementDataKey {
    Tx(u64),
}

/// Settlement id linked to `tx_id`, if that transaction has been finalized into a settlement.
pub fn get_tx_settlement_id(env: &Env, tx_id: u64) -> Option<u64> {
    env.storage()
        .persistent()
        .get(&SettlementDataKey::Tx(tx_id))
}

/// Associates each `tx_id` with `settlement_id`. Admin only.
///
/// # Panics
///
/// Panics with `"transaction already settled"` if any `tx_id` already has a settlement link.
pub fn finalize_settlement(
    env: &Env,
    caller: &Address,
    settlement_id: u64,
    tx_ids: Vec<u64>,
) -> Result<(), RiskManagementError> {
    require_admin(env, caller)?;
    let n = tx_ids.len();
    for i in 0..n {
        let tx_id = tx_ids.get(i).unwrap();
        if get_tx_settlement_id(env, tx_id).is_some() {
            panic!("transaction already settled");
        }
    }
    for i in 0..n {
        let tx_id = tx_ids.get(i).unwrap();
        env.storage()
            .persistent()
            .set(&SettlementDataKey::Tx(tx_id), &settlement_id);
    }
    Ok(())
}
