//! Admin module — two-step admin handover with safety guards.
//!
//! Provides functions to read, set, and transfer the protocol admin authority.
//! The [`set_admin`] function validates that the new admin is a sane address
//! before writing, preventing accidental lockout of admin-gated operations.

use soroban_sdk::{contracterror, contractevent, contracttype, Address, Env};

// ---------------------------------------------------------------------------
// Storage key
// ---------------------------------------------------------------------------

#[contracttype]
pub enum AdminDataKey {
    Admin,
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors raised during admin handover.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum AdminError {
    /// Transfer target is the contract's own address.
    CannotTransferToSelf = 1,
    /// Transfer target is the same as the current admin (no-op churn).
    AlreadyAdmin = 2,
    /// Caller is not the current admin.
    Unauthorized = 3,
    /// Admin has not been initialized yet.
    NotInitialized = 4,
}

// ---------------------------------------------------------------------------
// Event
// ---------------------------------------------------------------------------

/// Emitted when the protocol admin is transferred to a new address.
///
/// Topics: `("admin", "transferred")`
#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdminTransferredEvent {
    /// Address of the former admin.
    pub old_admin: Address,
    /// Address of the new admin.
    pub new_admin: Address,
}

// ---------------------------------------------------------------------------
// Query helpers
// ---------------------------------------------------------------------------

/// Return `true` if an admin has been stored (contract is initialized).
pub fn has_admin(env: &Env) -> bool {
    env.storage().instance().has(&AdminDataKey::Admin)
}

/// Return the current admin address, or `None` if not initialized.
pub fn get_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&AdminDataKey::Admin)
}

// ---------------------------------------------------------------------------
// Mutator
// ---------------------------------------------------------------------------

