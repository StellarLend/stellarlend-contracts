//! Stateful invariant tests across the lending lifecycle.
//!
//! This module builds a bounded, property-based state-machine model of the
//! single-asset lending lifecycle — `deposit`, `withdraw`, `borrow`, `repay`,
//! `liquidate`, and interest accrual driven by ledger-time advances — and
//! replays generated operation sequences against a live contract instance,
//! asserting that the contract's on-chain state never diverges from the
//! reference model.
//!
//! # Design
//!
//! * **Generated actors/assets.** Each generated sequence draws [`NUM_USERS`]
//!   actors and a collateral/debt token pair; any actor may act as depositor,
//!   borrower, repayer, or liquidator.
//! * **Reference model.** [`Model`] mirrors the contract's persistent state
//!   exactly: per-user collateral and `DebtPosition`, the global borrow
//!   index, `TotalDeposits`/`TotalDebt`, the insurance fund, bad debt, the
//!   protocol reserve (`TotalDeposits` surplus), and every governable
//!   parameter the lifecycle ops consult. Settlement math reuses the
//!   contract's own pure `debt` functions, so the model is a faithful
//!   differential test of the *state machine* (keys written, ordering,
//!   success/failure branches) rather than a re-implementation of the math.
//! * **Accrual.** `AdvanceTime` steps move the ledger clock forward up to a
//!   year per step; subsequent borrow/repay/liquidate ops settle accrued
//!   interest through the global borrow index, and the views
//!   (`get_position`) report elapsed-time debt the model reproduces with the
//!   same `debt::effective_debt` formula.
//! * **Failure semantics.** Invalid operations must fail with the *typed*
//!   error the contract defines, never a host trap, and must not partially
//!   mutate the position the caller asked to change. The model snapshots its
//!   state before each mutating operation and restores that snapshot whenever
//!   the contract invocation returns an error, matching Soroban's atomic
//!   transaction rollback semantics.
//! * **Shrinking & determinism.** Sequences are generated with proptest
//!   strategies, so a failing case shrinks to a minimal sequence. A fixed
//!   ChaCha seed makes every CI run deterministic; the failure report includes
//!   the exact seed and minimized sequence, and failures replay with
//!   `STELLARLEND_STATE_SEED`.
//! * **CI runtime limits.** The suite is bounded by [`STATE_CASES`]
//!   sequences of at most [`MAX_OPS`] operations, with bounded amounts and
//!   bounded time steps. `STELLARLEND_STATE_CASES` and
//!   `STELLARLEND_STATE_SEED` let CI tune the budget or replay a failure
//!   without editing code:
//!
//!   ```text
//!   STELLARLEND_STATE_CASES=16 cargo test -p stellarlend-lending --lib stateful_lifecycle
//!   STELLARLEND_STATE_SEED=0x… cargo test -p stellarlend-lending --lib stateful_lifecycle
//!   ```
//!
//! # Invariants checked after every operation
//!
//! 1. **Debt** — every user's `get_position().debt` equals the model's
//!    effective debt; stored `DebtPosition` fields match the model exactly
//!    once written; protocol `total_borrow` matches the model's `TotalDebt`
//!    accumulator.
//! 2. **Collateral** — every user's collateral and protocol `total_supply`
//!    match the model; the reserve (`TotalDeposits` − Σ collateral) is never
//!    negative and grows only by liquidation seizures.
//! 3. **Reserve / insurance / bad debt** — the insurance fund and bad-debt
//!    accumulators match the model, and liquidation shortfalls draw insurance
//!    before being booked as bad debt.
//! 4. **Authorization** — user ops require the position owner's auth and
//!    admin ops require the admin's auth. These are covered by the
//!    deterministic auth tests in the lower half of this module, since the
//!    generated model uses `mock_all_auths` like the other property suites
//!    in this crate.
//! 5. **Global accounting** — the borrow index equals the model's index;
//!    protocol metrics (total borrow/supply, utilization) equal the model;
//!    every governable parameter reads back equal to the model.
//!
//! # Observed implementation behaviour the model pins
//!
//! * The borrow-rate model is never configured, so `current_borrow_rate`
//!   returns the constant [`debt::DEFAULT_APR_BPS`] (500 bps).
//! * `TotalDebt` is *not* a strict sum of position principals: `liquidate`
//!   repays principal without deducting it from `TotalDebt`, and a `repay`
//!   whose amount is smaller than the interest settled since the last touch
//!   clamps the deduction to zero (`prev_principal.checked_sub(...).unwrap_or(0)`).
//!   The model maintains its own `TotalDebt` accumulator using exactly those
//!   rules and asserts the contract's `total_borrow` matches it; it does not
//!   assert the (false) identity `TotalDebt == Σ principal`. See
//!   `STATE_LIFECYCLE_INVARIANT_TESTING.md`.
//! * On a successful `liquidate`, collateral seizure does **not** reduce
//!   `TotalDeposits`; that surplus is tracked as the model's reserve. Failed
//!   liquidations are fully rolled back with the rest of the transaction.

extern crate alloc;
extern crate std;

use super::*;
use crate::debt::{self, DebtPosition, INDEX_SCALE};
use crate::liquidate_transfer_test::{MockToken, MockTokenClient};
use alloc::format;
use alloc::vec::Vec;
use proptest::prelude::*;

