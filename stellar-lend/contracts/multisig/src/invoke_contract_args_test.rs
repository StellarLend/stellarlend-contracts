/// End-to-end test: an InvokeContract proposal that targets a function
/// requiring arguments succeeds, and the target contract actually receives
/// those arguments.
///
/// Test strategy
/// ─────────────
/// 1. Deploy a minimal stub contract (`StubTarget`) that exposes a single
///    entrypoint `set_value(u32)` which stores the received value.
/// 2. Deploy the multisig with a 2-of-3 signer set.
/// 3. Build an `InvokeContract` action whose `args` field holds the XDR-
///    encoded `u32` argument as a `Vec<Bytes>`.
/// 4. Create a proposal, collect two approvals (threshold = 2), execute.
/// 5. Read back the value stored by `StubTarget` and assert it equals 42.
///
/// The test would fail (stub stores 0, not 42) if `dispatch_action` still
/// hard-coded an empty argument list.
#[cfg(test)]
mod invoke_contract_args_e2e {
    use crate::{MultisigContract, MultisigContractClient, ProposalAction, ProposalStatus};
    use soroban_sdk::{
        contract, contractimpl, contracttype,
        testutils::Address as _,
        xdr::ToXdr,
        Address, Bytes, Env, IntoVal, Symbol, Vec,
    };

    // -----------------------------------------------------------------------
    // Minimal stub target contract
    // -----------------------------------------------------------------------

    #[contracttype]
    pub enum StubKey {
        Value,
    }

    #[contract]
    pub struct StubTarget;

    #[contractimpl]
    impl StubTarget {
        /// Store `value` in persistent storage so the test can read it back.
        pub fn set_value(env: Env, value: u32) {
            env.storage()
                .persistent()
                .set(&StubKey::Value, &value);
        }

        /// Read back the stored value (0 if never set).
        pub fn get_value(env: Env) -> u32 {
            env.storage()
                .persistent()
                .get(&StubKey::Value)
                .unwrap_or(0u32)
        }
    }

    // -----------------------------------------------------------------------
    // Helper: encode a single IntoVal<Env, Val> argument as XDR Bytes
    // -----------------------------------------------------------------------

    fn encode_arg<T: IntoVal<soroban_sdk::Env, soroban_sdk::Val>>(env: &Env, v: T) -> Bytes {
        // ToXdr serialises via Val → ScVal XDR, which is exactly what
        // FromXdr in dispatch_action decodes.
        v.into_val(env).to_xdr(env)
    }

    // -----------------------------------------------------------------------
    // Helper: deploy multisig with `threshold` and `signer_count` signers
    // -----------------------------------------------------------------------

    fn setup_multisig(
        env: &Env,
        threshold: u32,
        signer_count: usize,
    ) -> (Address, Vec<Address>) {
        let contract_id = env.register(MultisigContract, ());
        let client = MultisigContractClient::new(env, &contract_id);

        let mut signers = Vec::new(env);
        for _ in 0..signer_count {
            signers.push_back(Address::generate(env));
        }

        client.initialize(&signers, &threshold);
        (contract_id, signers)
    }

    // -----------------------------------------------------------------------
    // E2E test — parameterised call
    // -----------------------------------------------------------------------

    #[test]
    fn invoke_contract_with_args_reaches_target() {
        let env = Env::default();
        env.mock_all_auths();

        // 1. Deploy the stub target
        let stub_id = env.register(StubTarget, ());

        // 2. Deploy a 2-of-3 multisig
        let (multisig_id, signers) = setup_multisig(&env, 2, 3);
        let client = MultisigContractClient::new(&env, &multisig_id);

        // 3. Build the InvokeContract action.
        //    We want to call `stub.set_value(42u32)`.
        //    Each argument is XDR-encoded into a `Bytes` element.
        let expected_value: u32 = 42;
        let mut args: Vec<Bytes> = Vec::new(&env);
        args.push_back(encode_arg(&env, expected_value));

        let action = ProposalAction::InvokeContract(
            stub_id.clone(),
            Symbol::new(&env, "set_value"),
            args,
        );

        // payload_hash is presented unchanged at execution time.
        let payload_hash = Bytes::from_slice(&env, b"invoke_args_e2e_hash");

        // 4. Create the proposal
        let proposal_id = client.create_proposal(
            &signers.get(0).unwrap(),
            &action,
            &payload_hash,
            &500u64,
        );

        // 5. Collect 2 approvals (threshold = 2)
        client.approve_proposal(&signers.get(0).unwrap(), &proposal_id);
        client.approve_proposal(&signers.get(1).unwrap(), &proposal_id);

        let p = client.get_proposal(&proposal_id);
        assert_eq!(
            p.status,
            ProposalStatus::Passed,
            "Proposal should be Passed after reaching quorum"
        );

        // 6. Execute
        client.execute_proposal(
            &signers.get(0).unwrap(),
            &proposal_id,
            &payload_hash,
        );

        let p = client.get_proposal(&proposal_id);
        assert_eq!(
            p.status,
            ProposalStatus::Executed,
            "Proposal should be Executed after execute_proposal"
        );

        // 7. Verify the argument actually reached the stub
        let stub_client = StubTargetClient::new(&env, &stub_id);
        let stored = stub_client.get_value();
        assert_eq!(
            stored, expected_value,
            "StubTarget should have stored {expected_value} but got {stored}; \
             dispatch_action may still be passing an empty arg list"
        );
    }

    // -----------------------------------------------------------------------
    // Regression: zero-arg InvokeContract still works
    // -----------------------------------------------------------------------

    #[contracttype]
    pub enum NullKey {
        Called,
    }

    #[contract]
    pub struct NullStub;

    #[contractimpl]
    impl NullStub {
        pub fn ping(env: Env) {
            env.storage().persistent().set(&NullKey::Called, &true);
        }

        pub fn was_called(env: Env) -> bool {
            env.storage()
                .persistent()
                .get(&NullKey::Called)
                .unwrap_or(false)
        }
    }

    #[test]
    fn invoke_contract_zero_args_still_works() {
        let env = Env::default();
        env.mock_all_auths();

        let null_id = env.register(NullStub, ());
        let (multisig_id, signers) = setup_multisig(&env, 2, 3);
        let client = MultisigContractClient::new(&env, &multisig_id);

        let action = ProposalAction::InvokeContract(
            null_id.clone(),
            Symbol::new(&env, "ping"),
            Vec::new(&env),
        );

        let payload_hash = Bytes::from_slice(&env, b"zero_args_hash");
        let pid = client.create_proposal(
            &signers.get(0).unwrap(),
            &action,
            &payload_hash,
            &500u64,
        );

        client.approve_proposal(&signers.get(0).unwrap(), &pid);
        client.approve_proposal(&signers.get(1).unwrap(), &pid);
        client.execute_proposal(&signers.get(0).unwrap(), &pid, &payload_hash);

        let null_client = NullStubClient::new(&env, &null_id);
        assert!(null_client.was_called(), "NullStub::ping should have been called");
    }
}
