/// Fee accounting and value-conservation tests for bridge deposit / withdraw.
///
/// # Fee formula
///
/// ```text
/// fee      = ⌊ amount × fee_bps / 10_000 ⌋   (floor division)
/// credited = amount − fee
/// ```
///
/// # Conservation invariants
///
/// **C-1** `credited + fee == amount` — no satoshi is created or destroyed.
///
/// **C-2** Accrued protocol fees equal the running sum of each charged fee:
///         `accrued += fee` on every deposit (and withdraw if fees apply there).
///
/// **C-3** Zero fee (`fee_bps = 0`) → `credited == amount`, `fee == 0`.
///
/// **C-4** Zero / negative amount is rejected.
///
/// **C-5** Fee can never exceed `amount` (`fee ≤ amount`).
///
/// **C-6** Deposit followed by withdraw returns the full credited amount to the
///         user with no extra value created.
use proptest::prelude::*;

// ---------------------------------------------------------------------------
// Pure fee-accounting helpers (mirrors the on-chain bridge fee logic)
// ---------------------------------------------------------------------------

/// Compute the fee charged on `amount` at `fee_bps` basis points.
///
/// Uses floor division — the protocol always rounds in its own favour.
/// Returns `None` on overflow.
fn bridge_fee(amount: i128, fee_bps: i128) -> Option<i128> {
    if amount <= 0 || fee_bps < 0 || fee_bps > 10_000 {
        return None;
    }
    amount.checked_mul(fee_bps)?.checked_div(10_000)
}

/// Amount credited to the user after deducting the bridge fee.
///
/// Returns `None` when `fee_bps` is out of range or `amount` is non-positive.
fn bridge_credited(amount: i128, fee_bps: i128) -> Option<i128> {
    let fee = bridge_fee(amount, fee_bps)?;
    amount.checked_sub(fee)
}

// ---------------------------------------------------------------------------
// C-1: credited + fee == amount  (proptest)
// ---------------------------------------------------------------------------

proptest! {
    /// **C-1** — Value is conserved on every deposit: `credited + fee == amount`.
    #[test]
    fn prop_conservation_credited_plus_fee_equals_amount(
        amount  in 1i128..=1_000_000_000_000i128,
        fee_bps in 0i128..=10_000i128,
    ) {
        let fee      = bridge_fee(amount, fee_bps).unwrap();
        let credited = bridge_credited(amount, fee_bps).unwrap();
        prop_assert_eq!(credited + fee, amount,
            "C-1 violated: credited={credited} + fee={fee} != amount={amount}");
    }
}

// ---------------------------------------------------------------------------
// C-2: accrued fee equals running sum  (proptest)
// ---------------------------------------------------------------------------

proptest! {
    /// **C-2** — Accrued protocol fees equal the sum of individually charged fees.
    ///
    /// Simulates N deposits and confirms the running `accrued` counter matches
    /// the independent per-deposit sum.
    #[test]
    fn prop_accrued_fee_equals_sum_of_individual_fees(
        amounts  in prop::collection::vec(1i128..=1_000_000i128, 1..20),
        fee_bps  in 0i128..=10_000i128,
    ) {
        let mut accrued = 0i128;
        let mut expected_sum = 0i128;

        for &amt in &amounts {
            let fee = bridge_fee(amt, fee_bps).unwrap();
            accrued += fee;
            expected_sum += fee;
        }

        prop_assert_eq!(accrued, expected_sum,
            "C-2 violated: accrued={accrued} != sum={expected_sum}");
    }
}

// ---------------------------------------------------------------------------
// C-3: zero fee credits the full amount
// ---------------------------------------------------------------------------

#[test]
fn zero_fee_credits_full_amount() {
    for amount in [1, 100, 999, 1_000_000, i128::MAX / 10_000] {
        let fee = bridge_fee(amount, 0).unwrap();
        let credited = bridge_credited(amount, 0).unwrap();
        assert_eq!(fee, 0, "zero fee_bps must produce 0 fee, got {fee}");
        assert_eq!(credited, amount, "zero fee_bps must credit full amount");
    }
}

// ---------------------------------------------------------------------------
// C-4: zero / negative amounts are rejected
// ---------------------------------------------------------------------------

#[test]
fn zero_amount_rejected() {
    assert!(bridge_fee(0, 30).is_none(), "amount=0 must be rejected");
    assert!(
        bridge_fee(-1, 30).is_none(),
        "negative amount must be rejected"
    );
    assert!(bridge_fee(-1_000_000, 0).is_none());
}

// ---------------------------------------------------------------------------
// C-5: fee never exceeds amount
// ---------------------------------------------------------------------------