use proptest::strategy::Strategy;
use proptest::test_runner::{Config, RngAlgorithm, TestCaseError, TestRng, TestRunner};
use soroban_sdk::testutils::{Address as _, Ledger, MockAuth, MockAuthInvoke};
use soroban_sdk::IntoVal;

// ---------------------------------------------------------------------------
// Sizing / determinism knobs
// ---------------------------------------------------------------------------

/// Fixed ChaCha seed so every CI run replays the same generated sequences.
const STATE_SEED: [u8; 32] = [
    0x73, 0x74, 0x61, 0x74, 0x65, 0x66, 0x75, 0x6c, 0x2d, 0x6c, 0x69, 0x66, 0x65, 0x63, 0x79, 0x63,
    0x6c, 0x65, 0x2d, 0x73, 0x65, 0x65, 0x64, 0x2d, 0x30, 0x30, 0x31, 0x2d, 0x61, 0x62, 0x63, 0x64,
];

/// Number of generated sequences per test run. Bounded so the suite finishes
/// comfortably inside CI's `build-and-test` job; override with
/// `STELLARLEND_STATE_CASES` (e.g. `16` for a fast smoke run).
const STATE_CASES: u32 = 64;

/// Maximum number of lifecycle operations in a single generated sequence.
const MAX_OPS: usize = 48;

/// Cap on shrink iterations — kept modest so CI time stays bounded while
/// still producing minimal failing sequences.
const MAX_SHRINK_ITERS: u32 = 4096;

/// Number of generated actors per sequence.
const NUM_USERS: usize = 4;

/// Starting ledger timestamp (seconds since the Unix epoch).
const START_TS: u64 = 1_700_000_000;

/// Borrow APR the model assumes. The contract's `current_borrow_rate`
/// returns `debt::DEFAULT_APR_BPS` (500 bps) whenever no `RateParams` have
/// been configured; this suite never configures rate params, so the rate is
/// constant and the model is fully deterministic.
const BORROW_RATE_BPS: i128 = debt::DEFAULT_APR_BPS;

/// Basis-point denominator (10_000), matching the contract's `BPS_DENOM`.
const BPS_DENOM: i128 = 10_000;

/// `HEALTH_FACTOR_NO_DEBT` sentinel reported by `get_position` when a user
/// has no debt (private const in lib.rs; mirrored here for the view check).
const HEALTH_FACTOR_NO_DEBT: i128 = 100_000_000;

// ---------------------------------------------------------------------------
// Generated operations
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
enum Operation {
    Deposit(usize, u32),
    Withdraw(usize, u32),
    Borrow(usize, u32),
    Repay(usize, u32),
    Liquidate(usize, usize, u32),
    AdvanceTime(u32),
    SetMinBorrow(i128),
    SetDepositCap(i128),
    SetDebtCeiling(i128),
    SetCloseFactor(i128),
    SetIncentive(i128),
    SetThreshold(i128),
    SetInsuranceShare(i128),
}

fn operation_strategy() -> impl Strategy<Value = Operation> {
    let deposit = (0usize..NUM_USERS, 1u32..=1_000).prop_map(|(u, a)| Operation::Deposit(u, a));
    let withdraw = (0usize..NUM_USERS, 1u32..=1_000).prop_map(|(u, a)| Operation::Withdraw(u, a));
    let borrow = (0usize..NUM_USERS, 1u32..=1_000).prop_map(|(u, a)| Operation::Borrow(u, a));
    let repay = (0usize..NUM_USERS, 1u32..=1_000).prop_map(|(u, a)| Operation::Repay(u, a));
    let liquidate = (0usize..NUM_USERS, 0usize..NUM_USERS, 1u32..=1_000)
        .prop_map(|(l, b, a)| Operation::Liquidate(l, b, a));
    let short_time = (0u32..=86_400).prop_map(Operation::AdvanceTime);
    let long_time = (86_400u32..=31_536_000).prop_map(Operation::AdvanceTime);
    let min_borrow =
        prop::sample::select(&[0i128, 1, 50, 250]).prop_map(|v| Operation::SetMinBorrow(v));
    let deposit_cap =
        prop::sample::select(&[500i128, 5_000, 50_000]).prop_map(|v| Operation::SetDepositCap(v));
    let debt_ceiling =
        prop::sample::select(&[10_000i128, 100_000]).prop_map(|v| Operation::SetDebtCeiling(v));
    let close_factor =
        prop::sample::select(&[2_500i128, 5_000, 7_500]).prop_map(|v| Operation::SetCloseFactor(v));
    let incentive =
        prop::sample::select(&[0i128, 1_000, 2_000]).prop_map(|v| Operation::SetIncentive(v));
    let threshold =
        prop::sample::select(&[5_000i128, 8_000, 10_000]).prop_map(|v| Operation::SetThreshold(v));
    let insurance =
        prop::sample::select(&[0i128, 1_000, 5_000]).prop_map(|v| Operation::SetInsuranceShare(v));
    prop::strategy::Union::new_weighted(alloc::vec![
        (5, deposit.boxed()),
        (5, withdraw.boxed()),
        (5, borrow.boxed()),
        (5, repay.boxed()),
        (4, liquidate.boxed()),
        (3, short_time.boxed()),
        (2, long_time.boxed()),
        (1, min_borrow.boxed()),
        (1, deposit_cap.boxed()),
        (1, debt_ceiling.boxed()),
        (1, close_factor.boxed()),
        (1, incentive.boxed()),
        (1, threshold.boxed()),
        (1, insurance.boxed()),
    ])
}

