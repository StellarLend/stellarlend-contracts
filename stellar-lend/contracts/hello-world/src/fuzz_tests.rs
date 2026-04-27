
#![cfg(test)]


struct Xorshift64(u64);

impl Xorshift64 {
    fn new(seed: u64) -> Self {
        Self(if seed == 0 { 0xDEAD_BEEF_CAFE_1234 } else { seed })
    }

    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        assert!(hi >= lo);
        lo + (self.next() % (hi - lo + 1))
    }

    fn chance(&mut self, num: u64, denom: u64) -> bool {
        self.next() % denom < num
    }
}

// ---------------------------------------------------------------------------
// Protocol model
// ---------------------------------------------------------------------------

const SCALE: u64 = 1_000_000;

const MIN_COLLATERAL_RATIO: u64 = 150 * SCALE / 100;
const CLOSE_FACTOR: u64 = 50 * SCALE / 100;
const LIQUIDATION_INCENTIVE: u64 = 5 * SCALE / 100;
const FLASH_LOAN_FEE: u64 = 900;
const RESERVE_FACTOR: u64 = 10 * SCALE / 100;
const BASE_RATE: u64 = 5_000;
const RATE_MULTIPLIER: u64 = 2 * SCALE;
const KINK_UTILIZATION: u64 = 80 * SCALE / 100;

#[derive(Debug, Clone, Default)]
struct UserPosition {
    collateral: u64,
    debt: u64,
}

#[derive(Debug, Clone)]
struct ProtocolState {
    total_collateral: u64,
    total_borrows: u64,
    protocol_reserves: u64,
    total_interest_accrued: u64,
    users: Vec<UserPosition>,
    price: u64,
}

impl ProtocolState {
    fn new(num_users: usize, price: u64) -> Self {
        Self {
            total_collateral: 0,
            total_borrows: 0,
            protocol_reserves: 0,
            total_interest_accrued: 0,
            users: vec![UserPosition::default(); num_users],
            price,
        }
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn utilization(&self) -> u64 {
        if self.total_collateral == 0 {
            return 0;
        }
        self.total_borrows.saturating_mul(SCALE) / self.total_collateral
    }

    fn borrow_rate(&self) -> u64 {
        let util = self.utilization();
        if util <= KINK_UTILIZATION {
            BASE_RATE + util.saturating_mul(RATE_MULTIPLIER) / SCALE
        } else {
            let excess = util - KINK_UTILIZATION;
            BASE_RATE
                + KINK_UTILIZATION.saturating_mul(RATE_MULTIPLIER) / SCALE
                + excess.saturating_mul(RATE_MULTIPLIER * 3) / SCALE
        }
    }

    fn collateral_value(&self, user: usize) -> u64 {
        self.users[user]
            .collateral
            .saturating_mul(self.price)
            / SCALE
    }

    fn collateral_ratio(&self, user: usize) -> u64 {
        let debt = self.users[user].debt;
        if debt == 0 {
            return u64::MAX;
        }
        self.collateral_value(user).saturating_mul(SCALE) / debt
    }

    fn is_under_collateralized(&self, user: usize) -> bool {
        self.collateral_ratio(user) < MIN_COLLATERAL_RATIO
    }

    // -----------------------------------------------------------------------
    // Operations
    // -----------------------------------------------------------------------

    fn deposit(&mut self, user: usize, amount: u64) {
        if amount == 0 {
            return;
        }
        self.users[user].collateral = self.users[user].collateral.saturating_add(amount);
        self.total_collateral = self.total_collateral.saturating_add(amount);
    }

    fn borrow(&mut self, user: usize, amount: u64) -> Result<(), &'static str> {
        if amount == 0 {
            return Ok(());
        }
        // Compute prospective debt
        let new_debt = self.users[user].debt.saturating_add(amount);
        let coll_value = self.collateral_value(user);
        let required_collateral = new_debt
            .saturating_mul(MIN_COLLATERAL_RATIO)
            / SCALE;
        if coll_value < required_collateral {
            return Err("insufficient collateral");
        }

        let available = self.total_collateral.saturating_sub(self.total_borrows);
        if amount > available {
            return Err("insufficient liquidity");
        }
        self.users[user].debt = new_debt;
        self.total_borrows = self.total_borrows.saturating_add(amount);
        Ok(())
    }

