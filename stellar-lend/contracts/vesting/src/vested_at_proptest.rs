use proptest::prelude::*;

use crate::test_harness::Grant;

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

#[cfg(test)]
mod contract_grant_properties {
    use super::*;
    use crate::{Grant as ContractGrant, VestingSchedule};
    use soroban_sdk::{Env, Vec};

    const MAX_CONTRACT_TIME: u64 = 1_000_000_000;
    const MAX_CONTRACT_PRINCIPAL: i128 = 1_000_000_000_000_000;

    fn contract_grant_fields() -> impl Strategy<Value = (i128, i128, u64, u64, u64, bool, bool)> {
        (
            2i128..=MAX_CONTRACT_PRINCIPAL,
            any::<u64>(),
            0..=MAX_CONTRACT_TIME,
            2..=MAX_CONTRACT_TIME,
            0..=MAX_CONTRACT_TIME,
            any::<bool>(),
            any::<bool>(),
        )
            .prop_map(
                |(total, claimed_seed, start_ts, duration_secs, cliff_seed, revoked, milestone)| {
                    let claimed_amount = (claimed_seed as u128 % (total as u128 + 1)) as i128;
                    let cliff_secs = cliff_seed % (duration_secs + 1);
                    (
                        total,
                        claimed_amount,
                        start_ts,
                        duration_secs,
                        cliff_secs,
                        revoked,
                        milestone,
                    )
                },
            )
    }

    fn make_contract_grant(
        env: &Env,
        fields: (i128, i128, u64, u64, u64, bool, bool),
    ) -> ContractGrant {
        let (total_amount, claimed_amount, start_ts, duration_secs, cliff_secs, revoked, milestone) =
            fields;
        let schedule = if milestone {
            let mut milestones = Vec::new(env);
            milestones.push_back((start_ts + duration_secs / 2, total_amount / 2));
            milestones.push_back((start_ts + duration_secs, total_amount));
            VestingSchedule::Milestone(milestones)
        } else {
            VestingSchedule::Linear
        };

        ContractGrant {
            grantee: soroban_sdk::Address::generate(env),
            total_amount,
            claimed_amount,
            start_ts,
            cliff_secs,
            duration_secs,
            revoked,
            schedule,
        }
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        #[test]
        fn prop_vested_never_exceeds_total(
            fields in contract_grant_fields(),
            now in 0..=MAX_CONTRACT_TIME * 2,
        ) {
            let env = Env::default();
            let grant = make_contract_grant(&env, fields);
            prop_assert!(grant.vested_at(now) <= grant.total_amount);
        }

        #[test]
        fn prop_vested_monotone_contract(
            fields in contract_grant_fields(),
            t1 in 0..=MAX_CONTRACT_TIME * 2,
            t2 in 0..=MAX_CONTRACT_TIME * 2,
        ) {
            prop_assume!(t1 <= t2);
            let env = Env::default();
            let grant = make_contract_grant(&env, fields);
            prop_assert!(grant.vested_at(t1) <= grant.vested_at(t2));
        }

        #[test]
        fn prop_revoked_claimable_is_remainder(
            total_amount in 2i128..=MAX_CONTRACT_PRINCIPAL,
            claimed_seed in any::<u64>(),
            start_ts in 0..=MAX_CONTRACT_TIME,
            duration_secs in 2..=MAX_CONTRACT_TIME,
            now in 0..=MAX_CONTRACT_TIME * 2,
        ) {
            let claimed_amount = (claimed_seed as u128 % (total_amount as u128 + 1)) as i128;
            let env = Env::default();
            let grant = ContractGrant {
                grantee: soroban_sdk::Address::generate(&env),
                total_amount,
                claimed_amount,
                start_ts,
                cliff_secs: 0,
                duration_secs,
                revoked: true,
                schedule: VestingSchedule::Linear,
            };
            prop_assert_eq!(grant.claimable_at(now), total_amount - claimed_amount);
        }
    }
}