fn operation_sequence_strategy() -> impl Strategy<Value = Vec<Operation>> {
    prop::collection::vec(operation_strategy(), 1..=MAX_OPS)
}

// ---------------------------------------------------------------------------
// Reference model
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
struct ModelUser {
    collateral: i128,
    position: DebtPosition,
    /// Whether the contract has an on-chain `DataKey::Debt` entry for this
    /// user. Before the first borrow/repay/liquidation touch there is no
    /// entry, and `load_debt` fabricates a default whose `last_update` is the
    /// *read-time* timestamp — so the stored position can only be compared
    /// field-for-field once a write has happened.
    has_stored_debt: bool,
}

/// Reference state machine for the single-asset lending lifecycle.
#[derive(Clone, Debug)]
struct Model {
    users: Vec<ModelUser>,
    /// Current ledger timestamp (synced to the test env on `AdvanceTime`).
    now: u64,
    /// Global borrow index (mirrors `DataKey::BorrowIndex`).
    index: i128,
    /// Timestamp of the last index touch (mirrors `DataKey::LastIndexUpdate`).
    last_index_update: u64,
    /// Mirrors `DataKey::TotalDeposits`.
    total_deposits: i128,
    /// Mirrors `DataKey::TotalDebt`, replicated with the contract's exact
    /// update rules (see module docs for the known divergences from the sum
    /// of position principals).
    total_debt: i128,
    /// Σ of all user collateral balances.
    sum_collateral: i128,
    /// Mirrors `DataKey::BadDebt`.
    bad_debt: i128,
    /// Mirrors `DataKey::InsuranceFund`.
    insurance_fund: i128,
    /// Mirrors `DataKey::InsuranceShareBps` (interest share routed to the fund).
    insurance_share_bps: i128,
    deposit_cap: i128,
    debt_ceiling: Option<i128>,
    min_borrow: i128,
    close_factor_bps: i128,
    incentive_bps: i128,
    threshold_bps: i128,
    /// `TotalDeposits` surplus over Σ collateral — the protocol reserve.
    /// Grows only through liquidation seizures (governed write-offs are out
    /// of scope for this suite).
    reserve: i128,
}

impl Model {
    fn new(num_users: usize, start_ts: u64) -> Self {
        Self {
            users: (0..num_users)
                .map(|_| ModelUser {
                    collateral: 0,
                    position: DebtPosition {
                        principal: 0,
                        borrow_index_snapshot: INDEX_SCALE,
                        last_update: start_ts,
                    },
                    has_stored_debt: false,
                })
                .collect(),
            now: start_ts,
            index: INDEX_SCALE,
            last_index_update: start_ts,
            total_deposits: 0,
            total_debt: 0,
            sum_collateral: 0,
            bad_debt: 0,
            insurance_fund: 0,
            // setup_case() calls set_insurance_share(1000): 10% of settled
            // interest is routed into the insurance fund.
            insurance_share_bps: 1_000,
            // DEFAULT_DEPOSIT_CAP from lib.rs (private const).
            deposit_cap: 1_000_000_000_000,
            debt_ceiling: None,
            min_borrow: 0,
            close_factor_bps: DEFAULT_CLOSE_FACTOR_BPS,
            incentive_bps: DEFAULT_LIQUIDATION_INCENTIVE_BPS,
            threshold_bps: LIQUIDATION_THRESHOLD_BPS,
            reserve: 0,
        }
    }

    /// Advance the global borrow index to `now`, exactly like
    /// `debt::touch_borrow_index` runs on every mutating operation.
    fn touch(&mut self, now: u64) {
        let elapsed = now.saturating_sub(self.last_index_update);
        self.index = debt::accrue_index(self.index, elapsed, BORROW_RATE_BPS);
        self.last_index_update = now;
    }

    /// Settle `user`'s position to the current index and credit the insurance
    /// fund with the configured share of the interest, mirroring
    /// `settle_and_accrue_insurance` on a successful mutating operation.
    fn settle_user(&mut self, user: usize) -> DebtPosition {
        let prev = &self.users[user].position;
        let settled = debt::settle_position(prev, self.index, self.now)
            .expect("bounded model inputs cannot overflow settle_position");
        let interest = settled.principal.saturating_sub(prev.principal);
        if interest > 0 && self.insurance_share_bps > 0 {
            let share = interest * self.insurance_share_bps / BPS_DENOM;
            self.insurance_fund += share;
        }
        settled
    }

    /// Record a position write with the current index snapshot and timestamp,
    /// mirroring `save_debt` after a successful borrow/repay or any
    /// `liquidate` attempt.
    fn set_position(&mut self, user: usize, principal: i128) {
        self.users[user].position = DebtPosition {
            principal,
            borrow_index_snapshot: self.index,
            last_update: self.now,
        };
        self.users[user].has_stored_debt = true;
    }

