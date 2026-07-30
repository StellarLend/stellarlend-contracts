//! Integration tests for recovery.rs.
//!
//! Tests are driven through the contract's public entrypoints
//! (`set_guardians`, `start_recovery`, `approve_recovery`, `execute_recovery`)
//! so the exact same code paths exercised in production are exercised here.

#[cfg(test)]
mod tests {
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{Address, Env, Vec};

    use crate::governance::GovernanceError;
    use crate::{HelloContract, HelloContractClient};

    // -----------------------------------------------------------------------
    // Test harness
    // -----------------------------------------------------------------------

    /// Register and initialise the contract, returning `(env, contract_id,
    /// admin)`.  Governance is **not** initialised so we can test the
    /// recovery module in isolation.
    fn setup() -> (Env, soroban_sdk::Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(HelloContract, ());
        let admin = Address::generate(&env);
        // `initialize` sets the admin and risk-management state.
        let client = HelloContractClient::new(&env, &contract_id);
        client.initialize(&admin);
        (env, contract_id, admin)
    }

    // Helper: build a Vec<Address> of `n` fresh addresses.
    fn make_guardians(env: &Env, n: usize) -> Vec<Address> {
        let mut v: Vec<Address> = Vec::new(env);
        for _ in 0..n {
            v.push_back(Address::generate(env));
        }
        v
    }

    // -----------------------------------------------------------------------
    // set_guardians
    // -----------------------------------------------------------------------

    #[test]
    fn set_guardians_happy_path() {
        let (env, contract_id, admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);

        let guardians = make_guardians(&env, 3);
        let result = client.try_set_guardians(&admin, &guardians, &2);
        assert!(result.is_ok(), "set_guardians should succeed for admin");
    }

    #[test]
    fn set_guardians_stores_config() {
        let (env, contract_id, admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);

        let guardians = make_guardians(&env, 2);
        client.set_guardians(&admin, &guardians, &2);

        // Retrieve via governance query.
        let gc = client.gov_get_guardian_config().expect("guardian config must be set");
        assert_eq!(gc.threshold, 2);
        assert_eq!(gc.guardians.len(), 2);
    }

    #[test]
    fn set_guardians_replaces_previous_config() {
        let (env, contract_id, admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);

        let first = make_guardians(&env, 3);
        client.set_guardians(&admin, &first, &2);

        let second = make_guardians(&env, 1);
        client.set_guardians(&admin, &second, &1);

        let gc = client.gov_get_guardian_config().unwrap();
        assert_eq!(gc.guardians.len(), 1, "old guardians should be replaced");
        assert_eq!(gc.threshold, 1);
    }

    #[test]
    fn set_guardians_rejects_non_admin() {
        let (env, contract_id, _admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);

        let stranger = Address::generate(&env);
        let guardians = make_guardians(&env, 2);
        let result = client.try_set_guardians(&stranger, &guardians, &1);
        assert!(
            matches!(result, Err(Ok(GovernanceError::Unauthorized))),
            "non-admin caller must be rejected, got {:?}",
            result
        );
    }

    #[test]
    fn set_guardians_rejects_threshold_zero() {
        let (env, contract_id, admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);

        let guardians = make_guardians(&env, 2);
        let result = client.try_set_guardians(&admin, &guardians, &0);
        assert!(
            matches!(result, Err(Ok(GovernanceError::InvalidConfig))),
            "threshold=0 must be rejected, got {:?}",
            result
        );
    }

    #[test]
    fn set_guardians_rejects_threshold_exceeds_guardian_count() {
        let (env, contract_id, admin) = setup();
        let client = HelloContractClient::new(&env, &contract_id);

        let guardians = make_guardians(&env, 2);
        let result = client.try_set_guardians(&admin, &guardians, &3); // 3 > 2
        assert!(
            matches!(result, Err(Ok(GovernanceError::InvalidConfig))),
            "threshold > guardian count must be rejected, got {:?}",
            result
        );
    }

