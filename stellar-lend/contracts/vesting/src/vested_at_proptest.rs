use proptest::prelude::*;

use super::Grant;

/// Generates a `Grant` with bounded fields that avoid arithmetic overflow in
/// `Grant::vested_at`. Times are capped at `MAX_TIME` and `total` at
/// `MAX_PRINCIPAL` so that the intermediate multiplication
/// `total * elapsed` always fits in `u128`.
const MAX_TIME: u64 = 1_000_000_000;
const MAX_PRINCIPAL: u128 = 1_000_000_000_000_000;

fn any_grant() -> impl Strategy<Value = Grant> {
    (
        any::<u128>().prop_map(|v| v % MAX_PRINCIPAL),
        0..=MAX_TIME,
        0..=MAX_TIME,
        0..=MAX_TIME,
    )
        .prop_filter("start + cliff must not overflow u64", |&(_, s, _, c)| {
            s.checked_add(c).is_some()
        })
        .prop_map(|(total, start, duration, cliff)| Grant {
            grantee: "proptest".into(),
            total,
            claimed: 0,
            released: 0,
            start_seconds: start,
            duration_seconds: duration,
            cliff_seconds: cliff,
            revoked: false,
        })
}

proptest! {
    #[test]
    fn vested_at_never_exceeds_principal(
        grant in any_grant(),
        now in 0..=MAX_TIME * 2,
    ) {
        let v = grant.vested_at(now);
        assert!(
            v <= grant.total,
            "vested_at({now}) = {v} > total = {} on grant={grant:?}",
            grant.total,
        );
    }

    #[test]
    fn vested_at_is_monotonic(
        grant in any_grant(),
        t1 in 0..=MAX_TIME * 2,
        t2 in 0..=MAX_TIME * 2,
    ) {
        prop_assume!(t1 <= t2);
        let v1 = grant.vested_at(t1);
        let v2 = grant.vested_at(t2);
        assert!(
            v1 <= v2,
            "vested_at({t1}) = {v1} > vested_at({t2}) = {v2} for grant={grant:?}",
        );
    }

    #[test]
    fn vested_at_zero_before_cliff(
        grant in any_grant(),
    ) {
        let cliff_end = match grant.start_seconds.checked_add(grant.cliff_seconds) {
            Some(ts) if ts > 0 => ts,
            _ => return Ok(()),
        };
        let before = cliff_end - 1;
        assert_eq!(
            grant.vested_at(before),
            0,
            "vested_at({before}) should be 0 before cliff_end={cliff_end} for grant={grant:?}",
        );
    }
}
