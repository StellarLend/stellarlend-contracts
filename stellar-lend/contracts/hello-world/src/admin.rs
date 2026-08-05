//! Admin module — two-step admin handover with safety guards.
//!
//! Provides functions to read, propose, accept, and initialise the protocol
//! admin authority.
//!
//! ## Two-step handover
//!
//! Admin transfer is intentionally two-phased to ensure the incoming admin
//! consents before control is transferred:
//!
//! 1. The current admin calls [`propose_admin`], which records `new_admin` as
//!    the pending admin. This does **not** change the active admin.
//! 2. The proposed admin calls [`accept_admin`], which requires their
//!    signature (`new_admin.require_auth()`), promotes them to active admin,
//!    and clears the pending slot.
//!
//! This prevents accidental lockout: if the proposed address is wrong or
//! unreachable, no handover occurs — the current admin retains control and
//! can propose a different address.
//!
//! The validation guards (`CannotTransferToSelf`, `AlreadyAdmin`) on
//! [`propose_admin`] further prevent fat-finger proposals.

use soroban_sdk::{contracterror, contractevent, contracttype, Address, Env};

// ---------------------------------------------------------------------------
// Storage keys
// ---------------------------------------------------------------------------

#[contracttype]
pub enum AdminDataKey {
    /// The active protocol admin.
    Admin,
    /// A pending admin proposed by the current admin; cleared on acceptance.
    PendingAdmin,
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
    /// `accept_admin` was called but no admin has been proposed.
    PendingAdminNotSet = 5,
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

/// Emitted when a new admin is proposed by the current admin.
///
/// Topics: `("admin", "proposed")`
#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdminProposedEvent {
    /// Address of the current admin who submitted the proposal.
    pub current_admin: Address,
    /// Address of the proposed new admin.
    pub proposed_admin: Address,
}

/// Emitted when the proposed admin accepts and becomes the active admin.
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

/// Return the pending admin address, or `None` if no proposal is active.
pub fn get_pending_admin(env: &Env) -> Option<Address> {
    env.storage().instance().get(&AdminDataKey::PendingAdmin)
}

/// Require `caller` to be the stored protocol admin.
///
/// This is the shared authorization check for admin-gated modules. Keeping
/// the lookup here ensures every module uses the same admin storage and
/// initialization semantics.
pub fn require_admin(env: &Env, caller: &Address) -> Result<(), AdminError> {
    caller.require_auth();

    match get_admin(env) {
        Some(admin) if admin == *caller => Ok(()),
        Some(_) => Err(AdminError::Unauthorized),
        None => Err(AdminError::NotInitialized),
    }
}

// ---------------------------------------------------------------------------
// Initialisation
// ---------------------------------------------------------------------------

