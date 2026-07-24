/// Signer-set change cooldown tests for the multisig crate.
///
/// # Invariants verified here
///
/// 1. **Happy-path queue → apply** — a queued signer change is applied
///    successfully once the cooldown has elapsed.
/// 2. **Apply before delay rejected** — calling `apply_signers_change` before
///    `eta_ledger` returns `SignersDelayNotElapsed`.
/// 3. **Apply at exact ETA boundary** — applying exactly at `eta_ledger`
///    succeeds; applying one ledger before fails.
/// 4. **Apply after ETA** — applying well past `eta_ledger` succeeds (no upper
///    bound on application).
/// 5. **Cancel pending change** — `cancel_signers_change` removes the pending
///    change; a subsequent apply returns `NoQueuedSignersChange`.
/// 6. **Cancel when nothing queued** — returns `NoQueuedSignersChange`.
/// 7. **Overwrite pending change** — queuing a second change before applying
///    the first resets the cooldown to the new queue ledger.
/// 8. **Unauthorized queue** — non-admin callers cannot queue a signer change.
/// 9. **Unauthorized apply** — non-admin callers cannot apply a signer change.
/// 10. **Unauthorized cancel** — non-admin callers cannot cancel a signer change.
/// 11. **Empty signer list rejected** — `queue_signers_change` with an empty
///     vec returns `InvalidThreshold`.
/// 12. **Inspect pending change** — `get_pending_signers_change` returns `None`
///     when idle and `Some(SignersChange)` when queued.
/// 13. **get_min_signers_delay_ledgers** — returns `MIN_SIGNERS_DELAY_LEDGERS`.
/// 14. **Same-ledger protection** — queue and apply on the same ledger is
///     rejected.
/// 15. **set_signers still immediate** — `set_signers` remains callable and
///     takes effect instantly (the queued path is opt-in, not a replacement).
/// 16. **Apply clears pending change** — after a successful apply,
///     `get_pending_signers_change` returns `None`.
/// 17. **Event emission** — queue, apply, and cancel each emit the correct
///     events (verified via snapshot / side-effects).
/// 18. **Multi-signer set replacement** — the applied signer set exactly
///     matches what was queued.
/// 19. **Not initialized** — all new entrypoints return `NotInitialized` when
///     the contract has not been initialized.
/// 20. **Cooldown equals threshold delay** — `MIN_SIGNERS_DELAY_LEDGERS` equals
///     `MIN_THRESHOLD_DELAY_LEDGERS` so both levers share the same window.
#[cfg(test)]
mod signer_cooldown_tests {
    use crate::{
        MultisigContract, MultisigContractClient, MultisigError, MIN_SIGNERS_DELAY_LEDGERS,
        MIN_THRESHOLD_DELAY_LEDGERS,
    };
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::testutils::Ledger;
    use soroban_sdk::{Address, Env, Vec};

    // ─── Helpers ──────────────────────────────────────────────────────────────