    // -----------------------------------------------------------------------
    // start_recovery
    // -----------------------------------------------------------------------

    /// Helper that sets up guardians and returns the first guardian address.
    fn setup_with_guardians(
        env: &Env,
        contract_id: &Address,
        admin: &Address,
        count: usize,
        threshold: u32,
    ) -> Vec<Address> {
        let client = HelloContractClient::new(env, contract_id);
        let guardians = make_guardians(env, count);
        client.set_guardians(admin, &guardians, &threshold);
        guardians
    }

    #[test]
    fn start_recovery_happy_path() {
        let (env, contract_id, admin) = setup();
        let guardians = setup_with_guardians(&env, &contract_id, &admin, 3, 2);
        let client = HelloContractClient::new(&env, &contract_id);

        let new_admin = Address::generate(&env);
        let result = client.try_start_recovery(&guardians.get(0).unwrap(), &admin, &new_admin);
        assert!(result.is_ok(), "guardian should be able to start recovery");
    }

    #[test]
    fn start_recovery_creates_request_with_initiator_as_first_approval() {
        let (env, contract_id, admin) = setup();
        let guardians = setup_with_guardians(&env, &contract_id, &admin, 3, 2);
        let client = HelloContractClient::new(&env, &contract_id);

        let initiator = guardians.get(0).unwrap();
        let new_admin = Address::generate(&env);
        client.start_recovery(&initiator, &admin, &new_admin);

        let req = client.gov_get_recovery_request().expect("request must exist");
        assert_eq!(req.new_admin, new_admin);
        assert_eq!(req.old_admin, admin);
        assert_eq!(req.approval_count, 1);

        let approvals = client.gov_get_recovery_approvals().expect("approvals must exist");
        assert_eq!(approvals.len(), 1);
        assert_eq!(approvals.get(0).unwrap(), initiator);
    }

    #[test]
    fn start_recovery_rejects_non_guardian() {
        let (env, contract_id, admin) = setup();
        let _guardians = setup_with_guardians(&env, &contract_id, &admin, 2, 1);
        let client = HelloContractClient::new(&env, &contract_id);

        let stranger = Address::generate(&env);
        let new_admin = Address::generate(&env);
        let result = client.try_start_recovery(&stranger, &admin, &new_admin);
        assert!(
            matches!(result, Err(Ok(GovernanceError::Unauthorized))),
            "non-guardian must be rejected, got {:?}",
            result
        );
    }

    #[test]
    fn start_recovery_resets_prior_request() {
        let (env, contract_id, admin) = setup();
        let guardians = setup_with_guardians(&env, &contract_id, &admin, 3, 2);
        let client = HelloContractClient::new(&env, &contract_id);

        let initiator = guardians.get(0).unwrap();
        let new_admin_1 = Address::generate(&env);
        let new_admin_2 = Address::generate(&env);

        client.start_recovery(&initiator, &admin, &new_admin_1);
        // Overwrite with a different new_admin.
        client.start_recovery(&initiator, &admin, &new_admin_2);

        let req = client.gov_get_recovery_request().unwrap();
        assert_eq!(req.new_admin, new_admin_2, "second start_recovery should overwrite the first");

        let approvals = client.gov_get_recovery_approvals().unwrap();
        assert_eq!(approvals.len(), 1, "approvals must reset on new request");
    }

    // -----------------------------------------------------------------------
    // approve_recovery
    // -----------------------------------------------------------------------

    #[test]
    fn approve_recovery_happy_path() {
        let (env, contract_id, admin) = setup();
        let guardians = setup_with_guardians(&env, &contract_id, &admin, 3, 2);
        let client = HelloContractClient::new(&env, &contract_id);

        let g0 = guardians.get(0).unwrap();
        let g1 = guardians.get(1).unwrap();
        let new_admin = Address::generate(&env);

        client.start_recovery(&g0, &admin, &new_admin);
        let result = client.try_approve_recovery(&g1);
        assert!(result.is_ok(), "second guardian approval should succeed");

        let approvals = client.gov_get_recovery_approvals().unwrap();
        assert_eq!(approvals.len(), 2);
    }