/// Store the initial admin during contract initialisation (no auth required).
///
/// This is the only path that bypasses authentication. It must only be called
/// once, during `initialize`, before any admin is stored.
pub fn set_admin(env: &Env, new_admin: Address, caller: Option<Address>) -> Result<(), AdminError> {
    if let Some(caller) = caller {
        // Delegate to the two-step propose path for post-init transfers.
        // Callers that previously used set_admin(env, new_admin, Some(caller))
        // should migrate to propose_admin + accept_admin.
        propose_admin(env, new_admin, caller)
    } else {
        // Initialisation path: no validation needed, just store.
        env.storage()
            .instance()
            .set(&AdminDataKey::Admin, &new_admin);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Two-step handover
// ---------------------------------------------------------------------------

/// Propose a new admin (current admin only) — step 1 of 2.
///
/// Records `new_admin` as the pending admin candidate. The active admin is
/// **not** changed until `new_admin` calls [`accept_admin`].
///
/// Calling this a second time with a different address replaces the earlier
/// pending proposal (useful for correcting a mis-typed address).
///
/// # Arguments
///
/// * `env` — Soroban environment.
/// * `new_admin` — The address being nominated as the next admin.
/// * `caller` — Must be the current active admin.
///
/// # Errors
///
/// * [`AdminError::NotInitialized`] — No admin exists yet.
/// * [`AdminError::Unauthorized`] — `caller` is not the current admin.
/// * [`AdminError::CannotTransferToSelf`] — `new_admin` is the contract's own
///   address; the contract can never sign, so this would permanently lock
///   every admin-gated function.
/// * [`AdminError::AlreadyAdmin`] — `new_admin` is already the active admin.
///
/// # Events
///
/// Emits [`AdminProposedEvent`] on success.
pub fn propose_admin(env: &Env, new_admin: Address, caller: Address) -> Result<(), AdminError> {
    caller.require_auth();

    let current_admin = get_admin(env).ok_or(AdminError::NotInitialized)?;

    if caller != current_admin {
        return Err(AdminError::Unauthorized);
    }

    // Guard: reject proposal to the contract's own address.
    if new_admin == env.current_contract_address() {
        return Err(AdminError::CannotTransferToSelf);
    }

    // Guard: reject proposal to the same admin (no-op churn).
    if new_admin == current_admin {
        return Err(AdminError::AlreadyAdmin);
    }

    env.storage()
        .instance()
        .set(&AdminDataKey::PendingAdmin, &new_admin);

    AdminProposedEvent {
        current_admin: caller,
        proposed_admin: new_admin,
    }
    .publish(env);

    Ok(())
}

/// Accept the pending admin proposal (proposed admin only) — step 2 of 2.
///
/// The caller must be the address that was nominated via [`propose_admin`].
/// On success, the caller becomes the active admin and the pending slot is
/// cleared.
///
/// # Arguments
///
/// * `env` — Soroban environment.
/// * `caller` — Must match the stored pending admin address.
///
/// # Errors
///
/// * [`AdminError::NotInitialized`] — No admin exists yet.
/// * [`AdminError::PendingAdminNotSet`] — No proposal is currently active.
/// * [`AdminError::Unauthorized`] — `caller` does not match the pending admin.
///
/// # Events
///
/// Emits [`AdminTransferredEvent`] on success.
pub fn accept_admin(env: &Env, caller: Address) -> Result<(), AdminError> {
    caller.require_auth();

    if !has_admin(env) {
        return Err(AdminError::NotInitialized);
    }

    let pending_admin = get_pending_admin(env).ok_or(AdminError::PendingAdminNotSet)?;

    if caller != pending_admin {
        return Err(AdminError::Unauthorized);
    }

    let old_admin = get_admin(env).expect("admin must exist if pending admin is set");

    // Promote pending admin to active admin and clear the pending slot.
    env.storage()
        .instance()
        .set(&AdminDataKey::Admin, &pending_admin);
    env.storage()
        .instance()
        .remove(&AdminDataKey::PendingAdmin);

    AdminTransferredEvent {
        old_admin,
        new_admin: pending_admin,
    }
    .publish(env);

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
        /// Initialise the contract with the given admin (no auth required).
        pub fn initialize(env: Env, admin: Address) {
            crate::admin::set_admin(&env, admin, None).unwrap();
        }

        /// Step 1: propose a new admin (current admin only).
        pub fn propose_admin(
            env: Env,
            new_admin: Address,
            caller: Address,
        ) -> Result<(), AdminError> {
            crate::admin::propose_admin(&env, new_admin, caller)
        }

        /// Step 2: accept the pending admin proposal (proposed admin only).
        pub fn accept_admin(env: Env, caller: Address) -> Result<(), AdminError> {
            crate::admin::accept_admin(&env, caller)
        }

        pub fn get_admin(env: Env) -> Option<Address> {
            crate::admin::get_admin(&env)
        }

        pub fn get_pending_admin(env: Env) -> Option<Address> {
            crate::admin::get_pending_admin(&env)
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
    // Happy path: full two-step flow
    // -----------------------------------------------------------------------

    #[test]
    fn test_full_two_step_flow_succeeds() {
        let (env, client, admin, new_admin) = setup();
        assert_eq!(client.get_admin(), Some(admin.clone()));
        assert_eq!(client.get_pending_admin(), None);

        // Step 1: current admin proposes.
        let r = client.try_propose_admin(&new_admin, &admin);
        assert!(r.is_ok(), "propose_admin should succeed");
        assert_eq!(client.get_admin(), Some(admin.clone()), "active admin must not change after propose");
        assert_eq!(client.get_pending_admin(), Some(new_admin.clone()), "pending admin should be set");

        // Step 2: proposed admin accepts.
        let r = client.try_accept_admin(&new_admin);
        assert!(r.is_ok(), "accept_admin should succeed");
        assert_eq!(client.get_admin(), Some(new_admin.clone()), "active admin should be the new admin");
        assert_eq!(client.get_pending_admin(), None, "pending admin should be cleared after acceptance");
    }

    #[test]
    fn test_admin_transferred_event_emitted_on_accept() {
        let (env, client, admin, new_admin) = setup();

        client.propose_admin(&new_admin, &admin);

        let event_count_before = env.events().all().len();
        let _ = client.try_accept_admin(&new_admin);
        let event_count_after = env.events().all().len();

        assert!(
            event_count_after > event_count_before,
            "AdminTransferredEvent should have been emitted on accept"
        );
    }

    #[test]
    fn test_admin_proposed_event_emitted_on_propose() {
        let (env, client, admin, new_admin) = setup();

        let event_count_before = env.events().all().len();
        let _ = client.try_propose_admin(&new_admin, &admin);
        let event_count_after = env.events().all().len();

        assert!(
            event_count_after > event_count_before,
            "AdminProposedEvent should have been emitted on propose"
        );
    }

    // -----------------------------------------------------------------------
    // Propose guards
    // -----------------------------------------------------------------------

    #[test]
    fn test_propose_to_self_contract_rejected() {
        let (env, client, admin, _new_admin) = setup();
        let contract_addr = env.current_contract_address();

        let result = client.try_propose_admin(&contract_addr, &admin);
        assert!(
            matches!(result, Err(Ok(AdminError::CannotTransferToSelf))),
            "propose to self-contract should be rejected, got {:?}",
            result
        );
        assert_eq!(client.get_pending_admin(), None, "pending admin should remain unset");
    }

    #[test]
    fn test_propose_to_current_admin_rejected() {
        let (_env, client, admin, _new_admin) = setup();

        let result = client.try_propose_admin(&admin, &admin);
        assert!(
            matches!(result, Err(Ok(AdminError::AlreadyAdmin))),
            "propose to current admin should be rejected, got {:?}",
            result
        );
        assert_eq!(client.get_pending_admin(), None);
    }

    #[test]
    fn test_propose_by_non_admin_rejected() {
        let (env, client, _admin, new_admin) = setup();
        let attacker = Address::generate(&env);

        let result = client.try_propose_admin(&new_admin, &attacker);
        assert!(
            matches!(result, Err(Ok(AdminError::Unauthorized))),
            "non-admin caller should be rejected with Unauthorized, got {:?}",
            result
        );
        assert_eq!(client.get_pending_admin(), None);
    }

    #[test]
    fn test_propose_before_initialization_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(TestHost, ());
        let client = TestHostClient::new(&env, &contract_id);
        let caller = Address::generate(&env);
        let new_admin = Address::generate(&env);

        let result = client.try_propose_admin(&new_admin, &caller);
        assert!(
            matches!(result, Err(Ok(AdminError::NotInitialized))),
            "propose before init should be rejected, got {:?}",
            result
        );
    }

    // -----------------------------------------------------------------------
    // Accept guards
    // -----------------------------------------------------------------------

    #[test]
    fn test_accept_without_pending_proposal_rejected() {
        let (_env, client, _admin, new_admin) = setup();

        // No propose has been called — accept should fail.
        let result = client.try_accept_admin(&new_admin);
        assert!(
            matches!(result, Err(Ok(AdminError::PendingAdminNotSet))),
            "accept with no pending proposal should be rejected, got {:?}",
            result
        );
    }

    #[test]
    fn test_accept_by_wrong_address_rejected() {
        let (env, client, admin, new_admin) = setup();
        let attacker = Address::generate(&env);

        client.propose_admin(&new_admin, &admin);

        // Attacker tries to accept the pending proposal.
        let result = client.try_accept_admin(&attacker);
        assert!(
            matches!(result, Err(Ok(AdminError::Unauthorized))),
            "wrong address should be rejected on accept, got {:?}",
            result
        );
        // Active admin must remain unchanged.
        assert_eq!(client.get_admin(), Some(admin));
        // Pending admin must still be set.
        assert_eq!(client.get_pending_admin(), Some(new_admin));
    }

    #[test]
    fn test_accept_before_initialization_rejected() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(TestHost, ());
        let client = TestHostClient::new(&env, &contract_id);
        let caller = Address::generate(&env);

        let result = client.try_accept_admin(&caller);
        assert!(
            matches!(result, Err(Ok(AdminError::NotInitialized))),
            "accept before init should be rejected, got {:?}",
            result
        );
    }

    // -----------------------------------------------------------------------
    // Proposal can be overwritten before acceptance
    // -----------------------------------------------------------------------

    #[test]
    fn test_propose_can_overwrite_previous_proposal() {
        let (env, client, admin, first_candidate) = setup();
        let second_candidate = Address::generate(&env);

        client.propose_admin(&first_candidate, &admin);
        assert_eq!(client.get_pending_admin(), Some(first_candidate.clone()));

        // Overwrite with second candidate.
        let r = client.try_propose_admin(&second_candidate, &admin);
        assert!(r.is_ok(), "overwriting pending proposal should succeed");
        assert_eq!(client.get_pending_admin(), Some(second_candidate.clone()));

        // First candidate can no longer accept.
        let result = client.try_accept_admin(&first_candidate);
        assert!(
            matches!(result, Err(Ok(AdminError::Unauthorized))),
            "superseded candidate should not be able to accept, got {:?}",
            result
        );

        // Second candidate succeeds.
        let r2 = client.try_accept_admin(&second_candidate);
        assert!(r2.is_ok(), "second candidate should accept successfully");
        assert_eq!(client.get_admin(), Some(second_candidate));
    }

    // -----------------------------------------------------------------------
    // Sequential transfers
    // -----------------------------------------------------------------------

    #[test]
    fn test_sequential_transfers_allowed() {
        let (env, client, admin, new_admin) = setup();
        let third_admin = Address::generate(&env);

        // First handover: admin → new_admin
        client.propose_admin(&new_admin, &admin);
        client.accept_admin(&new_admin);
        assert_eq!(client.get_admin(), Some(new_admin.clone()));

        // Second handover: new_admin → third_admin
        client.propose_admin(&third_admin, &new_admin);
        client.accept_admin(&third_admin);
        assert_eq!(client.get_admin(), Some(third_admin));
    }

    // -----------------------------------------------------------------------
    // has_admin / get_admin helpers
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
    // Error code stability
    // -----------------------------------------------------------------------

    #[test]
    fn test_error_code_stability() {
        assert_eq!(AdminError::CannotTransferToSelf as u32, 1);
        assert_eq!(AdminError::AlreadyAdmin as u32, 2);
        assert_eq!(AdminError::Unauthorized as u32, 3);
        assert_eq!(AdminError::NotInitialized as u32, 4);
        assert_eq!(AdminError::PendingAdminNotSet as u32, 5);
    }
}