    fn apply(
        &mut self,
        op: &Operation,
        client: &LendingContractClient<'static>,
        env: &Env,
        users: &[Address],
        debt_asset: &Address,
        collateral_asset: &Address,
    ) -> Result<(), TestCaseError> {
        match op {
            Operation::Deposit(user, amount) => {
                self.apply_deposit(*user, *amount as i128, client, users)
            }
            Operation::Withdraw(user, amount) => {
                self.apply_withdraw(*user, *amount as i128, client, users)
            }
            Operation::Borrow(user, amount) => {
                self.apply_borrow(*user, *amount as i128, client, users)
            }
            Operation::Repay(user, amount) => {
                self.apply_repay(*user, *amount as i128, client, users)
            }
            Operation::Liquidate(liquidator, borrower, amount) => self.apply_liquidate(
                *liquidator,
                *borrower,
                *amount as i128,
                client,
                users,
                debt_asset,
                collateral_asset,
            ),
            Operation::AdvanceTime(secs) => {
                self.now = self.now.saturating_add(*secs as u64);
                env.ledger().set_timestamp(self.now);
                Ok(())
            }
            Operation::SetMinBorrow(v) => {
                let res = client.try_set_min_borrow(v);
                prop_assert!(res.is_ok(), "set_min_borrow({v}) must succeed, got {res:?}");
                self.min_borrow = *v;
                prop_assert_eq!(client.get_min_borrow(), *v);
                Ok(())
            }
            Operation::SetDepositCap(v) => {
                let res = client.try_set_deposit_cap(v);
                prop_assert!(
                    res.is_ok(),
                    "set_deposit_cap({v}) must succeed, got {res:?}"
                );
                self.deposit_cap = *v;
                prop_assert_eq!(client.get_deposit_cap(), *v);
                Ok(())
            }
            Operation::SetDebtCeiling(v) => {
                let res = client.try_set_debt_ceiling(v);
                prop_assert!(
                    res.is_ok(),
                    "set_debt_ceiling({v}) must succeed, got {res:?}"
                );
                self.debt_ceiling = Some(*v);
                Ok(())
            }
            Operation::SetCloseFactor(v) => {
                let res = client.try_set_close_factor_bps(v);
                prop_assert!(
                    res.is_ok(),
                    "set_close_factor_bps({v}) must succeed, got {res:?}"
                );
                self.close_factor_bps = *v;
                prop_assert_eq!(client.get_close_factor_bps(), *v);
                Ok(())
            }
            Operation::SetIncentive(v) => {
                let res = client.try_set_liquidation_incentive_bps(v);
                prop_assert!(
                    res.is_ok(),
                    "set_liquidation_incentive_bps({v}) must succeed, got {res:?}"
                );
                self.incentive_bps = *v;
                prop_assert_eq!(client.get_liquidation_incentive_bps(), *v);
                Ok(())
            }
            Operation::SetThreshold(v) => {
                let res = client.try_set_liquidation_threshold_bps(v);
                prop_assert!(
                    res.is_ok(),
                    "set_liquidation_threshold_bps({v}) must succeed, got {res:?}"
                );
                self.threshold_bps = *v;
                prop_assert_eq!(client.get_liquidation_threshold_bps(), *v);
                Ok(())
            }
            Operation::SetInsuranceShare(v) => {
                let res = client.try_set_insurance_share(v);
                prop_assert!(
                    res.is_ok(),
                    "set_insurance_share({v}) must succeed, got {res:?}"
                );
                self.insurance_share_bps = *v;
                prop_assert_eq!(client.get_insurance_share(), *v);
                Ok(())
            }
        }
    }

    fn apply_deposit(
        &mut self,
        user: usize,
        amount: i128,
        client: &LendingContractClient<'static>,
        users: &[Address],
    ) -> Result<(), TestCaseError> {
        let new_total = self
            .total_deposits
            .checked_add(amount)
            .expect("bounded model inputs cannot overflow total deposits");
        let res = client.try_deposit(&users[user], &amount);
        // The deposit-cap gate runs before any write, so a rejection must not
        // mutate anything.
        if new_total > self.deposit_cap {
            prop_assert!(
                matches!(res, Err(Ok(LendingError::DepositCapExceeded))),
                "deposit over cap must fail with DepositCapExceeded, got {res:?}"
            );
            return Ok(());
        }
        let new_balance = match res {
            Ok(Ok(b)) => b,
            Ok(Err(conv)) => {
                return Err(TestCaseError::fail(format!(
                    "deposit return-value conversion error: {conv:?}"
                )))
            }
            Err(Ok(err)) => {
                return Err(TestCaseError::fail(format!(
                    "deposit unexpectedly rejected with {err:?}"
                )))
            }
            Err(Err(invoke)) => {
                return Err(TestCaseError::fail(format!("deposit trapped: {invoke:?}")))
            }
        };
        self.users[user].collateral += amount;
        self.sum_collateral += amount;
        self.total_deposits = new_total;
        prop_assert_eq!(new_balance, self.users[user].collateral);
        Ok(())
    }

    fn apply_withdraw(
        &mut self,
        user: usize,
        amount: i128,
        client: &LendingContractClient<'static>,
        users: &[Address],
    ) -> Result<(), TestCaseError> {
        let res = client.try_withdraw(&users[user], &amount);
        // Over-withdrawal is rejected before any write.
        if amount > self.users[user].collateral {
            prop_assert!(
                matches!(res, Err(Ok(LendingError::InvalidAmount))),
                "over-withdrawal must fail with InvalidAmount, got {res:?}"
            );
            return Ok(());
        }
        let new_balance = match res {
            Ok(Ok(b)) => b,
            Ok(Err(conv)) => {
                return Err(TestCaseError::fail(format!(
                    "withdraw return-value conversion error: {conv:?}"
                )))
            }
            Err(Ok(err)) => {
                return Err(TestCaseError::fail(format!(
                    "withdraw unexpectedly rejected with {err:?}"
                )))
            }
            Err(Err(invoke)) => {
                return Err(TestCaseError::fail(format!("withdraw trapped: {invoke:?}")))
            }
        };
        self.users[user].collateral -= amount;
        self.sum_collateral -= amount;
        self.total_deposits -= amount;
        prop_assert_eq!(new_balance, self.users[user].collateral);
        Ok(())
    }

