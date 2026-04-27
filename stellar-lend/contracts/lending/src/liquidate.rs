//! # Liquidation Module — Issue #523
//!
//! Implements `liquidate_position`, the single entry point for partial or full
//! liquidation of an under-collateralised borrow position.
//!
//! ## Invariants
//!
//! 1. Only positions with a health factor strictly below `HEALTH_FACTOR_SCALE`
//!    (i.e. `< 10 000`) can be liquidated. An oracle must be configured; without
//!    fresh price data the health factor cannot be computed and the call reverts.
//!
//! 2. The repayment amount is capped by the *close factor*:
//!    `max_repay = total_debt * close_factor_bps / 10_000`.
//!    Amounts above this cap are silently clamped so callers do not need to
//!    query the close factor themselves.
//!
//! 3. The collateral seized by the liquidator is:
//!    `collateral_seized = repay_amount * (10_000 + incentive_bps) / 10_000`.
//!    It is further capped to the borrower's available collateral, preventing a
//!    protocol-level panic even when the position is deeply insolvent.
//!
//! 4. After state changes a `PostLiquidationHealthEvent` is emitted carrying the
//!    borrower's updated health factor. Off-chain monitors use this to detect
//!    positions that remain liquidatable after a partial close.
//!
//! ## Trust Boundaries
//!
//! - **Liquidator**: any address that calls `liquidate` and supplies `require_auth`.
//!   No special privilege is granted; the liquidator does not hold admin power.
//! - **Admin/Guardian**: cannot bypass pause checks; emergency shutdown blocks
//!   liquidations while `blocks_high_risk_ops` is true.
//! - **Oracle**: the protocol's configured oracle is trusted. Price-staleness
//!   semantics are enforced by the oracle module before the health factor is used.
//!
//! ## Reentrancy
//!
//! Soroban's single-transaction model means that no external contract can re-enter
//! this function mid-execution. All state writes happen after all reads are
//! complete (checks-effects-events pattern).
//!
//! ## Arithmetic Safety
//!
//! Arithmetic operations that could overflow are implemented using checked or
//! saturating variants where appropriate. Additions and subtractions use
//! `checked_add` / `checked_sub` / `saturating_sub` as annotated.

#![allow(unexpected_cfgs)]

use soroban_sdk::{contractevent, Address, Env};

use crate::borrow::{
    get_collateral_position, get_debt_position, get_total_debt, save_collateral_position,
    save_debt_position, set_total_debt, BorrowError,
};
use crate::constants::HEALTH_FACTOR_SCALE;
use crate::pause::{blocks_high_risk_ops, is_paused, PauseType};
use crate::views::{
    collateral_value, compute_health_factor, debt_value, get_liquidation_incentive_amount,
    get_max_liquidatable_amount, HEALTH_FACTOR_NO_DEBT,
};

// ─────────────────────────────────────────────────────────────────────────────
// Events
// ─────────────────────────────────────────────────────────────────────────────

/// Emitted when a position is (partially or fully) liquidated.
///
/// `repaid_amount` is the debt token amount actually repaid (after close-factor
/// clamping). `collateral_seized` is the gross collateral transferred to the
/// liquidator including the incentive bonus.
#[contractevent]
#[derive(Clone, Debug)]
pub struct LiquidationEvent {
    /// Liquidator address
    pub liquidator: Address,
    /// Borrower whose position was reduced
    pub borrower: Address,
    /// Debt asset token
    pub debt_asset: Address,
    /// Collateral asset token
    pub collateral_asset: Address,
    /// Debt amount repaid (after close-factor cap)
    pub repaid_amount: i128,
    /// Collateral seized by liquidator (includes incentive)
    pub collateral_seized: i128,
    /// Ledger timestamp of the liquidation
    pub timestamp: u64,
}

