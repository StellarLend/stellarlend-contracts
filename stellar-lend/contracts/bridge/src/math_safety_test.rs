use crate::bridge::BridgeContract;
use soroban_sdk::Env;

#[test]
fn test_bridge_fee_check() {
    let env = Env::default();

    // 10^30 tokens * 10% fee (1000 bps)
    let amount = 1_000_000_000_000_000_000_000_000_000_000i128;
    let fee = BridgeContract::compute_fee(env.clone(), amount, 1000);
    // 10^30 * 1000 / 10000 = 10^29
    assert_eq!(fee, 100_000_000_000_000_000_000_000_000_000i128);

    // Test with absolute max that would overflow intermediate multiplication
    // i128::MAX * 1000 > i128::MAX
    // But since result fits in i128 (exact is approx i128::MAX / 10), it should work with I256
    let max_amount = i128::MAX;
    let fee_large = BridgeContract::compute_fee(env.clone(), max_amount, 1000);
    assert!(fee_large > 0);
    assert_eq!(fee_large, max_amount / 10);
}