    #[test]
    fn approve_recovery_is_idempotent() {
        let (env, contract_id, admin) = setup();
        let guardians = setup_with_guardians(&env, &contract_id, &admin, 3, 2);
        let client = HelloContractClient::new(&env, &contract_id);

        let g0 = guardians.get(0).unwrap();
        let new_admin = Address::generate(&env);

        client.start_recovery(&g0, &admin, &new_admin);
        // g0 approves again — should be silently ignored.
        client.approve_recovery(&g0);
        client.approve_recovery(&g0);

        let approvals = client.gov_get_recovery_approvals().unwrap();
        assert_eq!(approvals.len(), 1, "duplicate approval must not increase count");
    }

    #[test]
    fn approve_recovery_rejects_non_guardian() {
        let (env, contract_id, admin) = setup();
        let guardians = setup_with_guardians(&env, &contract_id, &admin, 2, 2);
        let client = HelloContractClient::new(&env, &contract_id);

        let g0 = guardians.get(0).unwrap();
        let stranger = Address::generate(&env);
        let new_admin = Address::generate(&env);

        client.start_recovery(&g0, &admin, &new_admin);
        let result = client.try_approve_recovery(&stranger);
        assert!(
            matches!(result, Err(Ok(GovernanceError::Unauthorized))),
            "non-guardian must be rejected, got {:?}",
            result
        );
    }

    #[test]
    fn approve_recovery_rejects_when_no_request_open() {
        let (env, contract_id, admin) = setup();
        let guardians = setup_with_guardians(&env, &contract_id, &admin, 2, 1);
        let client = HelloContractClient::new(&env, &contract_id);

        let g0 = guardians.get(0).unwrap();
        let result = client.try_approve_recovery(&g0);
        assert!(
            matches!(result, Err(Ok(GovernanceError::NotInitialized))),
            "approve without open request must return NotInitialized, got {:?}",
            result
        );
    }

    // -----------------------------------------------------------------------
    // execute_recovery
    // -----------------------------------------------------------------------

    /// Full happy-path: start + approve (2-of-3) + execute.
    #[test]
    fn execute_recovery_rotates_admin() {
        let (env, contract_id, admin) = setup();
        let guardians = setup_with_guardians(&env, &contract_id, &admin, 3, 2);
        let client = HelloContractClient::new(&env, &contract_id);

        let g0 = guardians.get(0).unwrap();
        let g1 = guardians.get(1).unwrap();
        let new_admin = Address::generate(&env);

        client.start_recovery(&g0, &admin, &new_admin);
        client.approve_recovery(&g1);

        let executor = Address::generate(&env);
        let result = client.try_execute_recovery(&executor);
        assert!(result.is_ok(), "execute should succeed once threshold is met");

        // Verify via a call that requires admin auth: the new admin is accepted
        // and the old admin is now rejected.
        let new_guardians = make_guardians(&env, 1);
        let ok = client.try_set_guardians(&new_admin, &new_guardians, &1);
        assert!(ok.is_ok(), "new_admin should be accepted by admin gate after recovery");

        let denied = client.try_set_guardians(&admin, &new_guardians, &1);
        assert!(
            matches!(denied, Err(Ok(GovernanceError::Unauthorized))),
            "old admin must be rejected after recovery, got {:?}",
            denied
        );
    }