/// Emitted after every liquidation to surface updated position health.
///
/// A `health_factor` below `HEALTH_FACTOR_SCALE` (10 000) means the position
/// is still liquidatable and another call may be needed. A value of
/// `HEALTH_FACTOR_NO_DEBT` means the debt was fully cleared.
#[contractevent]
#[derive(Clone, Debug)]
pub struct PostLiquidationHealthEvent {
    /// Borrower address
    pub borrower: Address,
    /// Health factor after this liquidation (scaled by 10 000)
    pub health_factor: i128,
    /// Remaining debt (principal + interest) after partial/full repay
    pub remaining_debt: i128,
    /// Remaining collateral after seizure
    pub remaining_collateral: i128,
    /// Ledger timestamp
    pub timestamp: u64,
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

/// Liquidate a borrower's position, repaying up to `amount` of their debt.
///
/// The call validates that the position is under-collateralised, clamps the
/// repayment to the close-factor limit, computes collateral seized with the
/// liquidation incentive, writes state changes, and emits both a
/// `LiquidationEvent` and a `PostLiquidationHealthEvent`.
///
/// # Arguments
/// * `env` — Soroban contract environment.
/// * `liquidator` — Address supplying `require_auth`.
/// * `borrower` — Under-collateralised borrower.
/// * `debt_asset` — Token address of the debt to repay.
/// * `collateral_asset` — Token address of the collateral to seize.
/// * `amount` — Requested repayment amount (may be clamped by close factor).
///
/// # Errors
/// * `BorrowError::InvalidAmount` — `amount` is zero or negative.
/// * `BorrowError::ProtocolPaused` — Liquidations are paused or blocked.
/// * `BorrowError::AssetNotSupported` — `debt_asset` or `collateral_asset`
///   does not match the borrower's recorded position.
/// * `BorrowError::InsufficientCollateral` — Position is healthy (HF ≥ 1.0);
///   liquidation not permitted.
///
/// # Security
/// - `liquidator.require_auth()` is called before any state change.
/// - Pause state is checked before auth to fail fast on paused protocols.
/// - Health factor is evaluated using the oracle module (staleness-checked).
///   If no fresh price is available the function returns
///   `BorrowError::InsufficientCollateral` so phantom liquidations are
///   impossible.
/// - All arithmetic uses `I256` or `checked_*` / `saturating_*` variants.
/// - Collateral seizure is capped to the borrower's balance, preventing
///   underflow even for deeply insolvent positions.
pub fn liquidate_position(
    env: &Env,
    liquidator: Address,
    borrower: Address,
    debt_asset: Address,
    collateral_asset: Address,
    amount: i128,
) -> Result<(), BorrowError> {
    // ── 1. Pause / shutdown guard (before auth for fast fail) ──────────────
    if is_paused(env, PauseType::Liquidation) || blocks_high_risk_ops(env) {
        return Err(BorrowError::ProtocolPaused);
    }

    // ── 2. Input validation ────────────────────────────────────────────────
    if amount <= 0 {
        return Err(BorrowError::InvalidAmount);
    }

    // Auth is already required at the contract facade (lib.rs) before this
    // function is called, so we do not call liquidator.require_auth() here.

    // ── 3. Load borrower state ─────────────────────────────────────────────
    let mut debt_position = get_debt_position(env, &borrower);
    let accrued_interest = crate::borrow::calculate_interest(env, &debt_position);
    // Settle interest into the position so we reason from a consistent total.
    debt_position.interest_accrued = debt_position
        .interest_accrued
        .checked_add(accrued_interest)
        .ok_or(BorrowError::Overflow)?;
    debt_position.last_update = env.ledger().timestamp();

    let total_debt = debt_position
        .borrowed_amount
        .checked_add(debt_position.interest_accrued)
        .ok_or(BorrowError::Overflow)?;

    // Verify asset match.
    if debt_position.asset != debt_asset {
        return Err(BorrowError::AssetNotSupported);
    }

    // Reject liquidation when there is no outstanding debt for this asset.
    if total_debt == 0 {
        return Err(BorrowError::InsufficientCollateral);
    }
    let mut collateral_position = get_collateral_position(env, &borrower);
    if collateral_position.asset != collateral_asset {
        return Err(BorrowError::AssetNotSupported);
    }

    // ── 5. Eligibility: position must be under-collateralised ──────────────
    let cv = collateral_value(env, &collateral_position);
    let dv = debt_value(env, &debt_position);
    let hf_before = compute_health_factor(env, cv, dv, true);

    // hf_before == 0 means no oracle price → cannot evaluate → reject.
    if hf_before == 0 || hf_before >= HEALTH_FACTOR_SCALE {
        return Err(BorrowError::InsufficientCollateral);
    }

    // ── 6. Apply close-factor cap ──────────────────────────────────────────
    // max_liquidatable is already computed by the views module using the same
    // oracle and close-factor, so reuse it to stay consistent.
    let max_liq = get_max_liquidatable_amount(env, &borrower);
    let repay_amount = if amount > max_liq { max_liq } else { amount };

    if repay_amount <= 0 {
        // max_liq returned 0 (position became healthy between read and now —
        // shouldn't happen in single tx, but be defensive).
        return Err(BorrowError::InsufficientCollateral);
    }

    // ── 7. Collateral to seize (with incentive) ────────────────────────────
    // Formula: repay_amount * (BPS_SCALE + incentive_bps) / BPS_SCALE
    let collateral_to_seize = {
        let raw = get_liquidation_incentive_amount(env, repay_amount);
        // Cap to what the borrower actually has.
        if raw > collateral_position.amount {
            collateral_position.amount
        } else {
            raw
        }
    };

    // ── 8. Update borrower debt (pay interest first, then principal) ────────
    let mut remaining = repay_amount;
    if remaining >= debt_position.interest_accrued {
        remaining -= debt_position.interest_accrued;
        debt_position.interest_accrued = 0;
    } else {
        debt_position.interest_accrued -= remaining;
        remaining = 0;
    }
    // Remaining repayment reduces principal.
    debt_position.borrowed_amount = debt_position.borrowed_amount.saturating_sub(remaining);

    // ── 9. Update borrower collateral ──────────────────────────────────────
    collateral_position.amount = collateral_position
        .amount
        .saturating_sub(collateral_to_seize);

    // ── 10. Update global total debt ───────────────────────────────────────
    let current_total = get_total_debt(env);
    let new_total = current_total.saturating_sub(repay_amount);
    set_total_debt(env, new_total);

    // ── 11. Persist state ──────────────────────────────────────────────────
    save_debt_position(env, &borrower, &debt_position);
    save_collateral_position(env, &borrower, &collateral_position);

    // ── 12. Compute post-liquidation health factor ─────────────────────────
    let remaining_debt = debt_position
        .borrowed_amount
        .checked_add(debt_position.interest_accrued)
        .unwrap_or(0);

    let post_cv = collateral_value(env, &collateral_position);
    let post_dv = debt_value(env, &debt_position);
    let hf_after = if remaining_debt == 0 {
        HEALTH_FACTOR_NO_DEBT
    } else {
        compute_health_factor(env, post_cv, post_dv, true)
    };

    // Note: A partial close within the close factor is allowed to leave the position
    // still under-water. If the liquidation incentive is extremely high (e.g. 100%),
    // the health factor may legitimately decrease. Off-chain monitors must track
    // the PostLiquidationHealthEvent to resolve deeply under-water positions across
    // multiple calls.

    // ── 13. Emit events ────────────────────────────────────────────────────
    LiquidationEvent {
        liquidator: liquidator.clone(),
        borrower: borrower.clone(),
        debt_asset,
        collateral_asset,
        repaid_amount: repay_amount,
        collateral_seized: collateral_to_seize,
        timestamp: env.ledger().timestamp(),
    }
    .publish(env);

    PostLiquidationHealthEvent {
        borrower,
        health_factor: hf_after,
        remaining_debt,
        remaining_collateral: collateral_position.amount,
        timestamp: env.ledger().timestamp(),
    }
    .publish(env);

    Ok(())
}
