//! Fuzz target: stateful cross-asset entrypoint sequence.
//!
//! This target drives the **real cross-asset entrypoints** through the Soroban
//! `Env` — `deposit_collateral_asset`, `withdraw_asset`, `borrow_asset`,
//! `repay_asset`, plus the legacy single-asset `liquidate` — over a generated
//! sequence of operations for 2–4 actors. It is the state-machine counterpart
//! to the pure-math targets: where those check a formula, this one checks that
//! the contract's accounting stays self-consistent no matter which order the
//! operations land in.
//!
//! ## Setup
//!
//! Each iteration builds a fresh `Env` (O(1) work per iteration, so `exec/s`
//! stays useful), registers `LendingContract`, calls `initialize`, and
//! configures two assets with `set_asset_params_v2` (LTV 5000 bps, liquidation
//! threshold 8000 bps, 7 decimals, a generous debt ceiling). Both assets get an
//! initial oracle price record, and the sequence may re-price either asset.
//!
//! No token contract is registered: the cross-asset entrypoints move no tokens,
//! and `liquidate` returns before `TokenClient` is reached because the legacy
//! single-asset `DataKey::Debt` is always zero in a cross-asset-only sequence.
//!
//! ## Operations
//!
//! `deposit` / `withdraw` / `borrow` / `repay` (user, asset, amount),
//! `liquidate` (liquidator, borrower, amount), `advance_time` (ledger clock),
//! and `set_price` (rewrites `DataKey::OraclePrice(asset)` at the current
//! ledger time). `set_price` writes the oracle record directly rather than
//! calling the `set_price` entrypoint, which requires an ed25519 signature from
//! a registered oracle key; the direct write is the same seeding pattern the
//! deterministic companion tests use.
//!
//! ## Invariants asserted after every operation
//!
//! 1. **No host trap.** Every entrypoint call resolves to a typed result — a
//!    success or a typed `LendingError`. `Err(Err(_))` (a host trap, including
//!    an arithmetic `panic!` inside the contract) is always a bug.
//! 2. **Health factor ≥ 1 after a successful borrow or withdraw.** When
//!    `borrow_asset` / `withdraw_asset` return `Ok`, the aggregate cross-asset
//!    health factor is `>= HEALTH_FACTOR_SCALE` (10 000 = 1.0). The entrypoints
//!    already enforce this internally; the assertion catches a state write that
//!    succeeds while leaving the *stored* position under-collateralised.
//! 3. **No negative balances.** Every per-user collateral balance, every
//!    per-user debt principal, and the `TotalDebtAsset` / `TotalDebt`
//!    accumulators are `>= 0`.
//! 4. **The borrow index never decreases.** `get_borrow_index()` is
//!    monotonic across the whole sequence.
//!
//! ### Cross-asset debt accounting (replaces "Σ collateral == TotalDeposits")
//!
//! The issue text asks for `Σ collateral == TotalDeposits`. That identity does
//! not hold on this surface and cannot: the cross-asset entrypoints write
//! `DataKey::CollateralAsset(user, asset)` and never touch `DataKey::TotalDeposits`
//! (nor `DataKey::TotalCollateralAsset`) — the legacy single-asset accumulators
//! belong to `deposit`/`withdraw`. The invariant that *is* maintained on the
//! cross-asset surface, and that this target therefore asserts, is the debt
//! accounting:
//!
//! 5. `DataKey::TotalDebtAsset(asset)` equals the sum over actors of the stored
//!    `DataKey::DebtAsset(user, asset).principal`; and
//! 6. `DataKey::TotalDebt` equals the sum over assets of `TotalDebtAsset(asset)`.
//!
//! Both hold exactly because `borrow_asset_internal` / `repay_asset_internal`
//! apply the same `delta = updated.principal - prev_principal` to the stored
//! position and to both accumulators. The deviation is recorded in
//! [`fuzz/README.md`](../README.md).
//!
//! ## Bounds
//!
//! Amounts are capped at [`MAX_AMOUNT`] (1e18) and prices at [`MAX_PRICE`]
//! (1e12), so a *successful* operation always leaves every intermediate product
//! several orders of magnitude below `i128::MAX`. Extreme-magnitude arithmetic
//! is covered by [`fuzz_accrual`](fuzz_accrual.rs),
//! [`fuzz_repay_borrow_roundtrip`](fuzz_repay_borrow_roundtrip.rs) and
//! [`fuzz_liquidate_close_factor`](fuzz_liquidate_close_factor.rs); this target
//! stays inside the band the entrypoints themselves admit so it keeps
//! exercising the state machine instead of degenerating into one repeat panic.