    fn apply_borrow(
        &mut self,
        user: usize,
        amount: i128,
        client: &LendingContractClient<'static>,
        users: &[Address],
    ) -> Result<(), TestCaseError> {
        let before = self.clone();

        // The minimum-borrow gate runs before interest settlement, so it must
        // not touch the borrow index or insurance fund.
        if amount < self.min_borrow {
            let res = client.try_borrow(&users[user], &amount);
            prop_assert!(
                matches!(res, Err(Ok(LendingError::BelowMinimumBorrow))),
                "sub-minimum borrow must fail with BelowMinimumBorrow, got {res:?}"
            );
            return Ok(());
        }

        // Interest settlement (index advance + insurance share) happens even
        // when the borrow is later rejected for solvency or the debt ceiling.
        let prev_principal = self.users[user].position.principal;
        self.touch(self.now);
        let settled = self.settle_user(user);
        let new_principal = settled.principal + amount;

        // Solvency gate mirrors `assert_borrow_solvent`: the *constant*
        // LIQUIDATION_THRESHOLD_BPS (8000), not the governable threshold, and
        // effective debt equals principal because last_update == now.
        let weighted_collateral = self.users[user].collateral * LIQUIDATION_THRESHOLD_BPS;
        let required_collateral = HEALTH_FACTOR_SCALE * new_principal;
        let insolvent = weighted_collateral < required_collateral;

        let new_total_debt = self.total_debt + (new_principal - prev_principal);
        let ceiling_exceeded = self.debt_ceiling.is_some_and(|c| new_total_debt > c);

        let res = client.try_borrow(&users[user], &amount);
        if insolvent {
            prop_assert!(
                matches!(res, Err(Ok(LendingError::InsufficientCollateral))),
                "insolvent borrow must fail with InsufficientCollateral, got {res:?}"
            );
            *self = before;
            return Ok(());
        }
        if ceiling_exceeded {
            prop_assert!(
                matches!(res, Err(Ok(LendingError::DebtCeilingExceeded))),
                "ceiling-exceeding borrow must fail with DebtCeilingExceeded, got {res:?}"
            );
            *self = before;
            return Ok(());
        }
        let new_balance = match res {
            Ok(Ok(b)) => b,
            Ok(Err(conv)) => {
                return Err(TestCaseError::fail(format!(
                    "borrow return-value conversion error: {conv:?}"
                )))
            }
            Err(Ok(err)) => {
                return Err(TestCaseError::fail(format!(
                    "borrow unexpectedly rejected with {err:?}"
                )))
            }
            Err(Err(invoke)) => {
                return Err(TestCaseError::fail(format!("borrow trapped: {invoke:?}")))
            }
        };
        self.set_position(user, new_principal);
        self.total_debt = new_total_debt;
        prop_assert_eq!(new_balance, new_principal);
        Ok(())
    }

    fn apply_repay(
        &mut self,
        user: usize,
        amount: i128,
        client: &LendingContractClient<'static>,
        users: &[Address],
    ) -> Result<(), TestCaseError> {
        let before = self.clone();
        let prev_principal = self.users[user].position.principal;
        self.touch(self.now);
        let settled = self.settle_user(user);
        let res = client.try_repay(&users[user], &amount);
        // Over-repayment is rejected after settlement (index/fund advance)
        // but before the position is written back.
        if amount > settled.principal {
            prop_assert!(
                matches!(res, Err(Ok(LendingError::RepayAmountTooHigh))),
                "over-repayment must fail with RepayAmountTooHigh, got {res:?}"
            );
            *self = before;
            return Ok(());
        }
        let new_balance = match res {
            Ok(Ok(b)) => b,
            Ok(Err(conv)) => {
                return Err(TestCaseError::fail(format!(
                    "repay return-value conversion error: {conv:?}"
                )))
            }
            Err(Ok(err)) => {
                return Err(TestCaseError::fail(format!(
                    "repay unexpectedly rejected with {err:?}"
                )))
            }
            Err(Err(invoke)) => {
                return Err(TestCaseError::fail(format!("repay trapped: {invoke:?}")))
            }
        };
        let updated_principal = settled.principal - amount;
        self.set_position(user, updated_principal);
        // Mirrors the contract's `repaid` computation: the deduction is the
        // pre-settle principal minus the updated principal, clamped to zero,
        // then `total_debt.saturating_sub(repaid)`.
        let repaid = (prev_principal - updated_principal).max(0);
        self.total_debt = self.total_debt.saturating_sub(repaid);
        prop_assert_eq!(new_balance, updated_principal);
        Ok(())
    }

