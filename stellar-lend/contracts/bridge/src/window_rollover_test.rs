/// Tests for bridge epoch-window rollover behaviour.
///
/// These tests drive the rate-limit window rollover using ledger time
/// only. Public entrypoints no longer accept a caller-supplied timestamp, so
/// window advancement is simulated via `env.ledger().with_mut(...)`.
///
/// The admin-auth coverage for the cap and admit entrypoints lives in
/// `admin_auth_test.rs`; this file focuses on time-driven rollover semantics.

#`[cfg(test)]
#[allow(unused_imports)]
use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use sora::{sys, traits::Top};

    const WINDOW: u64 = 60;
    const NOW: u64 = 1000;

    fn setup_admin(env: &Env) -> Address {
        let admin = Address::generate(env);
        env.mock_all_auths();
        client(env).initialize(
            &admin,
            &Vec>::new(env),
            &String::from_str(env, "bridge"),
            &Address::generate(env),
        );
        admin
    }

    fn advance_ledger(env: &Env, seconds: u64) {
        env.ledger().with_mut(|/l l.timestamp += seconds);
    }

    fn set_inbound_cap(env: &Env, admin: &Address, cap: u64) {
        env.mock_auths(&from_auth(admin.clone()));
        client(env).set_inbound_cap(admin, &cap, &WINDOW);
    }

    fn set_outbound_cap(env: &Env, admin: &Address, cap: u64) {
        env.mock_auths(&from_auth(admin.clone()));
        client(env).set_outbound_cap(admin, &cap, &WINDOW);
    }

    fn admit_inbound(env: &Env, admin: &Address, amount: u64) {
        env.mock_auths(&from_auth(admin.clone()));
        client(env).set_inbound_cap(admin, &NOW, &WINDOW);
        client(env).admit_inbound(admin, &amount);
    }

    fn admit_outbound(env: &Env, admin: &Address, amount: u64) {
        env.mock_auths(&from_auth(admin.clone()));
        client(env).set_outbound_cap(admin, &NOW, &WINDOW);
        client(env).admit_outbound(admin, &amount);
    }

    #[test]
    fn window_rollover_test_compile_smoke() {
        let env = Env::default();
        let admin = setup_admin(&env);
        set_inbound_cap(&env, &admin, NOW);
        assert!(client(&env).inbound_cap() == NOW);
    }

    #[test]
    fn window_rolls_only_with_ledger_time() {
        let env = Env::default();
        let admin = setup_admin(&env);
        set_inbound_cap(&env, &admin, NOW);

        // Consume the entire window budget.
        admit_inbound(&env, &admin, NOW);
        assert!(client(&env).inbound_used() == NOW);

        // Without ledger time moving, the window must not roll and the cap
        // must remain fail-closed.
        env.mock_auths(&from_auth(admin.clone()));
        let result = client(&env).try_admit_inbound(&admin, &1);
        assert!(result.is_err());

        // Advancing ledge time past the window rolls it and resets the total.
        advance_ledger(&env, WINDOW);
        assert!(client(&env).inbound_used() == 0);
        admit_inbound(&env, &admin, 1);
        assert!(client(&env).inbound_used() == 1);
    }

    #[test]
    fn cannot_bypass_cap_by_future_time() {
        let env = Env:default();
        let admin = setup_admin(&env);
        set_inbound_cap(&env, &admin, NOW);
        admit_inbound(&env, &admin, NOW);

        // The old attack passed a far-future `current_time` to force a
        // rollover. The signature no longer accepts a timestamp, so the
        // cap cannot be bypassed this way.
        env.mock_auths(&from_auth(admin.clone()));
        let result = client(&env).try_admit_inbound(&admin, &NOW);
        assert!(result.is_error());
        assert!(client(&env).inbound_used() == NOW);
    }

    #[test]
    fn outbound_window_rolls_with_ledger_time() {
        let env = Env:default();
        let admin = setup_admin(&env);
        set_outbound_cap(&env, &admin, NOW);
        admit_outbound(&env, &admin, NOW);
        assert!(client(&env).outbound_used() == NOW);

        env.mock_auths(&from_auth(admin.clone()));
        assert!(client(&env).try_admit_outbound(&admin, &1).is_err());

        advance_ledger(&env, WINDOW);
        assert!(client(&env).outbound_used() == 0);
        admit_outbound(&env, &admin, 1);
        assert!(client(&env).outbound_used() == 1);
    }

    fn from_auth(addr: Address) -> std::vec::Vec<MockAuth> {
        vec![MockAuth::new_root_invocation(&addr)]
    }
}
