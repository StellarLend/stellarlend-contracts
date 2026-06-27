#![no_std]

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, Address, Env, Vec,
};

/// Minimum threshold delay in ledgers (7 days = ~604,800 seconds / 5 sec per ledger = ~120,960 ledgers)
/// Using conservative estimate: 600,000 ledgers for 7 days
const MIN_THRESHOLD_DELAY_LEDGERS: u32 = 600_000;
/// Default proposal lifetime in ledgers (14 days at ~5 seconds per ledger).
const DEFAULT_PROPOSAL_EXPIRY_LEDGERS: u32 = 1_200_000;

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DataKey {
    /// Current multisig threshold
    Threshold,
    /// Admin who can queue and apply threshold changes
    Admin,
    /// Pending threshold change (new_threshold, eta_ledger)
    PendingThresholdChange,
    /// Ledger number when contract was initialized
    InitializedLedger,
    /// Monotonic proposal id counter
    ProposalCounter,
    /// Proposal keyed by id
    Proposal(u64),
    /// Stored approvals keyed by proposal id
    ProposalApprovals(u64),
    /// Registered signer set eligible to approve proposals
    Signers,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum MultisigError {
    /// Caller is not the admin
    Unauthorized = 1001,
    /// No pending threshold change queued
    NoQueuedChange = 1002,
    /// Threshold change delay period not yet elapsed
    DelayNotElapsed = 1003,
    /// Invalid threshold (must be > 0)
    InvalidThreshold = 1004,
    /// Contract has not been initialized
    NotInitialized = 1005,
    /// Contract already initialized
    AlreadyInitialized = 1006,
    /// Proposal does not exist
    ProposalNotFound = 1007,
    /// Proposal timelock has not elapsed
    ProposalNotReady = 1008,
    /// Proposal was already executed
    ProposalAlreadyExecuted = 1009,
    /// Proposal expiry ledger has passed
    ProposalExpired = 1010,
    /// Proposal parameters are invalid
    InvalidProposal = 1011,
    /// Caller has already approved this proposal (duplicate approval)
    AlreadyApproved = 1012,
    /// Caller is not a registered signer
    NotASigner = 1013,
    /// Quorum has not been reached (too few current-signer approvals)
    InsufficientApprovals = 1014,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThresholdChange {
    pub new_threshold: u32,
    pub eta_ledger: u32,
}

#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposal {
    pub id: u64,
    pub new_threshold: u32,
    pub eta_ledger: u32,
    pub expires_at_ledger: u32,
    pub executed: bool,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThresholdChangeQueuedEvent {
    pub admin: Address,
    pub new_threshold: u32,
    pub eta_ledger: u32,
}

#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThresholdChangeAppliedEvent {
    pub admin: Address,
    pub old_threshold: u32,
    pub new_threshold: u32,
    pub ledger: u32,
}

#[contract]
pub struct MultisigContract;

#[contractimpl]
impl MultisigContract {
    /// Initialize the multisig contract with an admin and initial threshold.
    /// Can only be called once.
    ///
    /// # Arguments
    /// * `env` - Soroban environment
    /// * `admin` - Address of the multisig administrator
    /// * `initial_threshold` - Initial signing threshold (must be > 0)
    ///
    /// # Errors
    /// * `AlreadyInitialized` - Contract already initialized
    /// * `InvalidThreshold` - Initial threshold is 0 or negative
    pub fn initialize(
        env: Env,
        admin: Address,
        initial_threshold: u32,
    ) -> Result<(), MultisigError> {
        if env.storage().instance().has(&DataKey::Threshold) {
            return Err(MultisigError::AlreadyInitialized);
        }

        if initial_threshold == 0 {
            return Err(MultisigError::InvalidThreshold);
        }

        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::Threshold, &initial_threshold);
        env.storage()
            .instance()
            .set(&DataKey::InitializedLedger, &env.ledger().sequence());

        Ok(())
    }

    /// Get the current multisig threshold.
    ///
    /// # Arguments
    /// * `env` - Soroban environment
    ///
    /// # Returns
    /// Current threshold value
    ///
    /// # Errors
    /// * `NotInitialized` - Contract not yet initialized
    pub fn get_threshold(env: Env) -> Result<u32, MultisigError> {
        env.storage()
            .instance()
            .get(&DataKey::Threshold)
            .ok_or(MultisigError::NotInitialized)
    }

    /// Get the current admin.
    ///
    /// # Arguments
    /// * `env` - Soroban environment
    ///
    /// # Returns
    /// Current admin address
    ///
    /// # Errors
    /// * `NotInitialized` - Contract not yet initialized
    pub fn get_admin(env: Env) -> Result<Address, MultisigError> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(MultisigError::NotInitialized)
    }

    /// Get any pending threshold change.
    ///
    /// # Arguments
    /// * `env` - Soroban environment
    ///
    /// # Returns
    /// Option containing pending change (new_threshold, eta_ledger) or None
    pub fn get_pending_threshold_change(env: Env) -> Option<ThresholdChange> {
        env.storage()
            .instance()
            .get(&DataKey::PendingThresholdChange)
    }

    /// Queue a new threshold change with a minimum delay of MIN_THRESHOLD_DELAY_LEDGERS.
    /// This prevents same-ledger takeover by requiring a delayed execution step.
    ///
    /// # Arguments
    /// * `env` - Soroban environment
    /// * `new_threshold` - New threshold value (must be > 0)
    ///
    /// # Security Invariant
    /// The threshold change is not applied immediately. It must be executed separately
    /// via `apply_threshold_change()` after the delay period has elapsed. This ensures
    /// that a compromised quorum cannot lower the threshold and pass a malicious proposal
    /// in the same transaction.
    ///
    /// # Errors
    /// * `Unauthorized` - Caller is not the admin
    /// * `NotInitialized` - Contract not initialized
    /// * `InvalidThreshold` - New threshold is 0
    pub fn queue_threshold_change(env: Env, new_threshold: u32) -> Result<(), MultisigError> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        if new_threshold == 0 {
            return Err(MultisigError::InvalidThreshold);
        }

        let current_ledger = env.ledger().sequence();
        let eta_ledger = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS;

        let change = ThresholdChange {
            new_threshold,
            eta_ledger,
        };

        env.storage()
            .instance()
            .set(&DataKey::PendingThresholdChange, &change);

        ThresholdChangeQueuedEvent {
            admin: admin.clone(),
            new_threshold,
            eta_ledger,
        }
        .publish(&env);

        Ok(())
    }

    /// Apply a previously queued threshold change.
    /// The delay period (MIN_THRESHOLD_DELAY_LEDGERS) must have elapsed since queueing.
    ///
    /// # Arguments
    /// * `env` - Soroban environment
    ///
    /// # Security Invariant
    /// * Prevents same-ledger takeover by enforcing a time delay
    /// * Only applies if the current ledger >= eta_ledger from queue operation
    /// * Clears the pending change on successful application
    ///
    /// # Errors
    /// * `Unauthorized` - Caller is not the admin
    /// * `NotInitialized` - Contract not initialized
    /// * `NoQueuedChange` - No threshold change is queued
    /// * `DelayNotElapsed` - Current ledger < eta_ledger
    pub fn apply_threshold_change(env: Env) -> Result<(), MultisigError> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        let change: ThresholdChange = env
            .storage()
            .instance()
            .get(&DataKey::PendingThresholdChange)
            .ok_or(MultisigError::NoQueuedChange)?;

        let current_ledger = env.ledger().sequence();
        if current_ledger < change.eta_ledger {
            return Err(MultisigError::DelayNotElapsed);
        }

        let old_threshold = Self::get_threshold(env.clone())?;
        env.storage()
            .instance()
            .set(&DataKey::Threshold, &change.new_threshold);
        env.storage()
            .instance()
            .remove(&DataKey::PendingThresholdChange);

        ThresholdChangeAppliedEvent {
            admin: admin.clone(),
            old_threshold,
            new_threshold: change.new_threshold,
            ledger: current_ledger,
        }
        .publish(&env);

        Ok(())
    }

    /// Create a threshold-change proposal with an explicit expiry ledger.
    ///
    /// The current admin is recorded as the first approver. Execution remains
    /// unavailable until the threshold-change delay has elapsed and permanently
    /// fails once `env.ledger().sequence() > expires_at_ledger`.
    pub fn create_proposal(
        env: Env,
        new_threshold: u32,
        expires_at_ledger: u32,
    ) -> Result<u64, MultisigError> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        if new_threshold == 0 {
            return Err(MultisigError::InvalidThreshold);
        }

        let current_ledger = env.ledger().sequence();
        if expires_at_ledger <= current_ledger {
            return Err(MultisigError::InvalidProposal);
        }

        let eta_ledger = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS;
        if expires_at_ledger < eta_ledger {
            return Err(MultisigError::InvalidProposal);
        }

        let next_id = env
            .storage()
            .instance()
            .get(&DataKey::ProposalCounter)
            .unwrap_or(0u64)
            + 1;

        let proposal = Proposal {
            id: next_id,
            new_threshold,
            eta_ledger,
            expires_at_ledger,
            executed: false,
        };

        let mut approvals = Vec::new(&env);
        approvals.push_back(admin);

        env.storage()
            .instance()
            .set(&DataKey::ProposalCounter, &next_id);
        env.storage()
            .instance()
            .set(&DataKey::Proposal(next_id), &proposal);
        env.storage()
            .instance()
            .set(&DataKey::ProposalApprovals(next_id), &approvals);

        Ok(next_id)
    }

    /// Create a proposal with the default 14-day expiry window.
    pub fn create_proposal_default_expiry(
        env: Env,
        new_threshold: u32,
    ) -> Result<u64, MultisigError> {
        let expires_at_ledger = env.ledger().sequence() + DEFAULT_PROPOSAL_EXPIRY_LEDGERS;
        Self::create_proposal(env, new_threshold, expires_at_ledger)
    }

    /// Get a proposal by id.
    pub fn get_proposal(env: Env, id: u64) -> Option<Proposal> {
        env.storage().instance().get(&DataKey::Proposal(id))
    }

    /// Get the approvals stored for a proposal id.
    pub fn get_proposal_approvals(env: Env, id: u64) -> Option<Vec<Address>> {
        env.storage()
            .instance()
            .get(&DataKey::ProposalApprovals(id))
    }

    /// Register the set of signers eligible to approve proposals.
    ///
    /// Only the admin may update the signer set. The signer set is independent
    /// of the admin and may overlap with it. Quorum is evaluated against this
    /// set at execution time, so changes take effect immediately for all
    /// pending proposals.
    ///
    /// # Arguments
    /// * `env` - Soroban environment
    /// * `signers` - New signer set (must be non-empty)
    ///
    /// # Errors
    /// * `Unauthorized` - Caller is not the admin
    /// * `NotInitialized` - Contract not initialized
    /// * `InvalidThreshold` - Signer list is empty
    pub fn set_signers(env: Env, signers: Vec<Address>) -> Result<(), MultisigError> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        if signers.is_empty() {
            return Err(MultisigError::InvalidThreshold);
        }

        env.storage().instance().set(&DataKey::Signers, &signers);

        Ok(())
    }

    /// Get the current registered signer set.
    ///
    /// Returns `None` if no signer set has been registered (fallback: only the
    /// admin counts as a signer).
    pub fn get_signers(env: Env) -> Option<Vec<Address>> {
        env.storage().instance().get(&DataKey::Signers)
    }

    /// Add an approval to an open proposal.
    ///
    /// # Quorum-integrity guarantees
    /// * **Deduplication** — a signer who has already approved the proposal
    ///   receives `AlreadyApproved`; duplicate calls never inflate the count.
    /// * **Signer-set membership** — only addresses in the registered signer
    ///   set (or the admin when no signer set is configured) may approve.
    ///
    /// # Arguments
    /// * `env` - Soroban environment
    /// * `id` - Proposal id to approve
    ///
    /// # Errors
    /// * `NotInitialized` - Contract not initialized
    /// * `ProposalNotFound` - Proposal id does not exist
    /// * `ProposalAlreadyExecuted` - Proposal has already been executed
    /// * `ProposalExpired` - Proposal expiry ledger has passed
    /// * `NotASigner` - Caller is not a registered signer
    /// * `AlreadyApproved` - Caller has already approved this proposal
    pub fn approve_proposal(env: Env, approver: Address, id: u64) -> Result<(), MultisigError> {
        approver.require_auth();

        // Ensure contract is initialized.
        let _admin = Self::get_admin(env.clone())?;

        let proposal: Proposal = env
            .storage()
            .instance()
            .get(&DataKey::Proposal(id))
            .ok_or(MultisigError::ProposalNotFound)?;

        if proposal.executed {
            return Err(MultisigError::ProposalAlreadyExecuted);
        }

        let current_ledger = env.ledger().sequence();
        if current_ledger > proposal.expires_at_ledger {
            return Err(MultisigError::ProposalExpired);
        }

        // Check that the approver is a registered signer (or admin when no
        // signer set is configured).
        let is_valid_signer = if let Some(signers) = Self::get_signers(env.clone()) {
            signers.contains(&approver)
        } else {
            // Fallback: admin is the sole implicit signer.
            approver == _admin
        };
        if !is_valid_signer {
            return Err(MultisigError::NotASigner);
        }

        let mut approvals: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ProposalApprovals(id))
            .unwrap_or_else(|| Vec::new(&env));

        // Deduplicate: reject a second approval from the same address.
        if approvals.contains(&approver) {
            return Err(MultisigError::AlreadyApproved);
        }

        approvals.push_back(approver);
        env.storage()
            .instance()
            .set(&DataKey::ProposalApprovals(id), &approvals);

        Ok(())
    }

    /// Execute a stored proposal if quorum is met, it is still fresh, and its
    /// delay has elapsed.
    ///
    /// # Quorum-integrity guarantees at execution time
    /// * **Current-signer validation** — only addresses that are *currently* in
    ///   the registered signer set count toward quorum. A signer removed after
    ///   approving is excluded automatically.
    /// * **Deduplication** — each signer address counts at most once even if
    ///   it appears multiple times in the stored approval list.
    /// * **Threshold** — the effective quorum threshold is read fresh from
    ///   storage at execution time, not captured at proposal creation. A
    ///   threshold raised after approval requires additional approvals before
    ///   the proposal can execute.
    ///
    /// Execution rejects stale approvals once `current_ledger > expires_at_ledger`,
    /// so old quorums cannot be replayed against newer protocol state.
    pub fn execute_proposal(env: Env, id: u64) -> Result<(), MultisigError> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        let mut proposal: Proposal = env
            .storage()
            .instance()
            .get(&DataKey::Proposal(id))
            .ok_or(MultisigError::ProposalNotFound)?;

        if proposal.executed {
            return Err(MultisigError::ProposalAlreadyExecuted);
        }

        let current_ledger = env.ledger().sequence();
        if current_ledger > proposal.expires_at_ledger {
            return Err(MultisigError::ProposalExpired);
        }

        if current_ledger < proposal.eta_ledger {
            return Err(MultisigError::ProposalNotReady);
        }

        // --- Quorum check ---
        // Read the current signer set and threshold fresh at execution time.
        // This ensures:
        //   1. Removed-after-approval signers do not count.
        //   2. A raised threshold requires additional approvals.
        let current_threshold = Self::get_threshold(env.clone())?;
        let approvals: Vec<Address> = env
            .storage()
            .instance()
            .get(&DataKey::ProposalApprovals(id))
            .unwrap_or_else(|| Vec::new(&env));

        // Determine effective signer set.
        let effective_signers: Option<Vec<Address>> = Self::get_signers(env.clone());

        // Count unique approvals from current signers.
        let mut unique_valid: Vec<Address> = Vec::new(&env);
        for addr in approvals.iter() {
            // Skip if already counted (deduplication).
            if unique_valid.contains(&addr) {
                continue;
            }
            // Check membership in current signer set.
            let is_current_signer = if let Some(ref signers) = effective_signers {
                signers.contains(&addr)
            } else {
                // No signer set registered: admin acts as implicit sole signer.
                addr == admin
            };
            if is_current_signer {
                unique_valid.push_back(addr);
            }
        }

        if (unique_valid.len() as u32) < current_threshold {
            return Err(MultisigError::InsufficientApprovals);
        }
        // --- End quorum check ---

        env.storage()
            .instance()
            .set(&DataKey::Threshold, &proposal.new_threshold);
        proposal.executed = true;
        env.storage()
            .instance()
            .set(&DataKey::Proposal(id), &proposal);

        Ok(())
    }

    /// Remove expired proposal and approval records from instance storage.
    ///
    /// Only the admin may run cleanup. Fresh proposals and executed proposals are
    /// retained for auditability; expired unexecuted proposals are safe to remove
    /// because they can never execute. Returns the number of proposals removed.
    pub fn cleanup_expired(env: Env, ids: Vec<u64>) -> Result<u32, MultisigError> {
        let admin = Self::get_admin(env.clone())?;
        admin.require_auth();

        let current_ledger = env.ledger().sequence();
        let mut removed = 0u32;

        for id in ids.iter() {
            if let Some(proposal) = env
                .storage()
                .instance()
                .get::<DataKey, Proposal>(&DataKey::Proposal(id))
            {
                if !proposal.executed && current_ledger > proposal.expires_at_ledger {
                    env.storage().instance().remove(&DataKey::Proposal(id));
                    env.storage()
                        .instance()
                        .remove(&DataKey::ProposalApprovals(id));
                    removed += 1;
                }
            }
        }

        Ok(removed)
    }

    /// Get the default proposal expiry window in ledgers.
    pub fn get_default_expiry_ledgers(_env: Env) -> u32 {
        DEFAULT_PROPOSAL_EXPIRY_LEDGERS
    }

    /// Get the minimum threshold delay in ledgers.
    ///
    /// # Arguments
    /// * `env` - Soroban environment (unused, kept for consistency)
    ///
    /// # Returns
    /// Minimum delay in ledgers
    pub fn get_min_threshold_delay_ledgers(_env: Env) -> u32 {
        MIN_THRESHOLD_DELAY_LEDGERS
    }
}