#![no_main]

use arbitrary::{Arbitrary, Result, Unstructured};
use libfuzzer_sys::fuzz_target;
use soroban_sdk::testutils::{Address as _, Ledger as _};
use soroban_sdk::{Address, Env};
use stellarlend_lending::{DataKey, LendingContract, LendingContractClient, PriceRecord};

// ── Sizing / protocol constants ─────────────────────────────────────────────

/// Number of assets configured at setup; sequences reference asset 0 or 1.
const NUM_ASSETS: usize = 2;
/// Number of actors generated; each sequence draws a live count in `2..=NUM_USERS`.
const NUM_USERS: usize = 4;
/// Starting ledger timestamp (seconds since the Unix epoch).
const START_TS: u64 = 1_700_000_000;
/// Maximum number of operations in a generated sequence.
const MAX_OPS: usize = 24;

/// `cross_asset::HEALTH_FACTOR_SCALE`: a health factor at or above this is 1.0.
const HEALTH_FACTOR_SCALE: i128 = 10_000;

/// Risk configuration written for both assets at setup.
const ASSET_LTV_BPS: i128 = 5_000;
const ASSET_THRESHOLD_BPS: i128 = 8_000;
const ASSET_DEBT_CEILING: i128 = 1_000_000_000_000_000_000; // 1e18
const ASSET_DECIMALS: u32 = 7;

/// Default oracle price: `10_000_000` == \$1.00 on the protocol's 7-decimal scale.
const UNIT_PRICE: i128 = 10_000_000;
/// Upper bound on generated oracle prices.
const MAX_PRICE: i128 = 1_000_000_000_000; // 1e12
/// Upper bound on generated amounts — see the "Bounds" note above.
const MAX_AMOUNT: i128 = 1_000_000_000_000_000_000; // 1e18
/// Longest single `advance_time` step, in seconds (one year).
const MAX_TIME_STEP: u32 = 31_536_000;

// ── Generated operations ────────────────────────────────────────────────────

#[derive(Debug)]
enum Op {
    Deposit { user: u8, asset: u8, amount: i128 },
    Withdraw { user: u8, asset: u8, amount: i128 },
    Borrow { user: u8, asset: u8, amount: i128 },
    Repay { user: u8, asset: u8, amount: i128 },
    Liquidate { liquidator: u8, borrower: u8, amount: i128 },
    AdvanceTime(u32),
    SetPrice { asset: u8, price: i128 },
}

fn gen_user(u: &mut Unstructured, n_users: u8) -> Result<u8> {
    u.int_in_range(0u8..=(n_users - 1))
}

fn gen_asset(u: &mut Unstructured) -> Result<u8> {
    u.int_in_range(0u8..=(NUM_ASSETS as u8 - 1))
}

/// Draw an amount from a multi-modal distribution: zero and small negatives to
/// exercise the `InvalidAmount` guard, small and realistic magnitudes to make
/// operations succeed, and the upper bound to push against the caps.
fn gen_amount(u: &mut Unstructured) -> Result<i128> {
    Ok(match u.int_in_range(0u8..=5)? {
        0 => 0,
        1 => u.int_in_range(-10i128..=10)?,
        2 => u.int_in_range(1..=1_000)?,
        3 => u.int_in_range(1..=1_000_000)?,
        _ => u.int_in_range(1..=MAX_AMOUNT)?,
    })
}

fn gen_time_step(u: &mut Unstructured) -> Result<u32> {
    Ok(match u.int_in_range(0u8..=3)? {
        0 => 0,
        1 => u.int_in_range(0u32..=3_600)?,
        2 => u.int_in_range(0u32..=86_400)?,
        _ => u.int_in_range(0u32..=MAX_TIME_STEP)?,
    })
}

/// Draw an oracle price. `0` is included so the `PriceFeedNotFound` path is
/// exercised; it never turns a *successful* operation into a failing invariant,
/// because a zero price makes the health-factor computation error out and the
/// borrow/withdraw path reject before returning `Ok`.
fn gen_price(u: &mut Unstructured) -> Result<i128> {
    Ok(match u.int_in_range(0u8..=4)? {
        0 => 0,
        1 | 4 => UNIT_PRICE,
        2 => 1,
        _ => u.int_in_range(1..=MAX_PRICE)?,
    })
}

