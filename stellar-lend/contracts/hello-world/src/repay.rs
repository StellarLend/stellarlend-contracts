//! Repay entrypoint for the StellarLend hello-world contract.
//!
//! Implements the debt-reduction accounting used by
//! `stellar-lend/contracts/lending/src/debt.rs::repay_amount`. Repayments
//! settle accrued simple interest first and then reduce principal. An
//! over-repayment is capped at the accrued debt so users cannot implicitly
//! mint protocol credit.
//!
//! ## Scope
//!
//! The hello-world crate is currently a stub skeleton: many sibling modules
//! (`risk_management`, `reentrancy`, `interest_rate`, …) are placeholders.
//! `HelloContract::repay_debt` calls this function via the wired call site in
//! `lib.rs`. Pause / emergency / reentrancy guards normally live in
//! `risk_management` and `reentrancy`; this entrypoint does not depend on
//! them yet. See issue #1460 for context.

use soroban_sdk::{contracterror, contractevent, contracttype, Address, Env};

/// Seconds in a non-leap year, used to convert basis-points APR into a
/// per-second interest factor.
const SECONDS_PER_YEAR: u64 = 31_536_000;
/// Default annual interest rate in basis points (5%).
const DEFAULT_APR_BPS: i128 = 500;
/// Basis-points denominator (1.00 == 10_000 bps).
const BPS_DENOM: i128 = 10_000;

/// Errors raised by [`repay_debt`].
///
/// `InvalidAsset`, `InsufficientBalance`, and `RepayPaused` are intentionally
/// not present here: those checks depend on `risk_management` / token-client
/// modules that are still stubs in the hello-world skeleton. They will be
/// wired in a follow-up once those modules become real implementations.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RepayError {
    /// The repayment amount must be strictly positive.
    InvalidAmount = 1,
    /// Arithmetic overflow/underflow during repayment accounting.
    Overflow = 2,
    /// The user has no outstanding principal debt to repay against.
    NoDebt = 3,
}

/// Storage namespace for the repay module's per-user state.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RepayDataKey {
    /// Per-user debt position (principal + last update timestamp).
    Position(Address),
}

/// Per-user debt position tracked by the repay entrypoint.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Position {
    pub principal: i128,
    pub last_update: u64,
}

/// Emitted after a repayment is applied to a user's outstanding debt.
#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepayEvent {
    #[topic]
    pub user: Address,
    #[topic]
    pub asset: Option<Address>,
    /// The actual amount applied to debt (capped at accrued debt).
    pub amount: i128,
    /// Amount of repaid interest accrued since `last_update`.
    pub interest_paid: i128,
    /// Amount of repaid principal.
    pub principal_paid: i128,
    /// Outstanding debt after the repayment.
    pub remaining_debt: i128,
}

