//! Borrower registry and per-user TTL helpers.

use super::*;

pub(crate) fn extend_collateral_ttl(env: &Env, user: &Address) {
    let key = DataKey::Collateral(user.clone());
    let extend_to = env.storage().max_ttl().min(PERSISTENT_TTL_LEDGERS);
    let threshold = extend_to / 2 + 1;
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, threshold, extend_to);
    }
}

// Keep the `FirstUnhealthyTimestamp(user)` anchor alive for the duration of
// the liquidation grace period (same policy as `extend_debt_ttl`).
pub(crate) fn extend_first_unhealthy_ttl(env: &Env, user: &Address) {
    let key = DataKey::FirstUnhealthyTimestamp(user.clone());
    let extend_to = env.storage().max_ttl().min(PERSISTENT_TTL_LEDGERS);
    let threshold = extend_to / 2 + 1;
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, threshold, extend_to);
    }
}

pub(crate) fn extend_debt_ttl(env: &Env, user: &Address) {
    let key = DataKey::Debt(user.clone());
    let extend_to = env.storage().max_ttl().min(PERSISTENT_TTL_LEDGERS);
    let threshold = extend_to / 2 + 1;
    if env.storage().persistent().has(&key) {
        env.storage()
            .persistent()
            .extend_ttl(&key, threshold, extend_to);
    }
}

// Append `user` to the `BorrowerList` stored in instance storage, if not
// already present.  This list is used by `migrate_positions` to iterate
// over all debt records. Also used by the reserve-factor checkpoint, so it
// must run in production borrow paths.
pub(crate) fn register_borrower(env: &Env, user: &Address) {
    let mut list: soroban_sdk::Vec<Address> = env
        .storage()
        .instance()
        .get(&DataKey::BorrowerList)
        .unwrap_or_else(|| soroban_sdk::vec![env]);

    // Linear scan is acceptable: the list is bounded by the number of unique
    // borrowers and is read/written only on borrow (not every tx).
    for i in 0..list.len() {
        if list.get(i).unwrap() == *user {
            return; // already registered
        }
    }
    list.push_back(user.clone());
    env.storage().instance().set(&DataKey::BorrowerList, &list);
}