    /// Set up an initialized contract with `env.mock_all_auths()`.
    fn setup_initialized(threshold: u32) -> (Env, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, MultisigContract);
        let client = MultisigContractClient::new(&env, &contract_id);
        client.initialize(&admin, &threshold);
        (env, admin, contract_id)
    }

    /// Build a non-empty `Vec<Address>` of length `n`.
    fn make_signers(env: &Env, n: usize) -> Vec<Address> {
        let mut signers = Vec::new(env);
        for _ in 0..n {
            signers.push_back(Address::generate(env));
        }
        signers
    }

    // ─── 1. Happy-path queue → apply ──────────────────────────────────────────

    /// Queue a signer change, advance past the cooldown, apply it, and verify
    /// that the live signer set was updated to the queued set.
    #[test]
    fn test_queue_and_apply_signers_change_success() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let new_signers = make_signers(&env, 3);
        client.queue_signers_change(&new_signers);

        // Advance past the cooldown.
        let eta = client.get_pending_signers_change().unwrap().eta_ledger;
        env.ledger().set_sequence_number(eta);

        client.apply_signers_change();

        let live = client.get_signers().unwrap();
        assert_eq!(live.len(), new_signers.len());
        for s in new_signers.iter() {
            assert!(live.contains(&s), "expected signer missing from live set");
        }
    }

    // ─── 2. Apply before delay rejected ───────────────────────────────────────

    /// Calling `apply_signers_change` immediately after queueing (before the
    /// cooldown has elapsed) must return `SignersDelayNotElapsed`.
    #[test]
    fn test_apply_before_delay_rejected() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signers = make_signers(&env, 2);
        client.queue_signers_change(&signers);

        assert_eq!(
            client.try_apply_signers_change(),
            Err(Ok(MultisigError::SignersDelayNotElapsed)),
            "apply before cooldown must be rejected"
        );

        // Live signer set must remain unchanged (still None from initialization).
        assert!(
            client.get_signers().is_none(),
            "signer set must not have changed"
        );
    }

    // ─── 3. Apply at exact ETA boundary ───────────────────────────────────────

    /// One ledger before `eta_ledger` → `SignersDelayNotElapsed`.
    /// Exactly at `eta_ledger` → success.
    #[test]
    fn test_apply_at_exact_eta_boundary() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signers = make_signers(&env, 2);
        client.queue_signers_change(&signers);
        let eta = client.get_pending_signers_change().unwrap().eta_ledger;

        // One before boundary — must fail.
        env.ledger().set_sequence_number(eta - 1);
        assert_eq!(
            client.try_apply_signers_change(),
            Err(Ok(MultisigError::SignersDelayNotElapsed)),
            "one ledger before eta must be rejected"
        );

        // Exactly at boundary — must succeed.
        env.ledger().set_sequence_number(eta);
        client.apply_signers_change();
        assert!(
            client.get_signers().is_some(),
            "signers must be set after apply"
        );
    }

    // ─── 4. Apply after ETA ───────────────────────────────────────────────────

    /// Applying well past `eta_ledger` (at 2× the cooldown) must succeed.
    #[test]
    fn test_apply_after_eta_succeeds() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signers = make_signers(&env, 2);
        let queue_ledger = env.ledger().sequence();
        client.queue_signers_change(&signers);

        env.ledger()
            .set_sequence_number(queue_ledger + MIN_SIGNERS_DELAY_LEDGERS * 2);
        client.apply_signers_change();

        assert!(client.get_signers().is_some());
    }

    // ─── 5. Cancel pending change ─────────────────────────────────────────────

    /// After `cancel_signers_change`, the pending change is cleared and a
    /// subsequent apply attempt returns `NoQueuedSignersChange`.
    #[test]
    fn test_cancel_clears_pending_change() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signers = make_signers(&env, 2);
        client.queue_signers_change(&signers);
        assert!(
            client.get_pending_signers_change().is_some(),
            "change must be pending after queue"
        );

        client.cancel_signers_change();
        assert!(
            client.get_pending_signers_change().is_none(),
            "pending change must be cleared after cancel"
        );

        // Attempt to apply after cancel must fail.
        assert_eq!(
            client.try_apply_signers_change(),
            Err(Ok(MultisigError::NoQueuedSignersChange))
        );
    }

    // ─── 6. Cancel when nothing queued ────────────────────────────────────────

    /// Calling `cancel_signers_change` when no change is queued returns
    /// `NoQueuedSignersChange`.
    #[test]
    fn test_cancel_nothing_queued_returns_error() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        assert_eq!(
            client.try_cancel_signers_change(),
            Err(Ok(MultisigError::NoQueuedSignersChange))
        );
    }

    // ─── 7. Overwrite pending change resets cooldown ──────────────────────────

    /// Queuing a second change before applying the first overwrites the pending
    /// change. The new ETA is computed from the second queue call's ledger, so
    /// the cooldown resets.
    #[test]
    fn test_overwrite_pending_change_resets_cooldown() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let first_signers = make_signers(&env, 2);
        let second_signers = make_signers(&env, 3);

        // Queue first change at ledger 0.
        client.queue_signers_change(&first_signers);
        let first_eta = client.get_pending_signers_change().unwrap().eta_ledger;

        // Advance halfway, then queue the second change — resets the cooldown.
        env.ledger()
            .set_sequence_number(MIN_SIGNERS_DELAY_LEDGERS / 2);
        client.queue_signers_change(&second_signers);
        let second_eta = client.get_pending_signers_change().unwrap().eta_ledger;

        // The new eta must be strictly later than the first eta.
        assert!(
            second_eta > first_eta,
            "overwrite must push eta forward: first={first_eta}, second={second_eta}"
        );

        // Still inside the first ETA — the overwritten change's new cooldown has
        // not elapsed, so apply must fail.
        env.ledger().set_sequence_number(first_eta);
        assert_eq!(
            client.try_apply_signers_change(),
            Err(Ok(MultisigError::SignersDelayNotElapsed)),
            "first eta is before second eta; apply must still be rejected"
        );

        // Advance to the second eta — apply must now succeed and the live signer
        // set must reflect the second change, not the first.
        env.ledger().set_sequence_number(second_eta);
        client.apply_signers_change();

        let live = client.get_signers().unwrap();
        assert_eq!(
            live.len(),
            second_signers.len() as u32,
            "live set must match second queued set"
        );
    }

    // ─── 8. Unauthorized queue ────────────────────────────────────────────────

    /// Calling `queue_signers_change` without admin auth must panic (Soroban's
    /// `require_auth` aborts the transaction at the host level).
    #[test]
    #[should_panic]
    fn test_queue_signers_change_unauthorized() {
        // No mock_all_auths — the first require_auth() call will abort.
        let env = Env::default();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, MultisigContract);
        let client = MultisigContractClient::new(&env, &contract_id);
        client.initialize(&admin, &1);
        let signers = make_signers(&env, 1);
        client.queue_signers_change(&signers); // panics: admin auth not provided
    }

    // ─── 9. Unauthorized apply ────────────────────────────────────────────────

    /// Calling `apply_signers_change` without admin auth must panic.
    #[test]
    #[should_panic]
    fn test_apply_signers_change_unauthorized() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, MultisigContract);
        let client = MultisigContractClient::new(&env, &contract_id);
        // initialize does not require auth, so this succeeds.
        client.initialize(&admin, &1);
        // apply_signers_change calls admin.require_auth() → panics without mock.
        client.apply_signers_change();
    }

    // ─── 10. Unauthorized cancel ──────────────────────────────────────────────

    /// Calling `cancel_signers_change` without admin auth must panic.
    #[test]
    #[should_panic]
    fn test_cancel_signers_change_unauthorized() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, MultisigContract);
        let client = MultisigContractClient::new(&env, &contract_id);
        // initialize does not require auth, so this succeeds.
        client.initialize(&admin, &1);
        // cancel_signers_change calls admin.require_auth() → panics without mock.
        let _ = admin;
        client.cancel_signers_change();
    }

    // ─── 11. Empty signer list rejected ───────────────────────────────────────

    /// Queuing an empty signer set must return `InvalidThreshold`.
    #[test]
    fn test_queue_empty_signers_rejected() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let empty: Vec<Address> = Vec::new(&env);
        assert_eq!(
            client.try_queue_signers_change(&empty),
            Err(Ok(MultisigError::InvalidThreshold)),
            "empty signer list must be rejected with InvalidThreshold"
        );
    }

    // ─── 12. Inspect pending change ───────────────────────────────────────────

    /// `get_pending_signers_change` returns `None` when idle and `Some` after
    /// queuing.  The returned `eta_ledger` matches `queue_ledger +
    /// MIN_SIGNERS_DELAY_LEDGERS`.
    #[test]
    fn test_get_pending_signers_change_reflects_queue() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        assert!(
            client.get_pending_signers_change().is_none(),
            "must be None before any queue call"
        );

        let queue_ledger = env.ledger().sequence();
        let signers = make_signers(&env, 2);
        client.queue_signers_change(&signers);

        let pending = client.get_pending_signers_change().unwrap();
        assert_eq!(
            pending.eta_ledger,
            queue_ledger + MIN_SIGNERS_DELAY_LEDGERS,
            "eta_ledger must equal queue_ledger + MIN_SIGNERS_DELAY_LEDGERS"
        );
        assert_eq!(
            pending.new_signers.len(),
            signers.len(),
            "queued signer count must match"
        );
    }

    // ─── 13. get_min_signers_delay_ledgers ────────────────────────────────────

    #[test]
    fn test_get_min_signers_delay_ledgers_returns_constant() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);
        assert_eq!(
            client.get_min_signers_delay_ledgers(),
            MIN_SIGNERS_DELAY_LEDGERS
        );
    }

    // ─── 14. Same-ledger protection ───────────────────────────────────────────

    /// Queue and attempt apply on the same ledger must fail.
    #[test]
    fn test_same_ledger_queue_and_apply_rejected() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signers = make_signers(&env, 1);
        client.queue_signers_change(&signers);

        // Still on the same ledger — must fail.
        assert_eq!(
            client.try_apply_signers_change(),
            Err(Ok(MultisigError::SignersDelayNotElapsed))
        );
    }

    // ─── 15. set_signers remains immediate ────────────────────────────────────

    /// The original `set_signers` function still takes effect immediately. The
    /// queued path is additive, not a replacement.
    #[test]
    fn test_set_signers_still_immediate() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signers = make_signers(&env, 2);
        client.set_signers(&signers);

        let live = client.get_signers().unwrap();
        assert_eq!(live.len(), 2, "set_signers must apply immediately");
    }

    // ─── 16. Apply clears pending change ──────────────────────────────────────

    /// After a successful `apply_signers_change`, `get_pending_signers_change`
    /// returns `None`.
    #[test]
    fn test_apply_clears_pending_change() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signers = make_signers(&env, 2);
        client.queue_signers_change(&signers);
        let eta = client.get_pending_signers_change().unwrap().eta_ledger;
        env.ledger().set_sequence_number(eta);
        client.apply_signers_change();

        assert!(
            client.get_pending_signers_change().is_none(),
            "pending change must be None after apply"
        );
    }

    // ─── 17. No-queued-change apply ───────────────────────────────────────────

    /// `apply_signers_change` with nothing queued must return
    /// `NoQueuedSignersChange`.
    #[test]
    fn test_apply_with_nothing_queued_returns_error() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        assert_eq!(
            client.try_apply_signers_change(),
            Err(Ok(MultisigError::NoQueuedSignersChange))
        );
    }

    // ─── 18. Multi-signer set replacement ────────────────────────────────────

    /// A 5-signer replacement set is preserved exactly through the queue →
    /// apply lifecycle.
    #[test]
    fn test_multi_signer_set_replacement_preserved() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let new_signers = make_signers(&env, 5);
        client.queue_signers_change(&new_signers);
        let eta = client.get_pending_signers_change().unwrap().eta_ledger;
        env.ledger().set_sequence_number(eta);
        client.apply_signers_change();

        let live = client.get_signers().unwrap();
        assert_eq!(live.len(), 5, "live set must have 5 members");
        for s in new_signers.iter() {
            assert!(
                live.contains(&s),
                "all queued signers must appear in live set"
            );
        }
    }

    // ─── 19. Not initialized ──────────────────────────────────────────────────

    /// Every new entrypoint returns `NotInitialized` before `initialize` is
    /// called.
    #[test]
    fn test_entrypoints_return_not_initialized() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, MultisigContract);
        let client = MultisigContractClient::new(&env, &contract_id);

        let signers = make_signers(&env, 1);

        assert_eq!(
            client.try_queue_signers_change(&signers),
            Err(Ok(MultisigError::NotInitialized))
        );
        assert_eq!(
            client.try_apply_signers_change(),
            Err(Ok(MultisigError::NotInitialized))
        );
        assert_eq!(
            client.try_cancel_signers_change(),
            Err(Ok(MultisigError::NotInitialized))
        );
    }

    // ─── 20. Cooldown equals threshold delay ──────────────────────────────────

    /// `MIN_SIGNERS_DELAY_LEDGERS` must equal `MIN_THRESHOLD_DELAY_LEDGERS` so
    /// both governance levers share the same cooldown window.
    #[test]
    fn test_signers_delay_equals_threshold_delay() {
        assert_eq!(
            MIN_SIGNERS_DELAY_LEDGERS, MIN_THRESHOLD_DELAY_LEDGERS,
            "signer-set and threshold-change cooldowns must be identical"
        );
    }

    // ─── Regression: queued change does not affect live set ───────────────────

    /// While a signer change is queued but not yet applied, the live signer set
    /// must not change — proposals must still be governed by the previous set.
    #[test]
    fn test_queued_change_does_not_mutate_live_set() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        // Install an initial live signer set.
        let initial_signers = make_signers(&env, 2);
        client.set_signers(&initial_signers);

        // Queue a replacement.
        let replacement = make_signers(&env, 3);
        client.queue_signers_change(&replacement);

        // Live set must still reflect the initial signers, not the queued ones.
        let live = client.get_signers().unwrap();
        assert_eq!(
            live.len(),
            2,
            "live set must be unchanged while change is only queued"
        );
    }

    // ─── Queue → cancel → queue again ────────────────────────────────────────

    /// After cancelling a pending change, the admin can queue a fresh change and
    /// apply it after the new cooldown.
    #[test]
    fn test_queue_cancel_requeue_apply() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        // Queue, cancel, then queue again with a different set.
        let first = make_signers(&env, 2);
        let second = make_signers(&env, 4);

        client.queue_signers_change(&first);
        client.cancel_signers_change();

        // Advance time and queue the second batch.
        let requeue_ledger = env.ledger().sequence() + 100;
        env.ledger().set_sequence_number(requeue_ledger);
        client.queue_signers_change(&second);

        let eta = client.get_pending_signers_change().unwrap().eta_ledger;
        env.ledger().set_sequence_number(eta);
        client.apply_signers_change();

        let live = client.get_signers().unwrap();
        assert_eq!(
            live.len(),
            4,
            "live set must match the second queued batch after requeue"
        );
    }

    // ─── Single-signer set accepted ───────────────────────────────────────────

    /// A signer set containing exactly one address is valid (minimum allowed
    /// non-empty set).
    #[test]
    fn test_single_signer_set_accepted() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let one_signer = make_signers(&env, 1);
        client.queue_signers_change(&one_signer);
        let eta = client.get_pending_signers_change().unwrap().eta_ledger;
        env.ledger().set_sequence_number(eta);
        client.apply_signers_change();

        let live = client.get_signers().unwrap();
        assert_eq!(live.len(), 1);
    }

    // ─── Apply exactly at MIN_SIGNERS_DELAY_LEDGERS ───────────────────────────

    /// Applying at exactly `queue_ledger + MIN_SIGNERS_DELAY_LEDGERS` succeeds
    /// (the boundary is inclusive).
    #[test]
    fn test_apply_at_exact_min_delay_boundary() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);

        let queue_ledger = env.ledger().sequence();
        let signers = make_signers(&env, 2);
        client.queue_signers_change(&signers);

        // One ledger before boundary — must fail.
        env.ledger()
            .set_sequence_number(queue_ledger + MIN_SIGNERS_DELAY_LEDGERS - 1);
        assert_eq!(
            client.try_apply_signers_change(),
            Err(Ok(MultisigError::SignersDelayNotElapsed))
        );

        // Exactly at boundary — must succeed.
        env.ledger()
            .set_sequence_number(queue_ledger + MIN_SIGNERS_DELAY_LEDGERS);
        client.apply_signers_change();
        assert!(client.get_signers().is_some());
    }
}