    fn apply_liquidate(
        &mut self,
        liquidator: usize,
        borrower: usize,
        amount: i128,
        client: &LendingContractClient<'static>,
        users: &[Address],
        debt_asset: &Address,
        collateral_asset: &Address,
    ) -> Result<(), TestCaseError> {
        let before = self.clone();
        let res = client.try_liquidate(
            &users[liquidator],
            &users[borrower],
            debt_asset,
            collateral_asset,
            &amount,
        );

        // Self-liquidation is rejected before any state is touched.
        if liquidator == borrower {
            prop_assert!(
                matches!(res, Err(Ok(LendingError::SelfLiquidation))),
                "self-liquidation must fail with SelfLiquidation, got {res:?}"
            );
            return Ok(());
        }

        // Everything below mirrors `liquidate`: the borrower's position is
        // settled (and saved) even when the liquidation is later rejected.
        let collateral = self.users[borrower].collateral;
        self.touch(self.now);
        let settled = self.settle_user(borrower);
        let debt = settled.principal;
        self.set_position(borrower, settled.principal);

        if debt <= 0 {
            prop_assert!(
                matches!(res, Err(Ok(LendingError::PositionHealthy))),
                "zero-debt liquidation must fail with PositionHealthy, got {res:?}"
            );
            *self = before;
            return Ok(());
        }

        // Health check uses the governable threshold (floor division, all
        // values positive so plain `/` matches `checked_mul_div_floor`).
        let hf = collateral * self.threshold_bps / debt;
        if hf >= HEALTH_FACTOR_SCALE {
            prop_assert!(
                matches!(res, Err(Ok(LendingError::PositionHealthy))),
                "healthy liquidation must fail with PositionHealthy, got {res:?}"
            );
            *self = before;
            return Ok(());
        }

        let max_repay = debt * self.close_factor_bps / BPS_DENOM;
        let actual_repay = amount.min(max_repay);
        if actual_repay <= 0 {
            prop_assert!(
                matches!(res, Err(Ok(LendingError::InvalidAmount))),
                "non-positive liquidation amount must fail with InvalidAmount, got {res:?}"
            );
            *self = before;
            return Ok(());
        }

        let seized = actual_repay * (BPS_DENOM + self.incentive_bps) / BPS_DENOM;
        let (final_seized, shortfall) = if seized > collateral {
            (collateral, seized - collateral)
        } else {
            (seized, 0)
        };
        let insurance_drawn = shortfall.min(self.insurance_fund);
        let residual = shortfall - insurance_drawn;

        let repaid = match res {
            Ok(Ok(v)) => v,
            Ok(Err(conv)) => {
                return Err(TestCaseError::fail(format!(
                    "liquidate return-value conversion error: {conv:?}"
                )))
            }
            Err(Ok(err)) => {
                return Err(TestCaseError::fail(format!(
                    "liquidate unexpectedly rejected with {err:?}"
                )))
            }
            Err(Err(invoke)) => {
                return Err(TestCaseError::fail(format!(
                    "liquidate trapped: {invoke:?}"
                )))
            }
        };
        prop_assert_eq!(repaid, actual_repay, "liquidate repaid amount");

        // Seized collateral leaves the books while TotalDeposits stays put →
        // the reserve grows; TotalDebt is not updated by liquidate.
        self.users[borrower].collateral -= final_seized;
        self.sum_collateral -= final_seized;
        self.reserve += final_seized;
        self.insurance_fund -= insurance_drawn;
        self.bad_debt += residual;
        self.set_position(borrower, debt - actual_repay);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Invariant checks (run after every operation)
// ---------------------------------------------------------------------------

fn check_invariants(
    model: &Model,
    client: &LendingContractClient<'static>,
    env: &Env,
    users: &[Address],
) -> Result<(), TestCaseError> {
    for (i, user) in users.iter().enumerate() {
        let mu = &model.users[i];
        let pos = client.get_position(user);

        prop_assert!(
            pos.collateral >= 0,
            "user {i}: negative collateral {}",
            pos.collateral
        );
        prop_assert!(pos.debt >= 0, "user {i}: negative debt {}", pos.debt);
        prop_assert_eq!(pos.collateral, mu.collateral, "user {} collateral", i);
        let expected_debt = debt::effective_debt(&mu.position, model.now, BORROW_RATE_BPS)
            .unwrap_or(mu.position.principal)
            .max(0);
        prop_assert_eq!(pos.debt, expected_debt, "user {} debt", i);

        let stored = client.get_debt_position(user);
        if mu.has_stored_debt {
            prop_assert_eq!(
                stored.principal,
                mu.position.principal,
                "user {} stored principal",
                i
            );
            prop_assert_eq!(
                stored.borrow_index_snapshot,
                mu.position.borrow_index_snapshot,
                "user {} stored index snapshot",
                i
            );
            prop_assert_eq!(
                stored.last_update,
                mu.position.last_update,
                "user {} stored last_update",
                i
            );
        } else {
            prop_assert_eq!(stored.principal, 0, "user {} default principal", i);
            prop_assert_eq!(
                stored.borrow_index_snapshot,
                INDEX_SCALE,
                "user {} default snapshot",
                i
            );
            prop_assert_eq!(
                stored.last_update,
                env.ledger().timestamp(),
                "user {} default last_update equals read time",
                i
            );
        }

        let expected_hf = if expected_debt > 0 {
            mu.collateral * model.threshold_bps / expected_debt
        } else {
            HEALTH_FACTOR_NO_DEBT
        };
        prop_assert_eq!(pos.health_factor, expected_hf, "user {} health factor", i);
    }

    let metrics = client.get_protocol_metrics();
    prop_assert_eq!(
        metrics.total_borrow,
        model.total_debt,
        "protocol total debt"
    );
    prop_assert_eq!(
        metrics.total_supply,
        model.total_deposits,
        "protocol total deposits"
    );
    let expected_util = if model.total_deposits > 0 {
        model.total_debt.saturating_mul(BPS_DENOM) / model.total_deposits
    } else {
        0
    };
    prop_assert_eq!(
        metrics.utilization_bps,
        expected_util,
        "protocol utilization"
    );

    prop_assert_eq!(
        client.get_borrow_index(),
        model.index,
        "global borrow index"
    );
    prop_assert_eq!(client.get_bad_debt(), model.bad_debt, "bad debt");
    prop_assert_eq!(
        client.get_insurance_fund(),
        model.insurance_fund,
        "insurance fund"
    );

    prop_assert_eq!(
        model.total_deposits - model.sum_collateral,
        model.reserve,
        "TotalDeposits surplus equals accumulated liquidation seizures"
    );
    prop_assert!(
        model.reserve >= 0,
        "protocol reserve must never go negative"
    );
    prop_assert!(
        model.total_deposits >= 0
            && model.total_debt >= 0
            && model.bad_debt >= 0
            && model.insurance_fund >= 0,
        "protocol accumulators must never go negative"
    );

    prop_assert_eq!(client.get_min_borrow(), model.min_borrow);
    prop_assert_eq!(client.get_deposit_cap(), model.deposit_cap);
    prop_assert_eq!(client.get_close_factor_bps(), model.close_factor_bps);
    prop_assert_eq!(client.get_liquidation_incentive_bps(), model.incentive_bps);
    prop_assert_eq!(client.get_liquidation_threshold_bps(), model.threshold_bps);
    prop_assert_eq!(client.get_insurance_share(), model.insurance_share_bps);
    Ok(())
}

// ---------------------------------------------------------------------------
// Seed / case-count overrides for reproducibility and CI tuning
// ---------------------------------------------------------------------------

fn state_cases() -> u32 {
    std::env::var("STELLARLEND_STATE_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(STATE_CASES)
}

fn state_seed() -> [u8; 32] {
    std::env::var("STELLARLEND_STATE_SEED")
        .ok()
        .and_then(|hex| parse_hex_seed(&hex))
        .unwrap_or(STATE_SEED)
}

fn parse_hex_seed(s: &str) -> Option<[u8; 32]> {
    let s = s.strip_prefix("0x").unwrap_or(s);
    if s.len() != 64 || !s.is_ascii() {
        return None;
    }
    let mut out = [0u8; 32];
    for i in 0..32 {
        out[i] = u8::from_str_radix(&s[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(out)
}

fn format_seed(seed: &[u8; 32]) -> alloc::string::String {
    let mut output = alloc::string::String::from("0x");
    for byte in seed {
        output.push_str(&format!("{byte:02x}"));
    }
    output
}

// ---------------------------------------------------------------------------
// Environment setup
// ---------------------------------------------------------------------------

fn setup_case() -> (
    Env,
    LendingContractClient<'static>,
    Vec<Address>,
    Address,
    Address,
) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START_TS);
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let users: Vec<Address> = (0..NUM_USERS).map(|_| Address::generate(&env)).collect();
    let debt_asset = env.register(MockToken, ());
    let collateral_asset = env.register(MockToken, ());

    // Fund every actor with debt tokens (any user may act as liquidator) and
    // the contract with collateral tokens, far beyond what a bounded sequence
    // can exhaust, so liquidation transfers never fail on balances.
    for user in &users {
        MockTokenClient::new(&env, &debt_asset).mint(user, &1_000_000_000);
    }
    MockTokenClient::new(&env, &collateral_asset).mint(&contract_id, &1_000_000_000);

    client.initialize(&admin);
    // Route 10% of settled interest into the insurance fund so the
    // liquidation shortfall path has a funded backstop to draw from.
    client.set_insurance_share(&1_000);
    (env, client, users, debt_asset, collateral_asset)
}

// ---------------------------------------------------------------------------
// The generated stateful suite
// ---------------------------------------------------------------------------

#[test]
fn stateful_lifecycle_invariants_hold_across_generated_sequences() {
    let cases = state_cases();
    let seed = state_seed();

    let mut runner = TestRunner::new_with_rng(
        Config {
            cases,
            max_shrink_iters: MAX_SHRINK_ITERS,
            ..Config::default()
        },
        TestRng::from_seed(RngAlgorithm::ChaCha, &seed),
    );

    let strategy = operation_sequence_strategy();
    let result = runner.run(&strategy, |ops| {
        let (env, client, users, debt_asset, collateral_asset) = setup_case();
        let mut model = Model::new(NUM_USERS, START_TS);
        for op in ops {
            model.apply(&op, &client, &env, &users, &debt_asset, &collateral_asset)?;
            check_invariants(&model, &client, &env, &users)?;
        }
        Ok(())
    });

    if let Err(error) = result {
        panic!(
            "stateful lifecycle failure; replay with STELLARLEND_STATE_SEED={}; proptest minimized input follows: {}",
            format_seed(&seed),
            error
        );
    }
}

// ---------------------------------------------------------------------------
// Authorization invariants (real auth, no mock_all_auths)
// ---------------------------------------------------------------------------

fn setup_real_auth() -> (
    Env,
    LendingContractClient<'static>,
    Address,
    Address,
    Address,
) {
    let env = Env::default();
    let contract_id = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &contract_id);
    let admin = Address::generate(&env);
    let alice = Address::generate(&env);
    let mallory = Address::generate(&env);

    // Only the intended admin signs `initialize`.
    env.mock_auths(&[MockAuth {
        address: &admin,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "initialize",
            args: (admin.clone(),).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.initialize(&admin);
    (env, client, contract_id, alice, mallory)
}

/// A user op without the position owner's auth must be rejected at the host
/// level with zero state change.
#[test]
fn deposit_without_user_auth_is_rejected_without_mutation() {
    let (_env, client, _contract_id, alice, _mallory) = setup_real_auth();
    // No auth is mocked for alice, so `alice.require_auth()` inside deposit()
    // fails at the host level.
    let res = client.try_deposit(&alice, &100);
    assert!(
        res.is_err(),
        "deposit without the depositor's auth must be rejected"
    );
    let pos = client.get_position(&alice);
    assert_eq!(pos.collateral, 0);
    assert_eq!(pos.debt, 0);
    assert_eq!(client.get_protocol_metrics().total_supply, 0);
    assert_eq!(client.get_protocol_metrics().total_borrow, 0);
}

/// A user may only withdraw their own collateral; an unrelated signer cannot
/// touch another user's position.
#[test]
fn withdraw_for_another_user_requires_owner_auth() {
    let (env, client, contract_id, alice, mallory) = setup_real_auth();

    // Alice deposits with her own signature.
    env.mock_auths(&[MockAuth {
        address: &alice,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "deposit",
            args: (alice.clone(), 100i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    client.deposit(&alice, &100);

    // Mallory tries to withdraw Alice's collateral; only Mallory signs, so
    // `alice.require_auth()` inside withdraw() fails.
    env.mock_auths(&[MockAuth {
        address: &mallory,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "withdraw",
            args: (alice.clone(), 50i128).into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let res = client.try_withdraw(&alice, &50);
    assert!(
        res.is_err(),
        "withdrawing another user's collateral must require that user's auth"
    );
    assert_eq!(client.get_position(&alice).collateral, 100);
    assert_eq!(client.get_protocol_metrics().total_supply, 100);
}

/// Admin-only configuration changes must be rejected with the typed
/// `Unauthorized` error and must not write anything.
#[test]
fn non_admin_config_change_is_rejected_with_unauthorized() {
    let (env, client, contract_id, alice, _mallory) = setup_real_auth();
    let asset = Address::generate(&env);

    // Alice signs a `set_asset_params` call naming *herself* as admin. Her
    // own auth passes, then the contract compares the argument to the stored
    // admin and must return a typed Unauthorized.
    let params: (Address, Address, i128, i128, i128, i128, i128) = (
        alice.clone(),
        asset.clone(),
        5_000i128,
        6_000i128,
        0i128,
        0i128,
        0i128,
    );
    env.mock_auths(&[MockAuth {
        address: &alice,
        invoke: &MockAuthInvoke {
            contract: &contract_id,
            fn_name: "set_asset_params",
            args: params.into_val(&env),
            sub_invokes: &[],
        },
    }]);
    let res = client.try_set_asset_params(&alice, &asset, &5_000, &6_000, &0, &0, &0);
    assert_eq!(res, Err(Ok(LendingError::Unauthorized)));
    assert!(
        client.get_asset_params(&asset).is_none(),
        "no asset params may be written by a non-admin"
    );
}

/// Self-liquidation is rejected before any state change.
#[test]
fn self_liquidation_is_rejected_without_mutation() {
    let (_env, client, users, debt_asset, collateral_asset) = setup_case();
    client.deposit(&users[0], &100);
    let res = client.try_borrow(&users[0], &50);
    assert!(res.is_ok(), "borrow against 100 collateral must succeed");
    let before = client.get_position(&users[0]);

    let res = client.try_liquidate(&users[0], &users[0], &debt_asset, &collateral_asset, &25);
    assert!(matches!(res, Err(Ok(LendingError::SelfLiquidation))));

    let after = client.get_position(&users[0]);
    assert_eq!(after.collateral, before.collateral);
    assert_eq!(after.debt, before.debt);
    assert_eq!(client.get_protocol_metrics().total_borrow, 50);
}

/// A compact deterministic companion to the generated suite: each invalid
/// operation fails with the documented typed error and leaves every
/// observable accumulator untouched.
#[test]
fn invalid_operations_fail_without_partial_mutation() {
    let (_env, client, users, _debt_asset, _collateral_asset) = setup_case();

    // Withdraw with no collateral: typed InvalidAmount, nothing changes.
    let res = client.try_withdraw(&users[0], &10);
    assert!(matches!(res, Err(Ok(LendingError::InvalidAmount))));
    assert_eq!(client.get_protocol_metrics().total_supply, 0);

    // Repay with no debt: typed RepayAmountTooHigh, nothing changes.
    let res = client.try_repay(&users[0], &10);
    assert!(matches!(res, Err(Ok(LendingError::RepayAmountTooHigh))));
    assert_eq!(client.get_protocol_metrics().total_borrow, 0);

    // Borrow with no collateral: typed InsufficientCollateral, nothing changes.
    let res = client.try_borrow(&users[0], &10);
    assert!(matches!(res, Err(Ok(LendingError::InsufficientCollateral))));
    assert_eq!(client.get_protocol_metrics().total_borrow, 0);

    // Deposit over the cap: typed DepositCapExceeded, nothing changes.
    client.set_deposit_cap(&1_000);
    let res = client.try_deposit(&users[0], &2_000);
    assert!(matches!(res, Err(Ok(LendingError::DepositCapExceeded))));
    assert_eq!(client.get_protocol_metrics().total_supply, 0);
    assert_eq!(client.get_position(&users[0]).collateral, 0);
}