    #[test]
    fn execute_recovery_clears_request_and_approvals() {
        let (env, contract_id, admin) = setup();
        let guardians = setup_with_guardians(&env, &contract_id, &admin, 2, 1);
        let client = HelloContractClient::new(&env, &contract_id);

        let g0 = guardians.get(0).unwrap();
        let new_admin = Address::generate(&env);

        client.start_recovery(&g0, &admin, &new_admin);
        let executor = Address::generate(&env);
        client.execute_recovery(&executor);

        assert!(
            client.gov_get_recovery_request().is_none(),
            "recovery request must be cleared after execution"
        );
        assert!(
            client.gov_get_recovery_approvals().is_none(),
            "recovery approvals must be cleared after execution"
        );
    }

    #[test]
    fn execute_recovery_rejects_when_no_request_open() {
        let (env, contract_id, admin) = setup();
        let _guardians = setup_with_guardians(&env, &contract_id, &admin, 2, 1);
        let client = HelloContractClient::new(&env, &contract_id);

        let executor = Address::generate(&env);
        let result = client.try_execute_recovery(&executor);
        assert!(
            matches!(result, Err(Ok(GovernanceError::NotInitialized))),
            "execute without open request must return NotInitialized, got {:?}",
            result
        );
    }

    #[test]
    fn execute_recovery_rejects_when_threshold_not_met() {
        let (env, contract_id, admin) = setup();
        // threshold=2, but only g0 approves (1 < 2).
        let guardians = setup_with_guardians(&env, &contract_id, &admin, 3, 2);
        let client = HelloContractClient::new(&env, &contract_id);

        let g0 = guardians.get(0).unwrap();
        let new_admin = Address::generate(&env);

        client.start_recovery(&g0, &admin, &new_admin);
        // g0 is already counted as first approval (count=1, need 2).

        let executor = Address::generate(&env);
        let result = client.try_execute_recovery(&executor);
        assert!(
            matches!(result, Err(Ok(GovernanceError::Unauthorized))),
            "execute below threshold must be rejected, got {:?}",
            result
        );
    }

    /// Full 1-of-1: a single guardian can start and immediately execute.
    #[test]
    fn execute_recovery_one_of_one_guardian() {
        let (env, contract_id, admin) = setup();
        let guardians = setup_with_guardians(&env, &contract_id, &admin, 1, 1);
        let client = HelloContractClient::new(&env, &contract_id);

        let g0 = guardians.get(0).unwrap();
        let new_admin = Address::generate(&env);

        client.start_recovery(&g0, &admin, &new_admin);

        let executor = Address::generate(&env);
        let result = client.try_execute_recovery(&executor);
        assert!(result.is_ok(), "1-of-1 recovery should execute immediately after start");
    }

    /// Full 3-of-3: all guardians must approve before execution succeeds.
    #[test]
    fn execute_recovery_three_of_three_guardians() {
        let (env, contract_id, admin) = setup();
        let guardians = setup_with_guardians(&env, &contract_id, &admin, 3, 3);
        let client = HelloContractClient::new(&env, &contract_id);

        let g0 = guardians.get(0).unwrap();
        let g1 = guardians.get(1).unwrap();
        let g2 = guardians.get(2).unwrap();
        let new_admin = Address::generate(&env);

        client.start_recovery(&g0, &admin, &new_admin);

        // Only 1 approval (from g0), threshold=3 — must fail.
        let executor = Address::generate(&env);
        assert!(
            matches!(
                client.try_execute_recovery(&executor),
                Err(Ok(GovernanceError::Unauthorized))
            ),
            "should fail after 1 approval with threshold=3"
        );

        client.approve_recovery(&g1);
        // Now 2 approvals — still fail.
        assert!(
            matches!(
                client.try_execute_recovery(&executor),
                Err(Ok(GovernanceError::Unauthorized))
            ),
            "should fail after 2 approvals with threshold=3"
        );

        client.approve_recovery(&g2);
        // Now 3 approvals — must succeed.
        assert!(
            client.try_execute_recovery(&executor).is_ok(),
            "should succeed after all 3 approvals"
        );
    }
}