#[cfg(test)]
mod quorum_edge_test;

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::testutils::Ledger;

    fn setup() -> (Env, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, MultisigContract);
        (env, admin, contract_id)
    }

    fn setup_initialized(threshold: u32) -> (Env, Address, Address) {
        let (env, admin, contract_id) = setup();
        let client = MultisigContractClient::new(&env, &contract_id);
        client.initialize(&admin, &threshold);
        (env, admin, contract_id)
    }

    #[test]
    fn test_initialize_success() {
        let (env, admin, contract_id) = setup();
        let client = MultisigContractClient::new(&env, &contract_id);
        client.initialize(&admin, &5);
        assert_eq!(client.get_threshold(), 5);
        assert_eq!(client.get_admin(), admin);
    }

    #[test]
    fn test_initialize_with_zero_threshold() {
        let (env, admin, contract_id) = setup();
        let client = MultisigContractClient::new(&env, &contract_id);
        assert_eq!(
            client.try_initialize(&admin, &0),
            Err(Ok(MultisigError::InvalidThreshold))
        );
    }

    #[test]
    fn test_initialize_already_initialized() {
        let (env, admin, contract_id) = setup();
        let client = MultisigContractClient::new(&env, &contract_id);
        client.initialize(&admin, &3);
        assert_eq!(
            client.try_initialize(&admin, &5),
            Err(Ok(MultisigError::AlreadyInitialized))
        );
    }

    #[test]
    fn test_get_threshold_not_initialized() {
        let (env, _admin, contract_id) = setup();
        let client = MultisigContractClient::new(&env, &contract_id);
        assert_eq!(
            client.try_get_threshold(),
            Err(Ok(MultisigError::NotInitialized))
        );
    }

    #[test]
    fn test_get_admin_not_initialized() {
        let (env, _admin, contract_id) = setup();
        let client = MultisigContractClient::new(&env, &contract_id);
        assert_eq!(
            client.try_get_admin(),
            Err(Ok(MultisigError::NotInitialized))
        );
    }

    #[test]
    fn test_queue_threshold_change_success() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);

        let initial_ledger = env.ledger().sequence();
        client.queue_threshold_change(&5);

        let pending = client.get_pending_threshold_change();
        assert!(pending.is_some());
        let change = pending.unwrap();
        assert_eq!(change.new_threshold, 5);
        assert_eq!(
            change.eta_ledger,
            initial_ledger + MIN_THRESHOLD_DELAY_LEDGERS
        );
    }

    #[test]
    fn test_queue_threshold_change_not_initialized() {
        let (env, _admin, contract_id) = setup();
        let client = MultisigContractClient::new(&env, &contract_id);
        assert_eq!(
            client.try_queue_threshold_change(&5),
            Err(Ok(MultisigError::NotInitialized))
        );
    }

    #[test]
    fn test_queue_threshold_change_zero_threshold() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);
        assert_eq!(
            client.try_queue_threshold_change(&0),
            Err(Ok(MultisigError::InvalidThreshold))
        );
    }

    /// Verifies that queue_threshold_change panics when admin auth is not provided.
    /// require_auth() enforces auth at the host level; the invocation aborts rather
    /// than returning MultisigError::Unauthorized.
    #[test]
    #[should_panic]
    fn test_queue_threshold_change_unauthorized() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, MultisigContract);
        let client = MultisigContractClient::new(&env, &contract_id);
        client.initialize(&admin, &3);
        client.queue_threshold_change(&5);
    }

    #[test]
    fn test_apply_threshold_change_before_delay() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);
        client.queue_threshold_change(&5);
        assert_eq!(
            client.try_apply_threshold_change(),
            Err(Ok(MultisigError::DelayNotElapsed))
        );
        assert_eq!(client.get_threshold(), 3);
    }

    #[test]
    fn test_apply_threshold_change_after_delay() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);
        client.queue_threshold_change(&5);
        let initial_ledger = env.ledger().sequence();
        env.ledger()
            .set_sequence_number(initial_ledger + MIN_THRESHOLD_DELAY_LEDGERS);
        client.apply_threshold_change();
        assert_eq!(client.get_threshold(), 5);
        assert!(client.get_pending_threshold_change().is_none());
    }

    #[test]
    fn test_apply_threshold_change_no_queued_change() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);
        assert_eq!(
            client.try_apply_threshold_change(),
            Err(Ok(MultisigError::NoQueuedChange))
        );
    }

    /// Verifies that apply_threshold_change panics when admin auth is not provided.
    #[test]
    #[should_panic]
    fn test_apply_threshold_change_unauthorized() {
        let env = Env::default();
        let admin = Address::generate(&env);
        let contract_id = env.register_contract(None, MultisigContract);
        let client = MultisigContractClient::new(&env, &contract_id);
        client.initialize(&admin, &3);
        client.apply_threshold_change();
    }

    #[test]
    fn test_multiple_threshold_changes() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);

        client.queue_threshold_change(&5);
        let initial_ledger = env.ledger().sequence();
        env.ledger()
            .set_sequence_number(initial_ledger + MIN_THRESHOLD_DELAY_LEDGERS);
        client.apply_threshold_change();
        assert_eq!(client.get_threshold(), 5);

        client.queue_threshold_change(&7);
        let second_ledger = env.ledger().sequence();
        env.ledger()
            .set_sequence_number(second_ledger + MIN_THRESHOLD_DELAY_LEDGERS);
        client.apply_threshold_change();
        assert_eq!(client.get_threshold(), 7);
    }

    #[test]
    fn test_overwrite_pending_change() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);

        client.queue_threshold_change(&5);
        assert_eq!(
            client.get_pending_threshold_change().unwrap().new_threshold,
            5
        );

        client.queue_threshold_change(&7);
        assert_eq!(
            client.get_pending_threshold_change().unwrap().new_threshold,
            7
        );

        let initial_ledger = env.ledger().sequence();
        env.ledger()
            .set_sequence_number(initial_ledger + MIN_THRESHOLD_DELAY_LEDGERS);
        client.apply_threshold_change();
        assert_eq!(client.get_threshold(), 7);
    }

    #[test]
    fn test_same_ledger_protection() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);
        client.queue_threshold_change(&1);
        assert_eq!(
            client.try_apply_threshold_change(),
            Err(Ok(MultisigError::DelayNotElapsed))
        );
        assert_eq!(client.get_threshold(), 3);
    }

    #[test]
    fn test_get_min_threshold_delay_ledgers() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);
        assert_eq!(
            client.get_min_threshold_delay_ledgers(),
            MIN_THRESHOLD_DELAY_LEDGERS
        );
    }

    #[test]
    fn test_queue_then_apply_reduces_takeover_window() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);
        client.queue_threshold_change(&1);
        assert_eq!(client.get_threshold(), 3);
        assert_eq!(
            client.try_apply_threshold_change(),
            Err(Ok(MultisigError::DelayNotElapsed))
        );
    }

    #[test]
    fn test_apply_at_exact_eta() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);
        client.queue_threshold_change(&5);
        let eta = client.get_pending_threshold_change().unwrap().eta_ledger;
        env.ledger().set_sequence_number(eta);
        client.apply_threshold_change();
        assert_eq!(client.get_threshold(), 5);
    }

    #[test]
    fn test_apply_after_eta() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);
        let initial_ledger = env.ledger().sequence();
        client.queue_threshold_change(&5);
        env.ledger()
            .set_sequence_number(initial_ledger + MIN_THRESHOLD_DELAY_LEDGERS * 2);
        client.apply_threshold_change();
        assert_eq!(client.get_threshold(), 5);
    }

    #[test]
    fn test_large_threshold_values() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);
        let large_threshold = 1_000_000u32;
        client.queue_threshold_change(&large_threshold);
        let initial_ledger = env.ledger().sequence();
        env.ledger()
            .set_sequence_number(initial_ledger + MIN_THRESHOLD_DELAY_LEDGERS);
        client.apply_threshold_change();
        assert_eq!(client.get_threshold(), large_threshold);
    }

    #[test]
    fn test_execute_fresh_proposal_ok() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);
        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 10;
        let proposal_id = client.create_proposal(&5, &expires_at);
        env.ledger()
            .set_sequence_number(current_ledger + MIN_THRESHOLD_DELAY_LEDGERS);
        client.execute_proposal(&proposal_id);
        assert_eq!(client.get_threshold(), 5);
        let proposal = client.get_proposal(&proposal_id).unwrap();
        assert!(proposal.executed);
        assert_eq!(proposal.expires_at_ledger, expires_at);
    }

    #[test]
    fn test_execute_expired_proposal_rejected() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);
        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS;
        let proposal_id = client.create_proposal(&5, &expires_at);
        env.ledger().set_sequence_number(expires_at + 1);
        assert_eq!(
            client.try_execute_proposal(&proposal_id),
            Err(Ok(MultisigError::ProposalExpired))
        );
        assert_eq!(client.get_threshold(), 3);
        assert!(!client.get_proposal(&proposal_id).unwrap().executed);
    }

    #[test]
    fn test_execute_proposal_at_exact_expiry_ok() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);
        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS;
        let proposal_id = client.create_proposal(&5, &expires_at);
        env.ledger().set_sequence_number(expires_at);
        client.execute_proposal(&proposal_id);
        assert_eq!(client.get_threshold(), 5);
    }

    #[test]
    fn test_execute_proposal_before_eta_rejected() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);
        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 10;
        let proposal_id = client.create_proposal(&5, &expires_at);
        assert_eq!(
            client.try_execute_proposal(&proposal_id),
            Err(Ok(MultisigError::ProposalNotReady))
        );
        assert_eq!(client.get_threshold(), 3);
    }

    #[test]
    fn test_cleanup_expired_frees_keys() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);
        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS;
        let expired_id = client.create_proposal(&5, &expires_at);
        let fresh_id = client.create_proposal(&7, &(expires_at + MIN_THRESHOLD_DELAY_LEDGERS));

        assert!(client.get_proposal(&expired_id).is_some());
        assert!(client.get_proposal_approvals(&expired_id).is_some());

        env.ledger().set_sequence_number(expires_at + 1);
        let mut ids = Vec::new(&env);
        ids.push_back(expired_id);
        ids.push_back(fresh_id);
        assert_eq!(client.cleanup_expired(&ids), 1u32);

        assert!(client.get_proposal(&expired_id).is_none());
        assert!(client.get_proposal_approvals(&expired_id).is_none());
        assert!(client.get_proposal(&fresh_id).is_some());
        assert!(client.get_proposal_approvals(&fresh_id).is_some());
    }

    #[test]
    fn test_create_proposal_rejects_expiry_before_eta() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);
        let current_ledger = env.ledger().sequence();
        let expires_too_soon = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS - 1;
        assert_eq!(
            client.try_create_proposal(&5, &expires_too_soon),
            Err(Ok(MultisigError::InvalidProposal))
        );
    }

    /// ProposalAlreadyExecuted: a second execute_proposal call must be rejected.
    #[test]
    fn test_execute_proposal_double_execution() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);
        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 10;
        let proposal_id = client.create_proposal(&5, &expires_at);
        env.ledger()
            .set_sequence_number(current_ledger + MIN_THRESHOLD_DELAY_LEDGERS);
        client.execute_proposal(&proposal_id);
        assert_eq!(client.get_threshold(), 5);
        assert_eq!(
            client.try_execute_proposal(&proposal_id),
            Err(Ok(MultisigError::ProposalAlreadyExecuted))
        );
        assert_eq!(client.get_threshold(), 5);
    }

    /// ProposalNotFound: executing a non-existent proposal id returns an error.
    #[test]
    fn test_execute_proposal_not_found() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);
        assert_eq!(
            client.try_execute_proposal(&9999u64),
            Err(Ok(MultisigError::ProposalNotFound))
        );
    }

    /// create_proposal_default_expiry: sets expires_at_ledger = current + DEFAULT_PROPOSAL_EXPIRY_LEDGERS.
    #[test]
    fn test_create_proposal_default_expiry() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);
        let current_ledger = env.ledger().sequence();
        let proposal_id = client.create_proposal_default_expiry(&5);
        let proposal = client.get_proposal(&proposal_id).unwrap();
        assert_eq!(
            proposal.expires_at_ledger,
            current_ledger + DEFAULT_PROPOSAL_EXPIRY_LEDGERS
        );
        assert_eq!(proposal.new_threshold, 5);
        assert!(!proposal.executed);
        env.ledger()
            .set_sequence_number(current_ledger + MIN_THRESHOLD_DELAY_LEDGERS);
        client.execute_proposal(&proposal_id);
        assert_eq!(client.get_threshold(), 5);
    }

    /// InvalidThreshold: create_proposal with threshold 0 must be rejected.
    #[test]
    fn test_create_proposal_invalid_threshold() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);
        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 1;
        assert_eq!(
            client.try_create_proposal(&0, &expires_at),
            Err(Ok(MultisigError::InvalidThreshold))
        );
    }

    /// cleanup_expired must keep executed proposals; only unexecuted expired ones are removed.
    #[test]
    fn test_cleanup_retains_executed_proposals() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);
        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS;
        let proposal_id = client.create_proposal(&5, &expires_at);
        env.ledger().set_sequence_number(expires_at);
        client.execute_proposal(&proposal_id);
        env.ledger().set_sequence_number(expires_at + 1);
        let mut ids = Vec::new(&env);
        ids.push_back(proposal_id);
        assert_eq!(client.cleanup_expired(&ids), 0u32);
        assert!(client.get_proposal(&proposal_id).is_some());
    }

    /// Monotonic counter: each new proposal receives a strictly-increasing id.
    #[test]
    fn test_proposal_counter_increments() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);
        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 10;
        let id1 = client.create_proposal(&5, &expires_at);
        let id2 = client.create_proposal(&7, &expires_at);
        let id3 = client.create_proposal(&9, &expires_at);
        assert!(id1 < id2 && id2 < id3);
    }

    /// Applying a threshold change at exactly MIN_THRESHOLD_DELAY_LEDGERS (boundary).
    #[test]
    fn test_apply_at_exact_min_delay_boundary() {
        let (env, _admin, contract_id) = setup_initialized(3);
        let client = MultisigContractClient::new(&env, &contract_id);
        let queue_ledger = env.ledger().sequence();
        client.queue_threshold_change(&5);
        // One ledger before the boundary — must fail.
        env.ledger()
            .set_sequence_number(queue_ledger + MIN_THRESHOLD_DELAY_LEDGERS - 1);
        assert_eq!(
            client.try_apply_threshold_change(),
            Err(Ok(MultisigError::DelayNotElapsed))
        );
        // Exactly at the boundary — must succeed.
        env.ledger()
            .set_sequence_number(queue_ledger + MIN_THRESHOLD_DELAY_LEDGERS);
        client.apply_threshold_change();
        assert_eq!(client.get_threshold(), 5);
    }

    /// Executing a proposal at exactly eta_ledger (the boundary between NotReady and Ready).
    #[test]
    fn test_execute_at_exact_eta_boundary() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);
        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS + 10;
        let proposal_id = client.create_proposal(&7, &expires_at);
        let eta = client.get_proposal(&proposal_id).unwrap().eta_ledger;
        // One ledger before eta — ProposalNotReady.
        env.ledger().set_sequence_number(eta - 1);
        assert_eq!(
            client.try_execute_proposal(&proposal_id),
            Err(Ok(MultisigError::ProposalNotReady))
        );
        // Exactly at eta — must succeed.
        env.ledger().set_sequence_number(eta);
        client.execute_proposal(&proposal_id);
        assert_eq!(client.get_threshold(), 7);
    }

    /// Expiry boundary: proposal is valid at exactly expires_at_ledger, expired one ledger later.
    ///
    /// Both proposals are created upfront so that advancing the ledger for the first
    /// execution does not affect the second proposal's creation validity check.
    #[test]
    fn test_execute_at_expiry_boundary() {
        let (env, _admin, contract_id) = setup_initialized(1);
        let client = MultisigContractClient::new(&env, &contract_id);
        let current_ledger = env.ledger().sequence();
        let expires_at = current_ledger + MIN_THRESHOLD_DELAY_LEDGERS;
        let proposal_id = client.create_proposal(&9, &expires_at);
        let proposal_id2 = client.create_proposal(&11, &expires_at);
        // At exactly expires_at — still valid (contract uses strict >).
        env.ledger().set_sequence_number(expires_at);
        client.execute_proposal(&proposal_id);
        assert_eq!(client.get_threshold(), 9);
        // One ledger past expiry — rejected.
        env.ledger().set_sequence_number(expires_at + 1);
        assert_eq!(
            client.try_execute_proposal(&proposal_id2),
            Err(Ok(MultisigError::ProposalExpired))
        );
    }
}