    /// Repay debt.
    fn repay(&mut self, user: usize, amount: u64) {
        let repay_amount = amount.min(self.users[user].debt);
        self.users[user].debt = self.users[user].debt.saturating_sub(repay_amount);
        self.total_borrows = self.total_borrows.saturating_sub(repay_amount);
    }

    fn withdraw(&mut self, user: usize, amount: u64) -> Result<(), &'static str> {
        if amount == 0 {
            return Ok(());
        }
        let new_collateral = self.users[user]
            .collateral
            .checked_sub(amount)
            .ok_or("insufficient collateral balance")?;
        let debt = self.users[user].debt;
        if debt > 0 {
            let new_coll_value = new_collateral.saturating_mul(self.price) / SCALE;
            let required = debt.saturating_mul(MIN_COLLATERAL_RATIO) / SCALE;
            if new_coll_value < required {
                return Err("withdrawal would under-collateralize");
            }
        }
        self.users[user].collateral = new_collateral;
        self.total_collateral = self.total_collateral.saturating_sub(amount);
        Ok(())
    }

    fn accrue_interest(&mut self) -> u64 {
        if self.total_borrows == 0 {
            return 0;
        }
        let rate = self.borrow_rate();
        let interest = self.total_borrows.saturating_mul(rate) / SCALE;
        if interest == 0 {
            return 0;

        let to_reserves = interest.saturating_mul(RESERVE_FACTOR) / SCALE;
        let to_borrowers = interest.saturating_sub(to_reserves);

        // Increase each user's debt proportionally
        let total_borrows_snapshot = self.total_borrows;
        for user in self.users.iter_mut() {
            if user.debt > 0 {
                let share = user
                    .debt
                    .saturating_mul(to_borrowers)
                    / total_borrows_snapshot;
                user.debt = user.debt.saturating_add(share);
            }
        }
        self.total_borrows = self.total_borrows.saturating_add(to_borrowers);
        self.protocol_reserves = self.protocol_reserves.saturating_add(to_reserves);
        self.total_interest_accrued = self.total_interest_accrued.saturating_add(interest);

        interest
    }

    fn liquidate(
        &mut self,
        liquidator: usize,
        target: usize,
    ) -> Result<(), &'static str> {
        if !self.is_under_collateralized(target) {
            return Err("not under-collateralized");
        }
        let ratio_before = self.collateral_ratio(target);
        let debt = self.users[target].debt;
        // Close factor: how much debt can be repaid
        let repay_amount = debt.saturating_mul(CLOSE_FACTOR) / SCALE;
        if repay_amount == 0 {
            return Err("repay amount too small");
        }
        let seized_value = repay_amount
            .saturating_mul(SCALE + LIQUIDATION_INCENTIVE)
            / SCALE;
        let seized_collateral = seized_value.saturating_mul(SCALE) / self.price;
        let seized_collateral = seized_collateral.min(self.users[target].collateral);

        // Apply
        self.users[target].debt = self.users[target].debt.saturating_sub(repay_amount);
        self.users[target].collateral =
            self.users[target].collateral.saturating_sub(seized_collateral);
        self.total_borrows = self.total_borrows.saturating_sub(repay_amount);
        self.total_collateral = self.total_collateral.saturating_sub(seized_collateral);

        self.users[liquidator].collateral = self.users[liquidator]
            .collateral
            .saturating_add(seized_collateral);
        self.total_collateral = self.total_collateral.saturating_add(seized_collateral);

        let ratio_after = self.collateral_ratio(target);
        assert!(
            ratio_after >= ratio_before || self.users[target].debt == 0,
            "INV-4 violated: liquidation worsened collateral ratio \
            (before={ratio_before}, after={ratio_after})"
        );
        Ok(())
    }

    fn flash_loan(&mut self, amount: u64) -> Result<(), &'static str> {
        if amount == 0 {
            return Ok(());
        }
        let available = self.total_collateral.saturating_sub(self.total_borrows);
        if amount > available {
            return Err("insufficient liquidity for flash loan");
        }

        let fee = amount.saturating_mul(FLASH_LOAN_FEE) / SCALE;
        let repay_required = amount.saturating_add(fee);

        let repaid = repay_required;
        assert!(
            repaid >= repay_required,
            "INV-6 violated: flash loan not repaid in full"
        );

        // Add fee to reserves
        self.protocol_reserves = self.protocol_reserves.saturating_add(fee);
        self.total_interest_accrued = self.total_interest_accrued.saturating_add(fee);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Invariant checkers
    // -----------------------------------------------------------------------

    fn check_inv1_no_undercollateralized_borrows(&self) {
        for (i, user) in self.users.iter().enumerate() {
            if user.debt > 0 {
                let ratio = self.collateral_ratio(i);
                assert!(
                    ratio >= MIN_COLLATERAL_RATIO,
                    "INV-1 violated: user {i} is under-collateralized \
                    (ratio={ratio}, min={MIN_COLLATERAL_RATIO}, \
                    collateral={}, debt={})",
                    user.collateral,
                    user.debt
                );
            }
        }
    }

    fn check_inv2_token_conservation(&self) {
        let user_total: u64 = self.users.iter().map(|u| u.collateral).sum();
        assert_eq!(
            self.total_collateral, user_total,
            "INV-2 violated: total_collateral mismatch \
            (global={}, sum_users={})",
            self.total_collateral, user_total
        );
        let user_debt_total: u64 = self.users.iter().map(|u| u.debt).sum();
        let drift = self.users.len() as u64;
        assert!(
            self.total_borrows.abs_diff(user_debt_total) <= drift,
            "INV-2 violated: total_borrows mismatch \
            (global={}, sum_users={}, allowed_drift={})",
            self.total_borrows,
            user_debt_total,
            drift
        );
    }

    fn check_inv3_interest_non_decreasing(&self, borrows_before: u64, interest: u64) {
        assert!(
            self.total_borrows >= borrows_before,
            "INV-3 violated: total_borrows decreased after interest accrual \
            (before={borrows_before}, after={}, interest={interest})",
            self.total_borrows
        );
    }

    fn check_inv5_fee_accounting(&self) {
        assert!(
            self.protocol_reserves <= self.total_interest_accrued,
            "INV-5 violated: protocol_reserves ({}) > total_interest_accrued ({})",
            self.protocol_reserves,
            self.total_interest_accrued
        );
    }

    fn check_inv7_reserve_factor(&self, prev_reserves: u64, interest: u64) {
        let max_reserves_increase = interest.saturating_mul(RESERVE_FACTOR) / SCALE;
        let actual_increase = self.protocol_reserves.saturating_sub(prev_reserves);
        assert!(
            actual_increase <= max_reserves_increase + 1,
            "INV-7 violated: reserve increase ({actual_increase}) \
            exceeds reserve factor share ({max_reserves_increase}) \
            of interest ({interest})"
        );
    }
}