fn gen_op(u: &mut Unstructured, n_users: u8) -> Result<Op> {
    Ok(match u.int_in_range(0u8..=15)? {
        0..=3 => Op::Deposit {
            user: gen_user(u, n_users)?,
            asset: gen_asset(u)?,
            amount: gen_amount(u)?,
        },
        4..=5 => Op::Withdraw {
            user: gen_user(u, n_users)?,
            asset: gen_asset(u)?,
            amount: gen_amount(u)?,
        },
        6..=8 => Op::Borrow {
            user: gen_user(u, n_users)?,
            asset: gen_asset(u)?,
            amount: gen_amount(u)?,
        },
        9..=10 => Op::Repay {
            user: gen_user(u, n_users)?,
            asset: gen_asset(u)?,
            amount: gen_amount(u)?,
        },
        11 => Op::Liquidate {
            liquidator: gen_user(u, n_users)?,
            borrower: gen_user(u, n_users)?,
            amount: gen_amount(u)?,
        },
        12..=13 => Op::AdvanceTime(gen_time_step(u)?),
        _ => Op::SetPrice {
            asset: gen_asset(u)?,
            price: gen_price(u)?,
        },
    })
}

/// A fuzzed operation sequence plus the number of live actors (2–4).
#[derive(Debug)]
struct Sequence {
    n_users: u8,
    ops: Vec<Op>,
}