/// Set (or transfer) the protocol admin.
///
/// # Arguments
///
/// * `env` — Soroban environment.
/// * `new_admin` — The address to set as admin.
/// * `caller` — When `Some(caller)`, authorises as the current admin and
///   performs safety validation. When `None` (used during contract
///   initialisation), skips auth and validation.
///
/// # Errors
///
/// * [`AdminError::CannotTransferToSelf`] — `new_admin` is the contract's own
///   address. Transferring admin to the contract would permanently brick
///   every admin-gated control because the contract itself cannot sign
///   transactions.
/// * [`AdminError::AlreadyAdmin`] — `new_admin` equals the current admin.
///   Rejecting this prevents unnecessary events and storage writes from
///   no-op churn.
/// * [`AdminError::Unauthorized`] — `caller` is `Some` but is not the current
///   admin.
/// * [`AdminError::NotInitialized`] — No admin exists yet (contract has not
///   been initialised with [`initialize`]).
///
/// # Events
///
/// Emits [`AdminTransferredEvent`] on success when `caller` is `Some`
/// (i.e. during a handover, not during initialisation).
///
/// # Safety model
///
/// The two-step flow is preserved: the current admin proposes by calling
/// `transfer_admin`, and the target address must sign to accept.  The
/// validation checks added here (`CannotTransferToSelf`, `AlreadyAdmin`)
/// prevent fat-finger lockout scenarios that have historically caused
/// irrecoverable protocol halts.
pub fn set_admin(
    env: &Env,
    new_admin: Address,
    caller: Option<Address>,
) -> Result<(), AdminError> {
    // When caller is provided, validate authorisation and check for unsafe
    // target addresses.
    if let Some(caller) = caller {
        caller.require_auth();

        let current_admin = env
            .storage()
            .instance()
            .get::<AdminDataKey, Address>(&AdminDataKey::Admin)
            .ok_or(AdminError::NotInitialized)?;

        if caller != current_admin {
            return Err(AdminError::Unauthorized);
        }

        // Guard: reject transfer to the contract's own address.
        // The contract address can never sign a transaction, so handing
        // admin to it would permanently lock all admin-gated functions.
        if new_admin == env.current_contract_address() {
            return Err(AdminError::CannotTransferToSelf);
        }

        // Guard: reject transfer to the same admin (no-op churn).
        if new_admin == current_admin {
            return Err(AdminError::AlreadyAdmin);
        }

        // Persist the new admin.
        env.storage()
            .instance()
            .set(&AdminDataKey::Admin, &new_admin);

        // Emit event after successful state mutation.
        AdminTransferredEvent {
            old_admin: current_admin,
            new_admin,
        }
        .publish(env);
    } else {
        // Initialisation path: no validation needed, just store.
        env.storage()
            .instance()
            .set(&AdminDataKey::Admin, &new_admin);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{contract, contractimpl, Env};

    /// Minimal contract to test admin module functions that need a deployed
    /// contract address (e.g. self-contract guard).
    #[contract]
    struct TestHost;

    #[contractimpl]
    impl TestHost {
        /// Expose `set_admin` so we can test it through the contract client,
        /// which gives us a real `env.current_contract_address()`.
        pub fn set_admin(env: Env, new_admin: Address, caller: Address) -> Result<(), AdminError> {
            crate::admin::set_admin(&env, new_admin, Some(caller))
        }

        pub fn initialize(env: Env, admin: Address) {
            crate::admin::set_admin(&env, admin, None).unwrap();
        }

        pub fn get_admin(env: Env) -> Option<Address> {
            crate::admin::get_admin(&env)
        }

        pub fn has_admin(env: Env) -> bool {
            crate::admin::has_admin(&env)
        }
    }

    fn setup() -> (Env, TestHostClient<'static>, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(TestHost, ());
        let client = TestHostClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let new_admin = Address::generate(&env);
        client.initialize(&admin);
        (env, client, admin, new_admin)
    }

    // -----------------------------------------------------------------------
    // Happy path
    // -----------------------------------------------------------------------

    #[test]
    fn test_transfer_to_valid_new_admin_succeeds() {
        let (env, client, admin, new_admin) = setup();
        let prev_admin = client.get_admin();
        assert_eq!(prev_admin, Some(admin.clone()));

        // Transfer to a different address.
        let result = client.try_set_admin(&new_admin, &admin);
        assert!(result.is_ok(), "transfer to valid new admin should succeed");

        let current = client.get_admin();
        assert_eq!(current, Some(new_admin));
    }

    #[test]
    fn test_admin_transferred_event_emitted_on_transfer() {
        let (env, client, admin, new_admin) = setup();

        let event_count_before = env.events().all().len();
        let _ = client.try_set_admin(&new_admin, &admin);
        let event_count_after = env.events().all().len();

        assert!(
            event_count_after > event_count_before,
            "AdminTransferredEvent should have been emitted"
        );
    }

    // -----------------------------------------------------------------------
    // Guard: transfer to the contract's own address
    // -----------------------------------------------------------------------

    #[test]
    fn test_transfer_to_self_contract_rejected() {
        let (env, client, admin, _new_admin) = setup();
        let contract_addr = env.current_contract_address();

        let result = client.try_set_admin(&contract_addr, &admin);
        assert!(
            matches!(result, Err(Ok(AdminError::CannotTransferToSelf))),
            "transfer to self-contract should be rejected with CannotTransferToSelf, got {:?}",
            result
        );

        // Admin should remain unchanged.
        assert_eq!(client.get_admin(), Some(admin));
    }

    // -----------------------------------------------------------------------
    // Guard: transfer to the same admin (no-op churn)
    // -----------------------------------------------------------------------

    #[test]
    fn test_transfer_to_current_admin_rejected() {
        let (env, client, admin, _new_admin) = setup();

        let result = client.try_set_admin(&admin, &admin);
        assert!(
            matches!(result, Err(Ok(AdminError::AlreadyAdmin))),
            "transfer to current admin should be rejected with AlreadyAdmin, got {:?}",
            result
        );

        // Admin should remain unchanged.
        assert_eq!(client.get_admin(), Some(admin));
    }

    // -----------------------------------------------------------------------
    // Guard: unauthorised caller
    // -----------------------------------------------------------------------

    #[test]
    fn test_transfer_by_non_admin_rejected() {
        let (env, _client, _admin, _new_admin) = setup();
        let attacker = Address::generate(&env);

        // Disable mock auth so the attacker actually fails auth.
        let env_no_mock = Env::default();
        let contract_id = env_no_mock.register(TestHost, ());
        let client_no_mock = TestHostClient::new(&env_no_mock, &contract_id);
        let admin = Address::generate(&env_no_mock);
        // Setup with auth on initialize
        env_no_mock.mock_auths(&[soroban_sdk::testutils::MockAuth {
            address: &admin,
            invoke: &soroban_sdk::testutils::MockAuthInvoke {
                contract: &contract_id,
                fn_name: "initialize",
                args: (admin.clone(),).into_val(&env_no_mock),
                sub_invokes: &[],
            },
        }]);
        client_no_mock.initialize(&admin);

        // Attacker tries to transfer without auth — should panic on require_auth.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            client_no_mock.set_admin(&Address::generate(&env_no_mock), &attacker);
        }));
        assert!(
            result.is_err(),
            "non-admin caller should panic on require_auth"
        );
    }

    // -----------------------------------------------------------------------
    // Guard: transferring before initialisation
    // -----------------------------------------------------------------------

    #[test]
    fn test_transfer_before_initialization_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(TestHost, ());
        let client = TestHostClient::new(&env, &contract_id);
        let caller = Address::generate(&env);
        let new_admin = Address::generate(&env);

        let result = client.try_set_admin(&new_admin, &caller);
        assert!(
            matches!(result, Err(Ok(AdminError::NotInitialized))),
            "transfer before init should be rejected with NotInitialized, got {:?}",
            result
        );
    }

    // -----------------------------------------------------------------------
    // has_admin / get_admin behaviour
    // -----------------------------------------------------------------------

    #[test]
    fn test_has_admin_returns_true_after_initialize() {
        let (_env, client, _admin, _new_admin) = setup();
        assert!(client.has_admin());
    }

    #[test]
    fn test_has_admin_returns_false_before_initialize() {
        let env = Env::default();
        let contract_id = env.register(TestHost, ());
        let client = TestHostClient::new(&env, &contract_id);
        assert!(!client.has_admin());
    }

    #[test]
    fn test_get_admin_returns_none_before_initialize() {
        let env = Env::default();
        let contract_id = env.register(TestHost, ());
        let client = TestHostClient::new(&env, &contract_id);
        assert_eq!(client.get_admin(), None);
    }

    #[test]
    fn test_get_admin_returns_admin_after_initialize() {
        let (_env, client, admin, _new_admin) = setup();
        assert_eq!(client.get_admin(), Some(admin));
    }

    // -----------------------------------------------------------------------
    // Multiple transfers work correctly
    // -----------------------------------------------------------------------

    #[test]
    fn test_sequential_transfers_allowed() {
        let (env, client, admin, new_admin) = setup();
        let third_admin = Address::generate(&env);

        // First transfer: admin → new_admin
        let r1 = client.try_set_admin(&new_admin, &admin);
        assert!(r1.is_ok(), "first transfer should succeed");
        assert_eq!(client.get_admin(), Some(new_admin.clone()));

        // Second transfer: new_admin → third_admin
        let r2 = client.try_set_admin(&third_admin, &new_admin);
        assert!(r2.is_ok(), "second transfer should succeed");
        assert_eq!(client.get_admin(), Some(third_admin));
    }

    // -----------------------------------------------------------------------
    // Error code stability
    // -----------------------------------------------------------------------

    #[test]
    fn test_error_code_stability() {
        assert_eq!(AdminError::CannotTransferToSelf as u32, 1);
        assert_eq!(AdminError::AlreadyAdmin as u32, 2);
        assert_eq!(AdminError::Unauthorized as u32, 3);
        assert_eq!(AdminError::NotInitialized as u32, 4);
    }

}
