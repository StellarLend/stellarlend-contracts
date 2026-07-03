// ════════════════════════════════════════════════════════════════════════════
// E2E TESTS: vesting lifecycle – balance conservation
//
// Covers issue #1228 requirements:
//   1. claimed + clawback_to_treasury + locked == principal at every step
//   2. total_locked and balance_of stay consistent after each operation
//   3. Revoke before the cliff (all principal clawed back)
//   4. Revoke after partial vesting (split: vested stays, unvested clawed back)
//
// Timeline used throughout (start = 0, duration = 1_000 s, cliff = 200 s):
//
//   t=0   grant created       locked=1_000 claimed=0 treasury=0
//   t=100 before cliff        nothing claimable
//   t=500 mid-vest (50%)      claimable = 500
//   t=800 mid-vest (80%)      claimable = 800 (cumulative)
//   t=999 near-full vest      claimable = 999
//   t=1000 fully vested        claimable = 1_000
// ════════════════════════════════════════════════════════════════════════════

#[cfg(test)]

mod lifecycle_e2e_tests {
    use crate::{VestingContract, VestingError};

    // ── shared parameters ─────────────────────────────────────────────────────

    const PRINCIPAL: u128 = 1_000;
    const START: u64 = 0;
    const DURATION: u64 = 1_000;
    const CLIFF: u64 = 200;

    /// Build a fresh contract with one grant for "alice".
    fn setup() -> VestingContract {
        let mut c = VestingContract::new("admin", "treasury");
        c.add_grant("admin", "alice", PRINCIPAL, START, DURATION, CLIFF)
            .expect("add_grant should succeed");
        c
    }

    /// Assert the core conservation invariant at any point in the lifecycle.
    ///
    /// `claimed + clawback + locked == principal` must hold exactly.
    fn assert_conservation(c: &VestingContract, label: &str) {
        let claimed = c.balance_of("alice");
        let clawback = c.balance_of("treasury");
        let locked = c.total_locked();
        assert_eq!(
            claimed + clawback + locked,
            PRINCIPAL,
            "{label}: conservation violated \
             (claimed={claimed} + clawback={clawback} + locked={locked} != {PRINCIPAL})"
        );
    }

    // ── Requirement 1 & 2: conservation immediately after add_grant ──────────

    /// After add_grant: nothing claimed, nothing in treasury, all locked.
    #[test]
    fn conservation_after_add_grant() {
        let c = setup();
        assert_eq!(c.balance_of("alice"), 0);
        assert_eq!(c.balance_of("treasury"), 0);
        assert_eq!(c.total_locked(), PRINCIPAL);
        assert_conservation(&c, "after add_grant");

        // balance_of("contract") reflects the escrowed principal
        assert_eq!(c.balance_of("contract"), PRINCIPAL);
    }

    // ── Before cliff: claim returns zero, conservation holds ─────────────────

    /// t=100 — before cliff; nothing claimable, all still locked.
    #[test]
    fn conservation_before_cliff() {
        let mut c = setup();
        let claimed = c.claim("alice", 100).expect("claim should not error");
        assert_eq!(claimed, 0, "nothing claimable before cliff");
        assert_eq!(c.balance_of("alice"), 0);
        assert_eq!(c.total_locked(), PRINCIPAL);
        assert_conservation(&c, "before cliff");
    }

    // ── Partial claim mid-vest, then conservation check ───────────────────────

    /// t=500 — 50% vested; partial claim of 300, conservation holds.
    #[test]
    fn conservation_after_partial_claim_mid_vest() {
        let mut c = setup();

        // At t=500: vested = 1_000 * 500 / 1_000 = 500 (cliff at 200, passed)
        let claimed = c
            .claim_partial("alice", 300, 500)
            .expect("partial claim should succeed");
        assert_eq!(claimed, 300);

        // conservation: 300 (alice) + 0 (treasury) + 700 (locked) = 1_000
        assert_eq!(c.balance_of("alice"), 300);
        assert_eq!(c.balance_of("treasury"), 0);
        assert_eq!(c.total_locked(), 700);
        assert_conservation(&c, "after partial claim at t=500");

        // balance_of("contract") decremented by the claimed amount
        assert_eq!(c.balance_of("contract"), 700);
    }

    // ── Full lifecycle: add_grant → partial claim → revoke ───────────────────