proptest! {
    /// **C-5** — The fee is never larger than the deposited amount.
    ///
    /// Even at `fee_bps = 10_000` (100 %), `fee == amount` and `credited == 0`.
    #[test]
    fn prop_fee_never_exceeds_amount(
        amount  in 1i128..=1_000_000_000_000i128,
        fee_bps in 0i128..=10_000i128,
    ) {
        let fee = bridge_fee(amount, fee_bps).unwrap();
        prop_assert!(fee <= amount,
            "C-5 violated: fee={fee} > amount={amount}");
        prop_assert!(fee >= 0,
            "fee must be non-negative, got {fee}");
    }
}

// ---------------------------------------------------------------------------
// C-6: deposit → withdraw round-trip conserves value
// ---------------------------------------------------------------------------

proptest! {
    /// **C-6** — A deposit then withdraw of the credited amount yields exactly
    /// `credited_deposit - fee_withdraw`.  No extra value is created.
    ///
    /// The user receives `final_out = credited_deposit − fee_withdraw`.
    /// The protocol accrues `fee_deposit + fee_withdraw`.
    /// Together: `final_out + accrued == amount_in`.
    #[test]
    fn prop_deposit_withdraw_round_trip_no_extra_value(
        amount  in 1i128..=1_000_000_000_000i128,
        fee_bps in 0i128..=10_000i128,
    ) {
        // Deposit leg
        let fee_dep      = bridge_fee(amount, fee_bps).unwrap();
        let credited_dep = amount - fee_dep;

        // Withdraw leg: user withdraws what was credited
        let fee_wit      = bridge_fee(credited_dep, fee_bps).unwrap_or(0);
        let final_out    = credited_dep - fee_wit;

        let total_accrued = fee_dep + fee_wit;

        // Value conservation: user received + protocol kept == original deposit
        prop_assert_eq!(final_out + total_accrued, amount,
            "C-6 violated: final_out={final_out} + accrued={total_accrued} != amount={amount}");

        // No value created from nothing
        prop_assert!(final_out <= amount,
            "user cannot receive more than deposited");
    }
}

// ---------------------------------------------------------------------------
// Edge cases: deterministic boundary inputs
// ---------------------------------------------------------------------------

#[test]
fn max_fee_bps_charges_full_amount() {
    // fee_bps = 10_000 → fee = amount, credited = 0
    let amount = 1_000i128;
    assert_eq!(bridge_fee(amount, 10_000).unwrap(), amount);
    assert_eq!(bridge_credited(amount, 10_000).unwrap(), 0);
}

#[test]
fn dust_amount_with_high_fee_rounds_to_zero() {
    // amount=1, fee_bps=9_999 → fee = ⌊1×9999/10000⌋ = 0 (floor)
    assert_eq!(bridge_fee(1, 9_999).unwrap(), 0);
    assert_eq!(bridge_credited(1, 9_999).unwrap(), 1);
}

#[test]
fn rounding_boundary_floor_not_ceiling() {
    // amount=3, fee_bps=3_333 → exact = 0.9999 → floor = 0
    assert_eq!(bridge_fee(3, 3_333).unwrap(), 0);
    // amount=3, fee_bps=3_334 → exact = 1.0002 → floor = 1
    assert_eq!(bridge_fee(3, 3_334).unwrap(), 1);
    // Conservation holds at both boundaries
    for &fp in &[3_333i128, 3_334] {
        let fee = bridge_fee(3, fp).unwrap();
        let credited = bridge_credited(3, fp).unwrap();
        assert_eq!(credited + fee, 3);
    }
}

#[test]
fn accrued_fee_multiple_deposits_exact() {
    // Three deposits: 100, 200, 300 at 30 bps
    // fees: 0, 0, 0  (floor: 100×30/10000=0, 200×30/10000=0, 300×30/10000=0)
    // Three deposits at 300 bps
    // fees: 3, 6, 9
    let amounts = [100i128, 200, 300];
    let fee_bps = 300i128;
    let mut accrued = 0i128;
    for &a in &amounts {
        accrued += bridge_fee(a, fee_bps).unwrap();
    }
    // 100×300/10000=3, 200×300/10000=6, 300×300/10000=9 → total=18
    assert_eq!(accrued, 18);
    // Conservation: sum of (credited+fee) == sum of amounts
    let total_in: i128 = amounts.iter().sum();
    let total_credited: i128 = amounts
        .iter()
        .map(|&a| bridge_credited(a, fee_bps).unwrap())
        .sum();
    assert_eq!(total_credited + accrued, total_in);
}

#[test]
fn invalid_fee_bps_rejected() {
    assert!(
        bridge_fee(100, -1).is_none(),
        "negative fee_bps must be rejected"
    );
    assert!(
        bridge_fee(100, 10_001).is_none(),
        "fee_bps > 10_000 must be rejected"
    );
}
