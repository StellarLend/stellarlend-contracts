use proptest::prelude::*;
use crate::math::compute_compound_interest;

proptest! {

    #[test]
    fn interest_never_below_principal(
        principal in 1i128..1_000_000_000,
        rate in 0i128..5_000,
        elapsed in 0u64..10_000,
    ) {
        let result = compute_compound_interest(principal, rate, elapsed).unwrap();
        prop_assert!(result >= principal);
    }

    #[test]
    fn monotonic_in_elapsed(
        principal in 1i128..1_000_000,
        rate in 0i128..3_000,
        t1 in 0u64..500,
        t2 in 501u64..1000,
    ) {
        let r1 = compute_compound_interest(principal, rate, t1).unwrap();
        let r2 = compute_compound_interest(principal, rate, t2).unwrap();

        prop_assert!(r2 >= r1);
    }

    #[test]
    fn monotonic_in_rate(
        principal in 1i128..1_000_000,
        rate1 in 0i128..1000,
        rate2 in 1001i128..3000,
        elapsed in 0u64..1000,
    ) {
        let r1 = compute_compound_interest(principal, rate1, elapsed).unwrap();
        let r2 = compute_compound_interest(principal, rate2, elapsed).unwrap();

        prop_assert!(r2 >= r1);
    }
}