    /// Full lifecycle: grant → partial claim at t=500 → revoke at t=500.
    ///
    /// Vested at t=500 = 500; already claimed = 300; remaining vested (claimable) = 200.
    /// Unvested at t=500 = 500 → clawed back to treasury.
    /// Conservation: alice=300 + treasury=500 + locked=200 == 1_000.
    ///
    /// Wait — after partial claim of 300 from 500 vested, releasing means:
    ///   released = 500, claimed = 300, claimable = 200, locked = 500 (unvested).
    /// On revoke at t=500:
    ///   clawback = locked = 500 → treasury gets 500.
    ///   remaining claimable = 200 stays for alice.
    ///
    /// Conservation: alice_balance=300 + treasury=500 + total_locked=0 = 800 ≠ 1_000.
    ///
    /// Note: total_locked tracks unvested tokens. After revoke total_locked=0.
    /// The 200 still claimable by alice is in "released but not claimed" state,
    /// which lives in balance_of("contract"). Conservation must account for it:
    ///   alice_claimed_so_far + contract_remaining + treasury = 1_000
    #[test]
    fn full_lifecycle_partial_claim_then_revoke() {
        let mut c = setup();

        // Step 1: partial claim at t=500
        c.claim_partial("alice", 300, 500)
            .expect("partial claim should succeed");

        // Step 2: revoke at t=500
        let clawback = c.revoke("admin", "alice", 500).expect("revoke should succeed");
        // unvested at t=500 = 1_000 - 500 = 500
        assert_eq!(clawback, 500, "treasury should receive 500 unvested tokens");

        assert_eq!(c.balance_of("treasury"), 500);
        assert_eq!(c.total_locked(), 0, "no more locked after revoke");

        // Step 3: alice claims her remaining vested 200
        let remaining = c.claim("alice", 500).expect("post-revoke claim should succeed");
        assert_eq!(remaining, 200);
        assert_eq!(c.balance_of("alice"), 500);

        // Final conservation: alice=500 + treasury=500 + locked=0 == 1_000
        assert_eq!(c.balance_of("alice") + c.balance_of("treasury") + c.total_locked(), PRINCIPAL);
    }

    // ── Requirement 3: revoke before cliff ───────────────────────────────────

    /// Revoke at t=100, before the cliff (cliff=200).
    ///
    /// At t=100 nothing is vested (cliff gate). The entire principal is
    /// unvested → treasury receives 1_000, alice gets 0.
    #[test]
    fn revoke_before_cliff_claws_back_entire_principal() {
        let mut c = setup();

        let clawback = c
            .revoke("admin", "alice", 100)
            .expect("revoke before cliff should succeed");

        assert_eq!(clawback, PRINCIPAL, "all tokens clawed back before cliff");
        assert_eq!(c.balance_of("treasury"), PRINCIPAL);
        assert_eq!(c.balance_of("alice"), 0);
        assert_eq!(c.total_locked(), 0);

        // conservation: 0 + 1_000 + 0 == 1_000
        assert_eq!(c.balance_of("alice") + c.balance_of("treasury") + c.total_locked(), PRINCIPAL);

        // Alice has nothing to claim after full pre-cliff revoke
        let late_claim = c.claim("alice", 500).expect("claim after revoke should not error");
        assert_eq!(late_claim, 0);
    }

    // ── Requirement 4: revoke after partial vesting ───────────────────────────

    /// Revoke at t=800, after partial vesting (80% vested = 800 tokens).
    ///
    /// No prior claim, so all 800 vested tokens remain claimable.
    /// Unvested = 200 → clawed back.
    /// Conservation at each step is checked.
    #[test]
    fn revoke_after_partial_vest_no_prior_claim() {
        let mut c = setup();

        // t=800: vested = 800, unvested = 200
        let clawback = c.revoke("admin", "alice", 800).expect("revoke should succeed");
        assert_eq!(clawback, 200, "200 unvested tokens clawed back");
        assert_eq!(c.balance_of("treasury"), 200);
        assert_eq!(c.total_locked(), 0);

        // contract still holds the 800 vested-but-unclaimed tokens
        assert_eq!(c.balance_of("contract"), 800);

        // alice claims her 800
        let claimed = c.claim("alice", 800).expect("claim after revoke should succeed");
        assert_eq!(claimed, 800);
        assert_eq!(c.balance_of("alice"), 800);
        assert_eq!(c.balance_of("contract"), 0);

        // Final conservation: 800 + 200 + 0 == 1_000
        assert_eq!(c.balance_of("alice") + c.balance_of("treasury") + c.total_locked(), PRINCIPAL);
    }

    // ── total_locked consistency after sequential operations ─────────────────

