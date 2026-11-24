//! Test-only mock contracts and invariant helpers.

use super::*;

#[cfg(test)]
#[contract]
pub struct MockAmm;
#[cfg(test)]
#[contractimpl]
impl MockAmm {
    pub fn swap(
        _env: Env,
        _caller: Address,
        _in: Address,
        _out: Address,
        amount_in: i128,
        min_out: i128,
        _dead: u64,
    ) -> i128 {
        let out = amount_in * 2;
        if out < min_out {
            panic_with_error!(&env, LendingError::InvalidAmount);
        }
        out
    }
}

#[cfg(test)]
#[contract]
pub struct BadAmm;
#[cfg(test)]
#[contractimpl]
impl BadAmm {
    pub fn swap(
        _env: Env,
        _caller: Address,
        _in: Address,
        _out: Address,
        amount_in: i128,
        _min: i128,
        _dead: u64,
    ) -> i128 {
        amount_in / 4
    }
}

#[cfg(test)]
pub fn assert_totals_consistent(env: &Env, users: &[Address]) {
    let mut sum_debt: i128 = 0;
    let mut sum_col: i128 = 0;
    for u in users {
        sum_debt = sum_debt.saturating_add(load_debt(env, u).principal);
        let c: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Collateral(u.clone()))
            .unwrap_or(0);
        sum_col = sum_col.saturating_add(c);
    }
    let total_debt: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDebt)
        .unwrap_or(0);
    let total_col: i128 = env
        .storage()
        .persistent()
        .get(&DataKey::TotalDeposits)
        .unwrap_or(0);
    assert_eq!(sum_debt, total_debt, "sum(user debt) == TotalDebt");
    assert_eq!(sum_col, total_col, "sum(collateral) == TotalDeposits");
}

#[cfg(test)]
#[contract]
pub struct MockAsset;
#[cfg(test)]
#[contractimpl]
impl MockAsset {}
