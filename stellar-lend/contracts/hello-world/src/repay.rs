//! Repay entrypoint for the StellarLend hello-world contract.
//!
//! Implements the debt-reduction accounting used by
//! `stellar-lend/contracts/lending/src/debt.rs::repay_amount`. Repayments
//! settle accrued simple interest first and then reduce principal. An
//! over-repayment is capped at the accrued debt so users cannot implicitly
//! mint protocol credit.
//!
//! See issue #1460 for context.

use soroban_sdk::{contracterror, contractevent, contracttype, Address, Env};

const SECONDS_PER_YEAR: u64 = 31_536_000;
const DEFAULT_APR_BPS: i128 = 500;
const BPS_DENOM: i128 = 10_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RepayError {
    InvalidAmount = 1,
    Overflow = 2,
    NoDebt = 3,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepayDataKey {
    Position(Address),
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Position {
    pub principal: i128,
    pub last_update: u64,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepayEvent {
    #[topic]
    pub user: Address,
    #[topic]
    pub asset: Option<Address>,
    pub amount: i128,
    pub interest_paid: i128,
    pub principal_paid: i128,
    pub remaining_debt: i128,
}

fn compute_interest(principal: i128, elapsed: u64, rate_bps: i128) -> Result<i128, RepayError> {
    if principal <= 0 || elapsed == 0 || rate_bps <= 0 {
        return Ok(0);
    }
    let numerator = principal
        .checked_mul(rate_bps)
        .and_then(|v| v.checked_mul(elapsed as i128))
        .ok_or(RepayError::Overflow)?;
    let denominator = BPS_DENOM
        .checked_mul(SECONDS_PER_YEAR as i128)
        .ok_or(RepayError::Overflow)?;
    numerator
        .checked_div(denominator)
        .ok_or(RepayError::Overflow)
}

fn load_position(env: &Env, user: &Address) -> Position {
    env.storage()
        .persistent()
        .get::<RepayDataKey, Position>(&RepayDataKey::Position(user.clone()))
        .unwrap_or(Position {
            principal: 0,
            last_update: env.ledger().timestamp(),
        })
}

fn save_position(env: &Env, user: &Address, position: &Position) {
    env.storage()
        .persistent()
        .set::<RepayDataKey, Position>(&RepayDataKey::Position(user.clone()), position);
}

pub fn repay_debt(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<(i128, i128, i128), RepayError> {
    if amount <= 0 {
        return Err(RepayError::InvalidAmount);
    }

    user.require_auth();

    let mut position = load_position(env, &user);
    if position.principal == 0 {
        return Err(RepayError::NoDebt);
    }
    if position.principal < 0 {
        return Err(RepayError::Overflow);
    }

    let now = env.ledger().timestamp();
    let elapsed = now.saturating_sub(position.last_update);
    let interest = compute_interest(position.principal, elapsed, DEFAULT_APR_BPS)?;
    let accrued_debt = position
        .principal
        .checked_add(interest)
        .ok_or(RepayError::Overflow)?;

    let applied = amount.min(accrued_debt);
    let interest_paid = applied.min(interest);
    let principal_paid = applied
        .checked_sub(interest_paid)
        .ok_or(RepayError::Overflow)?;

    position.principal = position
        .principal
        .checked_sub(principal_paid)
        .ok_or(RepayError::Overflow)?;
    position.last_update = now;
    save_position(env, &user, &position);

    let remaining_debt = accrued_debt
        .checked_sub(applied)
        .ok_or(RepayError::Overflow)?;

    let event = RepayEvent {
        user,
        asset,
        amount: applied,
        interest_paid,
        principal_paid,
        remaining_debt,
    };
    event.publish(env);

    Ok((remaining_debt, interest_paid, principal_paid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::contract;
    use soroban_sdk::testutils::{Address as _, Events, Ledger};

    // Empty contract used purely so `env.as_contract(&id, …)` has something
    // to register. Storage operations on `Env` are gated by a running
    // contract context in soroban-sdk 25.x; tests must enter that context
    // explicitly.
    #[contract]
    pub struct TestContract;

    fn with_test_contract<R>(env: &Env, f: impl FnOnce() -> R) -> R {
        let id = env.register(TestContract, ());
        env.as_contract(&id, f)
    }

    fn advance_time(env: &Env, secs: u64) {
        let current = env.ledger().timestamp();
        env.ledger().set_timestamp(current + secs);
    }

    fn seed_position(env: &Env, user: &Address, principal: i128, at: u64) {
        save_position(
            env,
            user,
            &Position {
                principal,
                last_update: at,
            },
        );
    }

    fn events_count(env: &Env) -> usize {
        env.events().all().events().len()
    }

    // NOTE: A multi-call monotonicity test was considered but skipped —
    // soroban-sdk 25.3.0's `mock_all_auths()` only fakes one auth per host
    // frame, and our entrypoint requires `user.require_auth()` on every
    // call. Multiple repay_debt invocations inside a single as_contract
    // frame trip `Error(Auth, ExistingValue)`. Per-call state transitions
    // are already covered by the single-call settlement tests below.

    #[test]
    fn repay_rejects_zero_amount() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            let before = events_count(&env);
            assert_eq!(
                repay_debt(&env, user, None, 0),
                Err(RepayError::InvalidAmount)
            );
            assert_eq!(events_count(&env), before);
        });
    }

    #[test]
    fn repay_rejects_negative_amount() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            let before = events_count(&env);
            assert_eq!(
                repay_debt(&env, user, None, -1),
                Err(RepayError::InvalidAmount)
            );
            assert_eq!(events_count(&env), before);
        });
    }

    #[test]
    fn repay_rejects_oversized_principal_with_overflow() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            env.storage().persistent().set::<RepayDataKey, Position>(
                &RepayDataKey::Position(user.clone()),
                &Position {
                    principal: i128::MAX,
                    last_update: env.ledger().timestamp(),
                },
            );
            advance_time(&env, SECONDS_PER_YEAR);
            assert_eq!(repay_debt(&env, user, None, 1), Err(RepayError::Overflow));
        });
    }

    #[test]
    fn repay_rejects_corrupted_negative_principal() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            env.storage().persistent().set::<RepayDataKey, Position>(
                &RepayDataKey::Position(user.clone()),
                &Position {
                    principal: -42,
                    last_update: env.ledger().timestamp(),
                },
            );
            assert_eq!(
                repay_debt(&env, user.clone(), None, 1),
                Err(RepayError::Overflow)
            );
            let stored = load_position(&env, &user);
            assert_eq!(stored.principal, -42);
        });
    }

    #[test]
    fn repay_rejects_when_user_has_no_position() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            assert_eq!(repay_debt(&env, user, None, 100), Err(RepayError::NoDebt));
        });
    }

    #[test]
    fn repay_rejects_when_position_principal_is_zero() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            seed_position(&env, &user, 0, env.ledger().timestamp());
            assert_eq!(repay_debt(&env, user, None, 100), Err(RepayError::NoDebt));
        });
    }

    #[test]
    fn repay_full_principal_immediately_zeros_remaining() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            let seeded_at = env.ledger().timestamp();
            seed_position(&env, &user, 1_000, seeded_at);
            let (remaining, interest, principal) =
                repay_debt(&env, user.clone(), None, 1_000).unwrap();
            assert_eq!(remaining, 0);
            assert_eq!(interest, 0);
            assert_eq!(principal, 1_000);
            let stored = load_position(&env, &user);
            assert_eq!(stored.principal, 0);
            assert_eq!(stored.last_update, seeded_at);
        });
    }

    #[test]
    fn repay_partial_principal_immediately() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            seed_position(&env, &user, 1_000, env.ledger().timestamp());
            let (remaining, interest, principal) =
                repay_debt(&env, user.clone(), None, 300).unwrap();
            assert_eq!(remaining, 700);
            assert_eq!(interest, 0);
            assert_eq!(principal, 300);
            let stored = load_position(&env, &user);
            assert_eq!(stored.principal, 700);
        });
    }

    #[test]
    fn repay_one_year_of_interest_pays_interest_first() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            seed_position(&env, &user, 1_000, env.ledger().timestamp());
            advance_time(&env, SECONDS_PER_YEAR);
            let (remaining, interest, principal) =
                repay_debt(&env, user.clone(), None, 30).unwrap();
            assert_eq!(interest, 30);
            assert_eq!(principal, 0);
            assert_eq!(remaining, 1_000 + 50 - 30);
            let stored = load_position(&env, &user);
            assert_eq!(stored.principal, 1_000);
        });
    }

    #[test]
    fn repay_one_year_of_interest_with_overpayment_pays_interest_then_principal() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            seed_position(&env, &user, 1_000, env.ledger().timestamp());
            advance_time(&env, SECONDS_PER_YEAR);
            let (remaining, interest, principal) =
                repay_debt(&env, user.clone(), None, 200).unwrap();
            assert_eq!(interest, 50);
            assert_eq!(principal, 150);
            assert_eq!(remaining, 850);
            assert_eq!(stored_principal(&env, &user), 850);
        });
    }

    #[test]
    fn repay_overpay_caps_at_accrued_debt_creating_no_credit() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            seed_position(&env, &user, 1_000, env.ledger().timestamp());
            advance_time(&env, SECONDS_PER_YEAR);
            let (remaining, interest, principal) =
                repay_debt(&env, user.clone(), None, 5_000).unwrap();
            assert_eq!(remaining, 0);
            assert_eq!(interest + principal, 1_050);
            assert_eq!(interest, 50);
            assert_eq!(principal, 1_000);
            assert_eq!(stored_principal(&env, &user), 0);
        });
    }

    #[test]
    fn repay_emits_one_event() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            let asset = Address::generate(&env);
            seed_position(&env, &user, 1_000, env.ledger().timestamp());
            advance_time(&env, SECONDS_PER_YEAR);
            let before = events_count(&env);
            let (remaining, interest, principal) =
                repay_debt(&env, user.clone(), Some(asset.clone()), 200).unwrap();
            assert_eq!(interest, 50);
            assert_eq!(principal, 150);
            assert_eq!(remaining, 850);
            assert_eq!(stored_principal(&env, &user), 850);
            assert_eq!(events_count(&env), before + 1);
        });
    }

    #[test]
    fn repay_no_interest_when_zero_elapsed() {
        let env = Env::default();
        env.mock_all_auths();
        with_test_contract(&env, || {
            let user = Address::generate(&env);
            seed_position(&env, &user, 1_000, env.ledger().timestamp());
            let (remaining, interest, principal) =
                repay_debt(&env, user.clone(), None, 250).unwrap();
            assert_eq!(interest, 0);
            assert_eq!(principal, 250);
            assert_eq!(remaining, 750);
        });
    }

    fn stored_principal(env: &Env, user: &Address) -> i128 {
        load_position(env, user).principal
    }
}