/// Compute simple interest accrued over `elapsed` seconds at `rate_bps` APR.
///
/// Formula: `interest = principal * rate_bps * elapsed / (BPS_DENOM * SECONDS_PER_YEAR)`.
/// Every multiplication is checked so that pathological inputs surface as
/// `RepayError::Overflow` instead of silently wrapping.
fn compute_interest(
    principal: i128,
    elapsed: u64,
    rate_bps: i128,
) -> Result<i128, RepayError> {
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
    numerator.checked_div(denominator).ok_or(RepayError::Overflow)
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

/// Repay the caller's outstanding debt for `asset`.
///
/// Settlement policy: accrued simple interest is paid first, then principal.
/// Returns `(remaining_debt, interest_paid, principal_paid)`. The total
/// applied to debt is `interest_paid + principal_paid`. Overpayment is
/// capped at the accrued debt; no credit is implicitly created.
pub fn repay_debt(
    env: &Env,
    user: Address,
    asset: Option<Address>,
    amount: i128,
) -> Result<(i128, i128, i128), RepayError> {
    if amount <= 0 {
        return Err(RepayError::InvalidAmount);
    }

    // Mirrors `docs/repay.md`: "The repay_debt function requires the user's
    // authorization for the transfer." Production token settlement wired
    // through `risk_management` will rely on this gate even though this
    // entrypoint does not (yet) move tokens itself.
    user.require_auth();

    let mut position = load_position(env, &user);
    // No principal means no outstanding debt — interest cannot accrue on a
    // zero principal (see `compute_interest` short-circuit), so there is
    // nothing legitimate to repay here.
    if position.principal == 0 {
        return Err(RepayError::NoDebt);
    }
    // A negative principal can only come from outside-the-protocol storage
    // tampering. Refuse to compute on it instead of silently emitting a
    // negative `RepayEvent` and rewriting the same corrupted value back.
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

    // Cap the applied payment at the accrued debt so over-repayment cannot
    // push the user into a phantom credit state.
    let applied = amount.min(accrued_debt);

    // Interest-first, principal-second split.
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

    RepayEvent {
        user,
        asset,
        amount: applied,
        interest_paid,
        principal_paid,
        remaining_debt,
    }
    .publish(env);

    Ok((remaining_debt, interest_paid, principal_paid))
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Events, Ledger};
    use soroban_sdk::IntoVal;

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

    fn last_emitted_event_topics(env: &Env) -> Vec<soroban_sdk::Val> {
        let all = env.events().all();
        all.last()
            .map(|(_, topics, _)| topics.clone())
            .unwrap_or_default()
    }

    fn events_count(env: &Env) -> usize {
        env.events().all().len()
    }

    #[test]
    fn repay_rejects_zero_amount() {
        let env = Env::default();
        env.mock_all_auths();
        let user = Address::generate(&env);
        let before = events_count(&env);
        assert_eq!(
            repay_debt(&env, user, None, 0),
            Err(RepayError::InvalidAmount)
        );
        assert_eq!(events_count(&env), before);
    }

    #[test]
    fn repay_rejects_negative_amount() {
        let env = Env::default();
        env.mock_all_auths();
        let user = Address::generate(&env);
        let before = events_count(&env);
        assert_eq!(
            repay_debt(&env, user, None, -1),
            Err(RepayError::InvalidAmount)
        );
        assert_eq!(events_count(&env), before);
    }

    #[test]
    fn repay_rejects_oversized_principal_with_overflow() {
        let env = Env::default();
        env.mock_all_auths();
        let user = Address::generate(&env);
        // Bypass the open-ended range by writing a corrupted position
        // directly; the entrypoint still must refuse to compute interest on
        // values that would overflow i128.
        env.storage().persistent().set::<RepayDataKey, Position>(
            &RepayDataKey::Position(user.clone()),
            &Position {
                principal: i128::MAX,
                last_update: env.ledger().timestamp(),
            },
        );
        // Time must advance so compute_interest runs (elapsed=0 short-circuits).
        advance_time(&env, SECONDS_PER_YEAR);
        assert_eq!(repay_debt(&env, user, None, 1), Err(RepayError::Overflow));
    }

    #[test]
    fn repay_rejects_corrupted_negative_principal() {
        let env = Env::default();
        env.mock_all_auths();
        let user = Address::generate(&env);
        env.storage().persistent().set::<RepayDataKey, Position>(
            &RepayDataKey::Position(user.clone()),
            &Position {
                principal: -42,
                last_update: env.ledger().timestamp(),
            },
        );
        // Negative principal can only come from storage tampering; refuse
        // rather than emit a RepayEvent with negative fields.
        assert_eq!(repay_debt(&env, user, None, 1), Err(RepayError::Overflow));
        // The corrupted value is NOT silently rewritten.
        let stored = load_position(&env, &user);
        assert_eq!(stored.principal, -42);
    }

    #[test]
    fn repay_rejects_when_user_has_no_position() {
        let env = Env::default();
        env.mock_all_auths();
        let user = Address::generate(&env);
        assert_eq!(repay_debt(&env, user, None, 100), Err(RepayError::NoDebt));
    }

    #[test]
    fn repay_rejects_when_position_principal_is_zero() {
        let env = Env::default();
        env.mock_all_auths();
        let user = Address::generate(&env);
        seed_position(&env, &user, 0, env.ledger().timestamp());
        assert_eq!(repay_debt(&env, user, None, 100), Err(RepayError::NoDebt));
    }

    #[test]
    fn repay_full_principal_immediately_zeros_remaining() {
        let env = Env::default();
        env.mock_all_auths();
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
    }

    #[test]
    fn repay_partial_principal_immediately() {
        let env = Env::default();
        env.mock_all_auths();
        let user = Address::generate(&env);
        seed_position(&env, &user, 1_000, env.ledger().timestamp());

        let (remaining, interest, principal) =
            repay_debt(&env, user.clone(), None, 300).unwrap();
        assert_eq!(remaining, 700);
        assert_eq!(interest, 0);
        assert_eq!(principal, 300);

        let stored = load_position(&env, &user);
        assert_eq!(stored.principal, 700);
    }

    #[test]
    fn repay_one_year_of_interest_pays_interest_first() {
        let env = Env::default();
        env.mock_all_auths();
        let user = Address::generate(&env);
        seed_position(&env, &user, 1_000, env.ledger().timestamp());

        advance_time(&env, SECONDS_PER_YEAR);

        // 5% APR simple on 1_000 over a year → exactly 50 interest units.
        let (remaining, interest, principal) =
            repay_debt(&env, user.clone(), None, 30).unwrap();
        assert_eq!(interest, 30);
        assert_eq!(principal, 0);
        assert_eq!(remaining, 1_000 + 50 - 30);

        let stored = load_position(&env, &user);
        assert_eq!(stored.principal, 1_000); // principal untouched
    }

    #[test]
    fn repay_one_year_of_interest_with_overpayment_pays_interest_then_principal() {
        let env = Env::default();
        env.mock_all_auths();
        let user = Address::generate(&env);
        seed_position(&env, &user, 1_000, env.ledger().timestamp());

        advance_time(&env, SECONDS_PER_YEAR);

        // accrued = 1050, send 200 → 50 interest + 150 principal, remaining 850.
        let (remaining, interest, principal) =
            repay_debt(&env, user.clone(), None, 200).unwrap();
        assert_eq!(interest, 50);
        assert_eq!(principal, 150);
        assert_eq!(remaining, 850);
        assert_eq!(stored_principal(&env, &user), 850);
    }

    #[test]
    fn repay_overpay_caps_at_accrued_debt_creating_no_credit() {
        let env = Env::default();
        env.mock_all_auths();
        let user = Address::generate(&env);
        seed_position(&env, &user, 1_000, env.ledger().timestamp());

        advance_time(&env, SECONDS_PER_YEAR);
        // accrued = 1050. Send 5000 → should cap at 1050.
        let (remaining, interest, principal) =
            repay_debt(&env, user.clone(), None, 5_000).unwrap();
        assert_eq!(remaining, 0);
        assert_eq!(interest + principal, 1_050);
        assert_eq!(interest, 50);
        assert_eq!(principal, 1_000);
        assert_eq!(stored_principal(&env, &user), 0);
    }

    #[test]
    fn repay_emits_event_with_full_breakdown_and_topics() {
        let env = Env::default();
        env.mock_all_auths();
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

        // Exactly one event was published.
        assert_eq!(events_count(&env), before + 1);

        // Topics are user and asset in declaration order. The asset topic
        // is encoded from the `Option<Address>` field via `Some(...).into_val`,
        // not from the bare `Address::into_val` — those two encodings differ.
        let topics = last_emitted_event_topics(&env);
        assert_eq!(topics.len(), 2);
        assert_eq!(topics[0], user.into_val(&env));
        assert_eq!(topics[1], Some(asset.clone()).into_val(&env));
    }

    #[test]
    fn repay_no_interest_when_zero_elapsed() {
        let env = Env::default();
        env.mock_all_auths();
        let user = Address::generate(&env);
        seed_position(&env, &user, 1_000, env.ledger().timestamp());

        // No time advance → no accrued interest.
        let (remaining, interest, principal) =
            repay_debt(&env, user.clone(), None, 250).unwrap();
        assert_eq!(interest, 0);
        assert_eq!(principal, 250);
        assert_eq!(remaining, 750);
    }

    #[test]
    fn repay_consecutive_calls_reduce_debt_monotonically() {
        let env = Env::default();
        env.mock_all_auths();
        let user = Address::generate(&env);
        seed_position(&env, &user, 1_000, env.ledger().timestamp());

        advance_time(&env, SECONDS_PER_YEAR);

        // First repayment: 50 interest + 100 principal → stored=900.
        let (_, _, _) = repay_debt(&env, user.clone(), None, 150).unwrap();
        assert_eq!(stored_principal(&env, &user), 900);

        // Advance another year so the remaining 900 principal accrues new
        // interest. accrued_debt = 900 + 45 = 945.
        advance_time(&env, SECONDS_PER_YEAR);
        let (_, interest2, principal2) = repay_debt(&env, user.clone(), None, 250).unwrap();
        assert_eq!(interest2, 45);
        assert_eq!(principal2, 205);
        assert_eq!(stored_principal(&env, &user), 695);

        // Final payoff from 695 principal (zero elapsed, no interest) → stored=0.
        let (_, _, _) = repay_debt(&env, user.clone(), None, 695).unwrap();
        assert_eq!(stored_principal(&env, &user), 0);
    }

    fn stored_principal(env: &Env, user: &Address) -> i128 {
        load_position(env, user).principal
    }
}