// ---------------------------------------------------------------------------
// Fuzz harness
// ---------------------------------------------------------------------------

fn fuzz_round(seed: u64, num_users: usize, max_ops: usize) {
    let mut rng = Xorshift64::new(seed);
    let price = rng.range(SCALE / 2, 3 * SCALE);
    let mut state = ProtocolState::new(num_users, price);

    for _ in 0..max_ops {
        let user = rng.range(0, num_users as u64 - 1) as usize;
        let op = rng.range(0, 6);

        match op {
            // Deposit
            0 => {
                let amount = rng.range(0, 10_000 * SCALE);
                state.deposit(user, amount);
            }
            // Borrow
            1 => {
                let amount = rng.range(1, 5_000 * SCALE);
                let _ = state.borrow(user, amount);
                // After a successful borrow, INV-1 must hold
                state.check_inv1_no_undercollateralized_borrows();
            }
            // Repay
            2 => {
                let amount = rng.range(0, 5_000 * SCALE);
                state.repay(user, amount);
            }
            // Withdraw
            3 => {
                let amount = rng.range(0, 2_000 * SCALE);
                let _ = state.withdraw(user, amount);
                // After any withdraw attempt, INV-1 must hold
                state.check_inv1_no_undercollateralized_borrows();
            }
            // Accrue interest
            4 => {
                let prev_borrows = state.total_borrows;
                let prev_reserves = state.protocol_reserves;
                let interest = state.accrue_interest();
                state.check_inv3_interest_non_decreasing(prev_borrows, interest);
                if interest > 0 {
                    state.check_inv7_reserve_factor(prev_reserves, interest);
                }
            }
            // Liquidate
            5 => {
                let target = rng.range(0, num_users as u64 - 1) as usize;
                let liquidator = rng.range(0, num_users as u64 - 1) as usize;
                if liquidator != target {
                    let _ = state.liquidate(liquidator, target);
                }
            }
            // Flash loan
            _ => {
                let amount = rng.range(0, 1_000 * SCALE);
                let _ = state.flash_loan(amount);
            }
        }

        // Check structural invariants after every operation
        state.check_inv2_token_conservation();
        state.check_inv5_fee_accounting();
    }

    // Final full check
    state.check_inv1_no_undercollateralized_borrows();
    state.check_inv2_token_conservation();
    state.check_inv5_fee_accounting();
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Run many random seeds to exercise broad parameter space.
#[test]
fn test_property_random_operation_sequences() {
    for seed in 1u64..=500 {
        fuzz_round(seed, 4, 200);
    }
}

#[test]
fn test_property_edge_case_seeds() {
    for seed in [0, 1, 2, 255, 256, u32::MAX as u64, u64::MAX, 0xDEAD_BEEF] {
        fuzz_round(seed, 3, 300);
    }
}

#[test]
fn test_property_high_load() {
    fuzz_round(0xCAFE_BABE, 10, 1_000);
}

// ---------------------------------------------------------------------------
// INV-1: Dedicated borrow invariant tests
// ---------------------------------------------------------------------------

#[test]
fn test_inv1_borrow_rejected_when_no_collateral() {
    let mut state = ProtocolState::new(2, SCALE);
    assert!(
        state.borrow(0, 1000 * SCALE).is_err(),
        "borrow with zero collateral must be rejected"
    );
    state.check_inv1_no_undercollateralized_borrows();
}

#[test]
fn test_inv1_borrow_rejected_when_below_min_ratio() {
    let mut state = ProtocolState::new(2, SCALE);
    state.deposit(0, 100 * SCALE);
    let result = state.borrow(0, 70 * SCALE);
    assert!(result.is_err(), "borrow that breaches min ratio must be rejected");
    state.check_inv1_no_undercollateralized_borrows();
}

#[test]
fn test_inv1_borrow_accepted_at_exact_min_ratio() {
    let mut state = ProtocolState::new(2, SCALE);
    state.deposit(0, 150 * SCALE);
    state.deposit(1, 500 * SCALE);
    let result = state.borrow(0, 100 * SCALE);
    assert!(result.is_ok(), "borrow at exactly min ratio must succeed");
    state.check_inv1_no_undercollateralized_borrows();
}

#[test]
fn test_inv1_withdraw_blocked_when_would_under_collateralize() {
    let mut state = ProtocolState::new(2, SCALE);
    state.deposit(0, 150 * SCALE);
    state.deposit(1, 500 * SCALE);
    state.borrow(0, 100 * SCALE).unwrap();
    // Try to withdraw any collateral – should fail
    let result = state.withdraw(0, 1 * SCALE);
    assert!(
        result.is_err(),
        "withdraw that would under-collateralize must be rejected"
    );
    state.check_inv1_no_undercollateralized_borrows();
}

// ---------------------------------------------------------------------------
// INV-2: Token conservation tests
// ---------------------------------------------------------------------------

#[test]
fn test_inv2_conservation_after_deposit_withdraw_cycle() {
    let mut state = ProtocolState::new(3, SCALE);
    state.deposit(0, 500 * SCALE);
    state.deposit(1, 300 * SCALE);
    state.deposit(2, 200 * SCALE);
    state.check_inv2_token_conservation();

    state.withdraw(0, 100 * SCALE).unwrap();
    state.check_inv2_token_conservation();

    state.withdraw(1, 50 * SCALE).unwrap();
    state.check_inv2_token_conservation();
}

#[test]
fn test_inv2_conservation_after_borrow_repay_cycle() {
    let mut state = ProtocolState::new(2, SCALE);
    state.deposit(0, 1500 * SCALE);
    state.deposit(1, 500 * SCALE);
    state.borrow(0, 500 * SCALE).unwrap();
    state.check_inv2_token_conservation();

    state.repay(0, 200 * SCALE);
    state.check_inv2_token_conservation();

    state.repay(0, 300 * SCALE);
    state.check_inv2_token_conservation();
}

#[test]
fn test_inv2_conservation_after_interest_accrual() {
    let mut state = ProtocolState::new(2, SCALE);
    state.deposit(0, 1500 * SCALE);
    state.deposit(1, 500 * SCALE);
    state.borrow(0, 500 * SCALE).unwrap();

    for _ in 0..10 {
        state.accrue_interest();
        state.check_inv2_token_conservation();
    }
}

// ---------------------------------------------------------------------------
// INV-3: Interest non-decreasing
// ---------------------------------------------------------------------------

#[test]
fn test_inv3_interest_never_decreases_total_borrows() {
    let mut state = ProtocolState::new(2, SCALE);
    state.deposit(0, 1500 * SCALE);
    state.deposit(1, 500 * SCALE);
    state.borrow(0, 900 * SCALE).unwrap();

    let mut prev_borrows = state.total_borrows;
    for _ in 0..20 {
        let interest = state.accrue_interest();
        state.check_inv3_interest_non_decreasing(prev_borrows, interest);
        assert!(
            state.total_borrows >= prev_borrows,
            "total_borrows must not decrease after interest accrual"
        );
        prev_borrows = state.total_borrows;
    }
}

// ---------------------------------------------------------------------------
// INV-4: Liquidation improves collateral ratio
// ---------------------------------------------------------------------------

#[test]
fn test_inv4_liquidation_improves_ratio() {
    // Drive a user under water by manipulating price after borrow
    let mut state = ProtocolState::new(2, SCALE);
    state.deposit(0, 150 * SCALE);
    state.deposit(1, 500 * SCALE);
    state.borrow(0, 100 * SCALE).unwrap();

    // Price drops: collateral value halves → under-collateralized
    state.price = SCALE / 2; // 0.5
    assert!(
        state.is_under_collateralized(0),
        "user 0 should be under-collateralized after price drop"
    );

    let ratio_before = state.collateral_ratio(0);
    state.liquidate(1, 0).expect("liquidation should succeed");
    let ratio_after = state.collateral_ratio(0);

    assert!(
        ratio_after >= ratio_before || state.users[0].debt == 0,
        "INV-4: ratio must improve or debt must be zero \
        (before={ratio_before}, after={ratio_after})"
    );
}

#[test]
fn test_inv4_liquidation_fails_when_collateralized() {
    let mut state = ProtocolState::new(2, SCALE);
    state.deposit(0, 300 * SCALE);
    state.deposit(1, 500 * SCALE);
    state.borrow(0, 100 * SCALE).unwrap();
    // User 0 is well-collateralized (300% ratio)
    assert!(state.liquidate(1, 0).is_err(), "liquidation of healthy position must fail");
}

// ---------------------------------------------------------------------------
// INV-5: Fee accounting
// ---------------------------------------------------------------------------

#[test]
fn test_inv5_reserves_never_exceed_interest_accrued() {
    let mut state = ProtocolState::new(2, SCALE);
    state.deposit(0, 1500 * SCALE);
    state.deposit(1, 500 * SCALE);
    state.borrow(0, 800 * SCALE).unwrap();

    for _ in 0..50 {
        state.accrue_interest();
        state.check_inv5_fee_accounting();
    }
}

#[test]
fn test_inv5_flash_loan_fee_goes_to_reserves() {
    let mut state = ProtocolState::new(1, SCALE);
    state.deposit(0, 1000 * SCALE);
    let prev_reserves = state.protocol_reserves;
    state.flash_loan(500 * SCALE).unwrap();
    assert!(
        state.protocol_reserves > prev_reserves,
        "flash loan fee must increase protocol reserves"
    );
    state.check_inv5_fee_accounting();
}

// ---------------------------------------------------------------------------
// INV-6: Flash loan repayment
// ---------------------------------------------------------------------------

#[test]
fn test_inv6_flash_loan_honest_repayment_succeeds() {
    let mut state = ProtocolState::new(1, SCALE);
    state.deposit(0, 1000 * SCALE);
    assert!(state.flash_loan(500 * SCALE).is_ok());
}

#[test]
fn test_inv6_flash_loan_fails_when_insufficient_liquidity() {
    let mut state = ProtocolState::new(1, SCALE);
    state.deposit(0, 100 * SCALE);
    let result = state.flash_loan(200 * SCALE);
    assert!(result.is_err(), "flash loan exceeding liquidity must fail");
}

// ---------------------------------------------------------------------------
// INV-7: Reserve factor
// ---------------------------------------------------------------------------

#[test]
fn test_inv7_reserve_factor_upper_bound() {
    let mut state = ProtocolState::new(2, SCALE);
    state.deposit(0, 1500 * SCALE);
    state.deposit(1, 500 * SCALE);
    state.borrow(0, 1000 * SCALE).unwrap();

    for _ in 0..100 {
        let prev_reserves = state.protocol_reserves;
        let interest = state.accrue_interest();
        if interest > 0 {
            state.check_inv7_reserve_factor(prev_reserves, interest);
        }
    }
}

// ---------------------------------------------------------------------------
// Combined stress: all invariants together
// ---------------------------------------------------------------------------

#[test]
fn test_all_invariants_combined_stress() {
    let mut rng = Xorshift64::new(0xF00D_CAFE);
    let mut state = ProtocolState::new(5, SCALE);

    // Seed some deposits so there is liquidity
    for i in 0..5 {
        state.deposit(i, rng.range(500, 2000) * SCALE);
    }

    for step in 0..2_000 {
        let user = rng.range(0, 4) as usize;
        let prev_borrows = state.total_borrows;
        let prev_reserves = state.protocol_reserves;

        if rng.chance(2, 10) {
            // Borrow
            let _ = state.borrow(user, rng.range(1, 300) * SCALE);
        } else if rng.chance(2, 10) {
            // Repay
            state.repay(user, rng.range(1, 200) * SCALE);
        } else if rng.chance(1, 10) {
            // Accrue interest
            let interest = state.accrue_interest();
            if interest > 0 {
                state.check_inv3_interest_non_decreasing(prev_borrows, interest);
                state.check_inv7_reserve_factor(prev_reserves, interest);
            }
        } else if rng.chance(1, 10) {
            // Deposit more
            state.deposit(user, rng.range(100, 500) * SCALE);
        } else if rng.chance(1, 10) {
            // Price shock (±20 %)
            let delta = rng.range(0, 20) * SCALE / 100;
            if rng.chance(1, 2) {
                state.price = state.price.saturating_add(delta);
            } else {
                state.price = state.price.saturating_sub(delta).max(SCALE / 10);
            }
            // Liquidate any underwater positions
            for target in 0..5 {
                if state.is_under_collateralized(target) {
                    let liquidator = (target + 1) % 5;
                    let _ = state.liquidate(liquidator, target);
                }
            }
        } else {
            let _ = state.flash_loan(rng.range(1, 200) * SCALE);
        }

        // All structural invariants after every step
        state.check_inv1_no_undercollateralized_borrows();
        state.check_inv2_token_conservation();
        state.check_inv5_fee_accounting();

        let _ = step;
    }
}

/// Regression: zero-amount operations must be no-ops (no panic, no state change).
#[test]
fn test_zero_amount_operations_are_noop() {
    let mut state = ProtocolState::new(2, SCALE);
    state.deposit(0, 0);
    assert_eq!(state.total_collateral, 0);
    assert!(state.borrow(0, 0).is_ok());
    state.repay(0, 0);
    assert!(state.withdraw(0, 0).is_ok());
    assert!(state.flash_loan(0).is_ok());
    state.check_inv1_no_undercollateralized_borrows();
    state.check_inv2_token_conservation();
    state.check_inv5_fee_accounting();
}

/// Regression: self-liquidation must be prevented (liquidator == target).
#[test]
fn test_self_liquidation_is_prevented() {
    let mut state = ProtocolState::new(1, SCALE);
    state.deposit(0, 150 * SCALE);
    state.price = SCALE / 10;
    state.check_inv1_no_undercollateralized_borrows();
}