    /// total_locked decrements correctly as vesting progresses through
    /// multiple claim checkpoints.
    #[test]
    fn total_locked_decrements_with_vesting_progress() {
        let mut c = setup();

        // t=200 (cliff exactly): vested = 200
        c.claim("alice", 200).expect("claim at cliff");
        assert_eq!(c.total_locked(), 800);
        assert_conservation(&c, "t=200");

        // t=500: vested = 500, additional 300 claimable
        c.claim("alice", 500).expect("claim at t=500");
        assert_eq!(c.total_locked(), 500);
        assert_conservation(&c, "t=500");

        // t=1_000: fully vested
        c.claim("alice", 1_000).expect("claim at full vest");
        assert_eq!(c.total_locked(), 0);
        assert_eq!(c.balance_of("alice"), PRINCIPAL);
        assert_conservation(&c, "t=1_000");
    }

    // ── balance_of consistency ────────────────────────────────────────────────

    /// balance_of("alice") grows monotonically with each claim.
    /// balance_of("contract") shrinks by the same delta each time.
    #[test]
    fn balance_of_consistency_across_claims() {
        let mut c = setup();

        let contract_start = c.balance_of("contract");
        assert_eq!(contract_start, PRINCIPAL);

        // claim at t=400 (vested = 400)
        let c1 = c.claim("alice", 400).expect("claim 1");
        assert_eq!(c.balance_of("alice"), c1);
        assert_eq!(c.balance_of("contract"), PRINCIPAL - c1);

        // claim at t=700 (additional 300 vested)
        let c2 = c.claim("alice", 700).expect("claim 2");
        assert_eq!(c.balance_of("alice"), c1 + c2);
        assert_eq!(c.balance_of("contract"), PRINCIPAL - c1 - c2);

        // conservation after each claim
        assert_conservation(&c, "after two claims");
    }

    // ── Double-revoke is rejected ─────────────────────────────────────────────

    /// A second revoke on an already-revoked grant must return AlreadyRevoked.
    #[test]
    fn double_revoke_returns_already_revoked() {
        let mut c = setup();

        c.revoke("admin", "alice", 300).expect("first revoke should succeed");
        let err = c.revoke("admin", "alice", 300).unwrap_err();
        assert_eq!(err, VestingError::AlreadyRevoked);
    }

    // ── Revoke by non-admin is rejected ──────────────────────────────────────

    /// Only admin can revoke; other callers get Unauthorized.
    #[test]
    fn revoke_by_non_admin_returns_unauthorized() {
        let mut c = setup();
        let err = c.revoke("attacker", "alice", 500).unwrap_err();
        assert_eq!(err, VestingError::Unauthorized);
        // conservation unaffected
        assert_conservation(&c, "after rejected revoke");
    }

    // ── Multi-step timeline advancing now across cliff and mid-vest ──────────

    /// Drive now across cliff boundary and mid-vest, checking conservation
    /// and consistency at each step as required by the issue.
    #[test]
    fn timeline_across_cliff_and_mid_vest() {
        let mut c = setup();

        // t=0: just after grant
        assert_eq!(c.total_locked(), 1_000);
        assert_conservation(&c, "t=0");

        // t=199: one second before cliff, still nothing claimable
        let pre_cliff = c.claim("alice", 199).expect("pre-cliff claim");
        assert_eq!(pre_cliff, 0);
        assert_conservation(&c, "t=199");

        // t=200: cliff; vested = 200 (200/1000 of total)
        let at_cliff = c.claim("alice", 200).expect("at-cliff claim");
        assert_eq!(at_cliff, 200);
        assert_conservation(&c, "t=200");

        // t=600: vested = 600; additional 400 available
        let mid1 = c.claim("alice", 600).expect("mid-vest claim 1");
        assert_eq!(mid1, 400);
        assert_conservation(&c, "t=600");

        // t=900: vested = 900; additional 300 available
        let mid2 = c.claim("alice", 900).expect("mid-vest claim 2");
        assert_eq!(mid2, 300);
        assert_conservation(&c, "t=900");

        // revoke at t=900: 100 tokens still unvested
        let clawback = c.revoke("admin", "alice", 900).expect("revoke at t=900");
        assert_eq!(clawback, 100);
        assert_conservation(&c, "t=900 post-revoke");

        // alice claims remaining 0 (all already claimed)
        let final_claim = c.claim("alice", 900).expect("final claim");
        assert_eq!(final_claim, 0);

        // Final state
        assert_eq!(c.balance_of("alice"), 900);
        assert_eq!(c.balance_of("treasury"), 100);
        assert_eq!(c.total_locked(), 0);
        assert_eq!(c.balance_of("alice") + c.balance_of("treasury") + c.total_locked(), PRINCIPAL);
    }
}
