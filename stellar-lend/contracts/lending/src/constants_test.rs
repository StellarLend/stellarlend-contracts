//! Tests for the `constants` module.
//!
//! Verifies that all BPS / ratio constants satisfy their documented invariants
//! so that a future refactor cannot silently break the bounds.

#[cfg(test)]
mod tests {
    use crate::constants::*;

    #[test]
    fn bps_scale_is_ten_thousand() {
        assert_eq!(BPS_SCALE, 10_000);
    }

    #[test]
    fn health_factor_scale_equals_bps_scale() {
        assert_eq!(HEALTH_FACTOR_SCALE, BPS_SCALE);
    }

    #[test]
    fn max_flash_loan_fee_within_bps_scale() {
        assert!(MAX_FLASH_LOAN_FEE_BPS > 0);
        assert!(MAX_FLASH_LOAN_FEE_BPS <= BPS_SCALE);
        assert_eq!(MAX_FLASH_LOAN_FEE_BPS, 1_000); // 10%
    }

    #[test]
    fn min_collateral_ratio_above_bps_scale() {
        // Must be > 100% to ensure over-collateralisation
        assert!(MIN_COLLATERAL_RATIO_BPS > BPS_SCALE);
        assert_eq!(MIN_COLLATERAL_RATIO_BPS, 15_000); // 150%
    }

    #[test]
    fn default_liquidation_threshold_within_bps_scale() {
        assert!(DEFAULT_LIQUIDATION_THRESHOLD_BPS > 0);
        assert!(DEFAULT_LIQUIDATION_THRESHOLD_BPS <= BPS_SCALE);
        assert_eq!(DEFAULT_LIQUIDATION_THRESHOLD_BPS, 8_000); // 80%
    }

    #[test]
    fn default_close_factor_within_bps_scale() {
        assert!(DEFAULT_CLOSE_FACTOR_BPS > 0);
        assert!(DEFAULT_CLOSE_FACTOR_BPS <= BPS_SCALE);
        assert_eq!(DEFAULT_CLOSE_FACTOR_BPS, 5_000); // 50%
    }

    #[test]
    fn default_liquidation_incentive_within_bps_scale() {
        assert!(DEFAULT_LIQUIDATION_INCENTIVE_BPS >= 0);
        assert!(DEFAULT_LIQUIDATION_INCENTIVE_BPS <= BPS_SCALE);
        assert_eq!(DEFAULT_LIQUIDATION_INCENTIVE_BPS, 1_000); // 10%
    }
}
