//! Shared storage TTL policy for persistent contract state.

use soroban_sdk::{Env, IntoVal, Val};

/// Approximate number of Soroban ledgers in one day (5 seconds per ledger).
pub const DAY_IN_LEDGERS: u32 = 17_280;

/// Instance storage is extended to this lifetime when below its threshold.
pub const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
/// Instance storage is refreshed when fewer than seven days remain.
pub const INSTANCE_LIFETIME_THRESHOLD: u32 = 7 * DAY_IN_LEDGERS;

/// Persistent entries are extended to this lifetime when below their threshold.
pub const PERSISTENT_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
/// Persistent entries are refreshed when fewer than seven days remain.
pub const PERSISTENT_LIFETIME_THRESHOLD: u32 = 7 * DAY_IN_LEDGERS;

/// Extend instance storage when its remaining lifetime is below policy threshold.
pub fn bump_instance(env: &Env) {
    let extend_to = INSTANCE_BUMP_AMOUNT.min(env.storage().max_ttl());
    if extend_to == 0 {
        return;
    }
    let threshold = INSTANCE_LIFETIME_THRESHOLD.min(extend_to);
    env.storage().instance().extend_ttl(threshold, extend_to);
}

/// Extend a persistent entry when needed. Missing entries are left untouched.
pub fn bump_persistent<K>(env: &Env, key: &K)
where
    K: IntoVal<Env, Val>,
{
    if !env.storage().persistent().has(key) {
        return;
    }

    let extend_to = PERSISTENT_BUMP_AMOUNT.min(env.storage().max_ttl());
    if extend_to == 0 {
        return;
    }
    let threshold = PERSISTENT_LIFETIME_THRESHOLD.min(extend_to);
    env.storage()
        .persistent()
        .extend_ttl(key, threshold, extend_to);
}

/// Write a persistent entry and immediately apply the shared TTL policy.
pub fn set_persistent<K, V>(env: &Env, key: &K, value: &V)
where
    K: IntoVal<Env, Val>,
    V: IntoVal<Env, Val>,
{
    env.storage().persistent().set(key, value);
    bump_instance(env);
    bump_persistent(env, key);
}