impl<'a> Arbitrary<'a> for Sequence {
    fn arbitrary(u: &mut Unstructured<'a>) -> Result<Self> {
        let n_users = u.int_in_range(2u8..=NUM_USERS as u8)?;
        let n_ops = u.int_in_range(1usize..=MAX_OPS)?;
        let mut ops = Vec::with_capacity(n_ops);
        for _ in 0..n_ops {
            ops.push(gen_op(u, n_users)?);
        }
        Ok(Sequence { n_users, ops })
    }
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Classify a contract call.
///
/// Panics on a host trap (`Err(Err(_))`) or a return-value conversion error
/// (`Ok(Err(_))`) — both are bugs — and otherwise reports whether the call
/// returned a contract success.
fn succeeded<T, C, E, I>(label: &str, res: Result<Result<T, C>, Result<E, I>>) -> bool
where
    T: core::fmt::Debug,
    C: core::fmt::Debug,
    E: core::fmt::Debug,
    I: core::fmt::Debug,
{
    match res {
        Err(Err(invoke)) => panic!("{label} trapped (host error): {invoke:?}"),
        Ok(Err(conv)) => panic!("{label} return-value conversion error: {conv:?}"),
        Err(Ok(_typed_err)) => false,
        Ok(Ok(_value)) => true,
    }
}

/// Read a persistent `i128` counter owned by the contract.
fn read_counter(env: &Env, cid: &Address, key: &DataKey) -> i128 {
    env.as_contract(cid, || env.storage().persistent().get(key).unwrap_or(0))
}

// ── Invariants ──────────────────────────────────────────────────────────────

/// Assert every per-operation invariant (see the module docs).
fn check_invariants(
    env: &Env,
    cid: &Address,
    users: &[Address],
    assets: &[Address],
    last_index: &mut i128,
) {
    let client = LendingContractClient::new(env, cid);

    // Invariant 4: the global borrow index is monotonic.
    let index = client.get_borrow_index();
    assert!(
        index >= *last_index,
        "borrow index decreased: {} -> {index}",
        *last_index
    );
    *last_index = index;

    let mut per_asset_totals = Vec::with_capacity(assets.len());
    for asset in assets {
        let total = read_counter(env, cid, &DataKey::TotalDebtAsset(asset.clone()));

        // Invariant 3: accumulators never go negative.
        assert!(total >= 0, "negative TotalDebtAsset: {total}");

        // Invariant 3 + 5: per-user balances are non-negative and their stored
        // principals sum to the per-asset debt accumulator.
        let mut sum = 0i128;
        for user in users {
            let collateral = client.get_collateral_asset_balance(user, asset);
            assert!(
                collateral >= 0,
                "negative collateral balance {collateral} for {user:?}/{asset:?}"
            );
            let principal = client.get_debt_asset_position(user, asset).principal;
            assert!(
                principal >= 0,
                "negative debt principal {principal} for {user:?}/{asset:?}"
            );
            sum = sum
                .checked_add(principal)
                .expect("sum of stored principals overflowed");
        }
        assert_eq!(
            sum, total,
            "TotalDebtAsset {total} != sum of stored principals {sum} for {asset:?}"
        );
        per_asset_totals.push(total);
    }

    // Invariant 3 + 6: protocol total debt is non-negative and equals the sum
    // of the per-asset accumulators.
    let protocol = read_counter(env, cid, &DataKey::TotalDebt);
    assert!(protocol >= 0, "negative TotalDebt: {protocol}");
    let expected: i128 = per_asset_totals.iter().copied().sum();
    assert_eq!(
        protocol, expected,
        "TotalDebt {protocol} != sum of per-asset totals {expected}"
    );
}

// ── Fuzz harness ────────────────────────────────────────────────────────────

fuzz_target!(|seq: Sequence| {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(START_TS);

    let cid = env.register(LendingContract, ());
    let client = LendingContractClient::new(&env, &cid);
    let admin = Address::generate(&env);
    client.initialize(&admin);

    let assets: Vec<Address> = (0..NUM_ASSETS).map(|_| Address::generate(&env)).collect();
    for asset in &assets {
        // The generated client unwraps the contract result and panics on error, so
        // a misconfigured asset aborts the iteration instead of fuzzing a
        // half-initialised protocol.
        client.set_asset_params_v2(
            &admin,
            asset,
            &ASSET_LTV_BPS,
            &ASSET_THRESHOLD_BPS,
            &ASSET_DEBT_CEILING,
            &0,
            &0,
            &ASSET_DECIMALS,
        );
        env.as_contract(&cid, || {
            env.storage().persistent().set(
                &DataKey::OraclePrice(asset.clone()),
                &PriceRecord {
                    price: UNIT_PRICE,
                    timestamp: START_TS,
                },
            );
        });
    }

    let all_users: Vec<Address> = (0..NUM_USERS).map(|_| Address::generate(&env)).collect();
    let users = &all_users[..seq.n_users as usize];

    let mut last_index = client.get_borrow_index();

    for op in &seq.ops {
        match op {
            Op::Deposit {
                user,
                asset,
                amount,
            } => {
                let user = &users[*user as usize];
                let asset = &assets[*asset as usize];
                succeeded(
                    "deposit_collateral_asset",
                    client.try_deposit_collateral_asset(user, asset, amount),
                );
            }
            Op::Withdraw {
                user,
                asset,
                amount,
            } => {
                let user = &users[*user as usize];
                let asset = &assets[*asset as usize];
                // Invariant 2: a successful withdrawal leaves HF >= 1.
                if succeeded(
                    "withdraw_asset",
                    client.try_withdraw_asset(user, asset, amount),
                ) {
                    let hf = client.get_cross_health_factor(user);
                    assert!(
                        hf >= HEALTH_FACTOR_SCALE,
                        "withdraw_asset succeeded with health factor {hf} < {HEALTH_FACTOR_SCALE}"
                    );
                }
            }
            Op::Borrow {
                user,
                asset,
                amount,
            } => {
                let user = &users[*user as usize];
                let asset = &assets[*asset as usize];
                // Invariant 2: a successful borrow leaves HF >= 1.
                if succeeded("borrow_asset", client.try_borrow_asset(user, asset, amount)) {
                    let hf = client.get_cross_health_factor(user);
                    assert!(
                        hf >= HEALTH_FACTOR_SCALE,
                        "borrow_asset succeeded with health factor {hf} < {HEALTH_FACTOR_SCALE}"
                    );
                }
            }
            Op::Repay {
                user,
                asset,
                amount,
            } => {
                let user = &users[*user as usize];
                let asset = &assets[*asset as usize];
                succeeded("repay_asset", client.try_repay_asset(user, asset, amount));
            }
            Op::Liquidate {
                liquidator,
                borrower,
                amount,
            } => {
                let liquidator = &users[*liquidator as usize];
                let borrower = &users[*borrower as usize];
                // The legacy single-asset path: `DataKey::Debt` is never written
                // by the cross-asset entrypoints, so this always resolves to a
                // typed `PositionHealthy`. It still advances the shared borrow
                // index, which invariant 4 then checks.
                succeeded(
                    "liquidate",
                    client.try_liquidate(liquidator, borrower, &assets[0], &assets[1], amount),
                );
            }
            Op::AdvanceTime(secs) => {
                let now = env.ledger().timestamp();
                env.ledger().set_timestamp(now.saturating_add(*secs as u64));
            }
            Op::SetPrice { asset, price } => {
                let asset = &assets[*asset as usize];
                let now = env.ledger().timestamp();
                env.as_contract(&cid, || {
                    env.storage().persistent().set(
                        &DataKey::OraclePrice(asset.clone()),
                        &PriceRecord {
                            price: *price,
                            timestamp: now,
                        },
                    );
                });
            }
        }

        check_invariants(&env, &cid, users, &assets, &mut last_index);
    }
});
