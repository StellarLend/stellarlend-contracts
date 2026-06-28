// Verification test for liquidation mechanics documentation
// Ensures that the formulae and examples in LIQUIDATION_MECHANICS.md remain correct.

#[cfg(test)]
use stellar_lend_common::{BPS_DENOM, CLOSE_FACTOR, LIQUIDATION_THRESHOLD_BPS};
mod tests {
    use soroban_sdk::{testutils::Address as _, Env};
    use stellarlend_lending::math;
    use stellarlend_lending::*;

    // Helper to compute seized collateral based on actual repay
    fn compute_seized(actual_repay: i128) -> i128 {
        // BPS_DENOM and INCENTIVE_BPS are defined in lib.rs
        const BPS_DENOM: i128 = 10_000;
        const INCENTIVE_BPS: i128 = 1_000; // 10%
        math::checked_mul_div_floor(actual_repay, BPS_DENOM + INCENTIVE_BPS, BPS_DENOM).unwrap()
    }

    #[test]
    fn example_one_simple_liquidation() {
        // Example 1 from the doc
        let collateral: i128 = 8_000;
        let debt: i128 = 10_000;
        let requested: i128 = 6_000;

        // Health factor
        let hf =
            math::checked_mul_div_floor(collateral, LIQUIDATION_THRESHOLD_BPS, 10_000).unwrap();
        assert!(hf < 10_000);

        // Close factor cap
        let max_repay = math::checked_mul_div_floor(debt, CLOSE_FACTOR, BPS_DENOM).unwrap();
        assert_eq!(max_repay, 5_000);
        let actual_repay = std::cmp::min(requested, max_repay);
        assert_eq!(actual_repay, 5_000);

        // Seized collateral
        let seized = compute_seized(actual_repay);
        assert_eq!(seized, 550);
        assert_eq!(collateral - seized, 7_450);
        assert_eq!(debt - actual_repay, 5_000);
    }

    #[test]
    fn example_two_close_factor_capped_and_shortfall() {
        // Example 2 from the doc
        let collateral: i128 = 2_000;
        let debt: i128 = 12_000;
        let requested: i128 = 8_000;

        // Health factor
        let hf = math::checked_mul_div_floor(collateral, LIQUIDATION_THRESHOLD_BPS, debt).unwrap();
        assert!(hf < 10_000);

        // Close factor cap
        let max_repay = math::checked_mul_div_floor(debt, CLOSE_FACTOR, BPS_DENOM).unwrap();
        assert_eq!(max_repay, 6_000);
        let actual_repay = std::cmp::min(requested, max_repay);
        assert_eq!(actual_repay, 6_000);

        // Seized collateral (capped by collateral amount)
        let seized = compute_seized(actual_repay);
        // Seized computation would give 6_600, but limited by collateral
        let seized_capped = std::cmp::min(seized, collateral);
        assert_eq!(seized_capped, 2_000);
        // Bad debt remains
        let remaining_debt = debt - actual_repay;
        assert_eq!(remaining_debt, 6_000);
    }
}
