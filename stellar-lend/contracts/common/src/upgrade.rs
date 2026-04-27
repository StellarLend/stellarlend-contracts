#![allow(clippy::enum_variant_names)]

use soroban_sdk::{
    contracterror, contracttype, panic_with_error, symbol_short, Address, BytesN, Env, Vec,
};

/// Initial version written into upgrade storage during initialization.
pub const INITIAL_CONTRACT_VERSION: u32 = 0;

/// Upper bound on tracked upgrade approvers.
///
/// The bound protects storage growth and event fan-out in shared upgrade flows.
pub const MAX_UPGRADE_APPROVERS: u32 = 32;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum UpgradeError {
    /// Upgrade storage was already initialized.
    AlreadyInitialized = 1,
    /// Upgrade storage is required but missing.
    NotInitialized = 2,
    /// Caller is not authorized for the requested operation.
    NotAuthorized = 3,
    /// Proposal id does not exist.
    ProposalNotFound = 4,
    /// Proposed version or version transition is invalid.
    InvalidVersion = 5,
    /// Proposal stage does not permit the requested operation.
    InvalidStatus = 6,
    /// Caller has already approved the proposal.
    AlreadyApproved = 7,
    /// Proposal has not yet reached the configured threshold.
    NotEnoughApprovals = 8,
    /// Threshold or approval-set configuration is invalid.
    InvalidThreshold = 9,
    /// Proposal id allocation overflowed.
    ProposalIdOverflow = 10,
    /// Stored upgrade data violates module invariants.
    StorageCorrupted = 11,
    /// Approver count exceeds the bounded shared limit.
    TooManyApprovers = 12,
}

#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum UpgradeStage {
    /// Proposal exists but has not yet reached threshold.
    Proposed,
    /// Proposal has enough approvals and can be executed.
    Approved,
    /// Proposal was executed and can be rolled back if prior state was captured.
    Executed,
    /// Proposal was executed and then rolled back.
    RolledBack,
}

/// Persistent upgrade proposal shared across contracts.
///
/// # Security
/// The `approvals` vector is bounded by [`MAX_UPGRADE_APPROVERS`], and `stage` must remain
/// consistent with the captured approval count.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeProposal {
    /// Monotonic identifier allocated from storage.
    pub id: u64,
    /// Admin address that created the proposal.
    pub proposer: Address,
    /// Target WASM hash to deploy.
    pub new_wasm_hash: BytesN<32>,
    /// Monotonic target version for the proposal.
    pub new_version: u32,
    /// Unique set of approvers that have approved this proposal.
    pub approvals: Vec<Address>,
    /// Current lifecycle stage.
    pub stage: UpgradeStage,
    /// Previous WASM hash captured at execution time for rollback.
    pub prev_wasm_hash: Option<BytesN<32>>,
    /// Previous version captured at execution time for rollback.
    pub prev_version: Option<u32>,
}

impl UpgradeProposal {
    /// Converts a stored proposal into a summarized status after validating shared invariants.
    ///
    /// # Errors
    /// Returns:
    /// - [`UpgradeError::InvalidThreshold`] if `required_approvals` is zero or exceeds
    ///   [`MAX_UPGRADE_APPROVERS`].
    /// - [`UpgradeError::TooManyApprovers`] if the stored approval set exceeds the shared bound.
    /// - [`UpgradeError::StorageCorrupted`] if the stage is inconsistent with approval counts or
    ///   rollback metadata.
    ///
    /// # Security
    /// Consumers should prefer this conversion instead of reconstructing status fields manually so
    /// approval thresholds and stage invariants are checked consistently across crates.
    pub fn try_into_status(&self, required_approvals: u32) -> Result<UpgradeStatus, UpgradeError> {
        validate_required_approvals(required_approvals)?;
        let approval_count = checked_approval_count(&self.approvals)?;
        validate_stage_invariants(self, approval_count, required_approvals)?;

        Ok(UpgradeStatus {
            id: self.id,
            stage: self.stage,
            approval_count,
            required_approvals,
            target_version: self.new_version,
        })
    }
}

/// Shared proposal status returned by contract entrypoints.
///
/// # Security
/// Instances should be created via [`UpgradeProposal::try_into_status`] so the returned counts and
/// stage reflect validated storage state.
#[contracttype]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct UpgradeStatus {
    /// Proposal identifier.
    pub id: u64,
    /// Current lifecycle stage.
    pub stage: UpgradeStage,
    /// Number of unique approvals recorded on the proposal.
    pub approval_count: u32,
    /// Threshold required for execution.
    pub required_approvals: u32,
    /// Requested version after execution.
    pub target_version: u32,
}

// collisions with other contracts sharing the same Soroban persistent storage.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
enum UpgradeKey {
    UpAdmin,
    UpApprovers,
    UpReqApprovals,
    UpNextPropId,
    UpCurrWasmHash,
    UpCurrVersion,
    UpProposal(u64),
}

pub struct UpgradeManager;

impl UpgradeManager {
    /// Initializes upgrade storage for a contract instance.
    ///
    /// # Errors
    /// Panics with:
    /// - [`UpgradeError::AlreadyInitialized`] if called twice.
    /// - [`UpgradeError::InvalidThreshold`] if `required_approvals` is zero or exceeds
    ///   [`MAX_UPGRADE_APPROVERS`].
    ///
    /// # Security
    /// The admin is inserted as the initial approver so every configuration begins with at least
    /// one authorized execution path.
    #[allow(deprecated)]
    pub fn init(env: Env, admin: Address, current_wasm_hash: BytesN<32>, required_approvals: u32) {
        if env.storage().persistent().has(&UpgradeKey::UpAdmin) {
            panic_with_error!(&env, UpgradeError::AlreadyInitialized);
        }
        unwrap_or_panic(&env, validate_required_approvals(required_approvals));

        let mut approvers = Vec::new(&env);
        approvers.push_back(admin.clone());

        env.storage().persistent().set(&UpgradeKey::UpAdmin, &admin);
        env.storage()
            .persistent()
            .set(&UpgradeKey::UpApprovers, &approvers);
        env.storage()
            .persistent()
            .set(&UpgradeKey::UpReqApprovals, &required_approvals);
        env.storage()
            .persistent()
            .set(&UpgradeKey::UpNextPropId, &1u64);
        env.storage()
            .persistent()
            .set(&UpgradeKey::UpCurrWasmHash, &current_wasm_hash);
        env.storage()
            .persistent()
            .set(&UpgradeKey::UpCurrVersion, &0u32);

        #[allow(deprecated)]
        env.events()
            .publish((symbol_short!("up_init"), admin), required_approvals);
    }

    /// Adds a new approver address.
    ///
    /// # Errors
    /// Panics with:
    /// - [`UpgradeError::NotInitialized`] if storage is missing.
    /// - [`UpgradeError::NotAuthorized`] if `caller` is not the admin.
    /// - [`UpgradeError::TooManyApprovers`] if the shared approver bound would be exceeded.
    ///
    /// # Security
    /// The approver set is deduplicated and bounded to prevent unbounded storage growth.
    #[allow(deprecated)]
    pub fn add_approver(env: Env, caller: Address, approver: Address) {
        caller.require_auth();
        Self::assert_admin(&env, &caller);

        let mut approvers = Self::approvers(&env);
        if !approvers.contains(&approver) {
            if approvers.len() >= MAX_UPGRADE_APPROVERS {
                panic_with_error!(&env, UpgradeError::TooManyApprovers);
            }
            approvers.push_back(approver.clone());
            env.storage()
                .persistent()
                .set(&UpgradeKey::UpApprovers, &approvers);
        }

        #[allow(deprecated)]
        env.events()
            .publish((symbol_short!("up_apadd"), caller, approver), ());
    }

    /// Removes an approver address.
    ///
    /// # Errors
    /// Panics with:
    /// - [`UpgradeError::NotInitialized`] if storage is missing.
    /// - [`UpgradeError::NotAuthorized`] if `caller` is not the admin.
    /// - [`UpgradeError::InvalidThreshold`] if removal would leave the threshold unsatisfiable.
    ///
    /// # Security
    /// Removal never permits a state where `required_approvals` exceeds the number of stored
    /// approvers.
    #[allow(deprecated)]
    pub fn remove_approver(env: Env, caller: Address, approver: Address) {
        caller.require_auth();
        Self::assert_admin(&env, &caller);

        let approvers = Self::approvers(&env);
        let mut updated = Vec::new(&env);
        for existing in approvers.iter() {
            if existing != approver {
                updated.push_back(existing);
            }
        }

        if updated.len() == approvers.len() {
            return;
        }
        if updated.is_empty() || updated.len() < Self::required_approvals(env.clone()) {
            panic_with_error!(&env, UpgradeError::InvalidThreshold);
        }

        env.storage()
            .persistent()
            .set(&UpgradeKey::UpApprovers, &updated);
        env.events()
            .publish((symbol_short!("up_aprm"), caller, approver), ());
    }

    /// Creates a new upgrade proposal.
    ///
    /// # Errors
    /// Panics with:
    /// - [`UpgradeError::NotInitialized`] if storage is missing.
    /// - [`UpgradeError::NotAuthorized`] if `caller` is not the admin.
    /// - [`UpgradeError::InvalidVersion`] if `new_version` is not strictly greater than current.
    /// - [`UpgradeError::ProposalIdOverflow`] if the stored proposal counter overflows.
    ///
    /// # Security
    /// The proposer's approval is recorded immediately so the status is always derived from a
    /// consistent approval set.
    pub fn upgrade_propose(
        env: Env,
        caller: Address,
        new_wasm_hash: BytesN<32>,
        new_version: u32,
    ) -> u64 {
        caller.require_auth();
        Self::assert_admin(&env, &caller);

        let current_version = Self::current_version(env.clone());
        if new_version <= current_version {
            panic_with_error!(&env, UpgradeError::InvalidVersion);
        }

        let mut approvals = Vec::new(&env);
        approvals.push_back(caller.clone());

        let required = Self::required_approvals(env.clone());
        let stage = if approvals.len() >= required {
            UpgradeStage::Approved
        } else {
            UpgradeStage::Proposed
        };

        let id: u64 = env
            .storage()
            .persistent()
            .get(&UpgradeKey::UpNextPropId)
            .unwrap_or(1);
        let proposal = UpgradeProposal {
            id,
            proposer: caller.clone(),
            new_wasm_hash,
            new_version,
            approvals,
            stage,
            prev_wasm_hash: None,
            prev_version: None,
        };

        env.storage()
            .persistent()
            .set(&UpgradeKey::UpProposal(id), &proposal);
        env.storage()
            .persistent()
            .set(&UpgradeKey::UpNextPropId, &(id + 1));

        #[allow(deprecated)]
        env.events()
            .publish((symbol_short!("up_prop"), caller, id), new_version);
        id
    }

    /// Records an approver vote for an existing proposal.
    ///
    /// # Errors
    /// Panics with:
    /// - [`UpgradeError::NotInitialized`] if storage is missing.
    /// - [`UpgradeError::NotAuthorized`] if `caller` is not an approver.
    /// - [`UpgradeError::ProposalNotFound`] if the proposal id is absent.
    /// - [`UpgradeError::InvalidStatus`] if the proposal is no longer actionable.
    /// - [`UpgradeError::AlreadyApproved`] if `caller` has already approved.
    #[allow(deprecated)]
    pub fn upgrade_approve(env: Env, caller: Address, proposal_id: u64) -> u32 {
        caller.require_auth();
        Self::assert_approver(&env, &caller);

        let mut proposal = Self::proposal(env.clone(), proposal_id);
        if proposal.stage != UpgradeStage::Proposed && proposal.stage != UpgradeStage::Approved {
            panic_with_error!(&env, UpgradeError::InvalidStatus);
        }
        if proposal.approvals.contains(&caller) {
            panic_with_error!(&env, UpgradeError::AlreadyApproved);
        }

        proposal.approvals.push_back(caller.clone());
        if proposal.approvals.len() >= Self::required_approvals(env.clone()) {
            proposal.stage = UpgradeStage::Approved;
        }
        let count = proposal.approvals.len();

        env.storage()
            .persistent()
            .set(&UpgradeKey::UpProposal(proposal_id), &proposal);
        #[allow(deprecated)]
        env.events()
            .publish((symbol_short!("up_appr"), caller, proposal_id), count);
        count
    }

    /// Executes an approved upgrade proposal.
    ///
    /// # Errors
    /// Panics with:
    /// - [`UpgradeError::NotInitialized`] if storage is missing.
    /// - [`UpgradeError::NotAuthorized`] if `caller` is not an approver.
    /// - [`UpgradeError::ProposalNotFound`] if the proposal id is absent.
    /// - [`UpgradeError::InvalidStatus`] if the proposal has not reached `Approved`.
    ///
    /// # Security
    /// Execution snapshots the prior WASM hash and version before updating contract code so admin
    /// rollback has explicit state to restore.
    #[allow(deprecated)]
    pub fn upgrade_execute(env: Env, caller: Address, proposal_id: u64) {
        caller.require_auth();
        Self::assert_approver(&env, &caller);

        let mut proposal = Self::proposal(env.clone(), proposal_id);
        if proposal.stage != UpgradeStage::Approved {
            panic_with_error!(&env, UpgradeError::InvalidStatus);
        }

        let current_hash = Self::current_wasm_hash(env.clone());
        let current_version = Self::current_version(env.clone());
        proposal.prev_wasm_hash = Some(current_hash.clone());
        proposal.prev_version = Some(current_version);
        proposal.stage = UpgradeStage::Executed;

        #[cfg(not(any(test, feature = "testutils")))]
        env.deployer()
            .update_current_contract_wasm(proposal.new_wasm_hash.clone());

        env.storage()
            .persistent()
            .set(&UpgradeKey::UpCurrWasmHash, &proposal.new_wasm_hash);
        env.storage()
            .persistent()
            .set(&UpgradeKey::UpCurrVersion, &proposal.new_version);
        env.storage()
            .persistent()
            .set(&UpgradeKey::UpProposal(proposal_id), &proposal);

        #[allow(deprecated)]
        env.events().publish(
            (symbol_short!("up_exec"), caller, proposal_id),
            proposal.new_version,
        );
    }

    /// Rolls back a previously executed proposal.
    ///
    /// # Errors
    /// Panics with:
    /// - [`UpgradeError::NotInitialized`] if storage is missing.
    /// - [`UpgradeError::NotAuthorized`] if `caller` is not the admin.
    /// - [`UpgradeError::ProposalNotFound`] if the proposal id is absent.
    /// - [`UpgradeError::InvalidStatus`] if the proposal was not executed.
    /// - [`UpgradeError::StorageCorrupted`] if rollback metadata is missing.
    #[allow(deprecated)]
    pub fn upgrade_rollback(env: Env, caller: Address, proposal_id: u64) {
        caller.require_auth();
        Self::assert_admin(&env, &caller);

        let mut proposal = Self::proposal(env.clone(), proposal_id);
        if proposal.stage != UpgradeStage::Executed {
            panic_with_error!(&env, UpgradeError::InvalidStatus);
        }

        let prev_hash = proposal.prev_wasm_hash.clone().unwrap();
        let prev_version = proposal.prev_version.unwrap();

        #[cfg(not(any(test, feature = "testutils")))]
        env.deployer()
            .update_current_contract_wasm(prev_hash.clone());

        env.storage()
            .persistent()
            .set(&UpgradeKey::UpCurrWasmHash, &prev_hash);
        env.storage()
            .persistent()
            .set(&UpgradeKey::UpCurrVersion, &prev_version);

        proposal.stage = UpgradeStage::RolledBack;
        env.storage()
            .persistent()
            .set(&UpgradeKey::UpProposal(proposal_id), &proposal);

        #[allow(deprecated)]
        env.events().publish(
            (symbol_short!("up_roll"), caller, proposal_id),
            prev_version,
        );
    }

    /// Returns a validated shared status view for a proposal.
    ///
    /// # Errors
    /// Panics with:
    /// - [`UpgradeError::ProposalNotFound`] if the proposal id is absent.
    /// - [`UpgradeError::InvalidThreshold`] or [`UpgradeError::StorageCorrupted`] if storage
    ///   contents violate shared invariants.
    pub fn upgrade_status(env: Env, proposal_id: u64) -> UpgradeStatus {
        let proposal = Self::proposal(env.clone(), proposal_id);
        let required_approvals = Self::required_approvals(env.clone());
        unwrap_or_panic(&env, proposal.try_into_status(required_approvals))
    }

    /// Returns the currently recorded WASM hash.
    ///
    /// # Errors
    /// Panics with [`UpgradeError::NotInitialized`] if upgrade storage is missing.
    pub fn current_wasm_hash(env: Env) -> BytesN<32> {
        env.storage()
            .persistent()
            .get(&UpgradeKey::UpCurrWasmHash)
            .unwrap()
    }

    /// Returns the currently recorded contract version.
    ///
    /// # Errors
    /// Panics with [`UpgradeError::NotInitialized`] if upgrade storage is missing.
    pub fn current_version(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&UpgradeKey::UpCurrVersion)
            .unwrap_or(0)
    }

    fn required_approvals(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&UpgradeKey::UpReqApprovals)
            .unwrap_or(0)
    }

    fn proposal(env: Env, proposal_id: u64) -> UpgradeProposal {
        env.storage()
            .persistent()
            .get(&UpgradeKey::UpProposal(proposal_id))
            .unwrap_or_else(|| panic_with_error!(&env, UpgradeError::ProposalNotFound))
    }

    fn approvers(env: &Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&UpgradeKey::UpApprovers)
            .unwrap_or_else(|| Vec::new(env))
    }

    fn assert_admin(env: &Env, caller: &Address) {
        let admin: Address = env
            .storage()
            .persistent()
            .get(&UpgradeKey::UpAdmin)
            .unwrap();
        if *caller != admin {
            panic_with_error!(env, UpgradeError::NotAuthorized);
        }
    }

    fn assert_approver(env: &Env, caller: &Address) {
        if !Self::approvers(env).contains(caller) {
            panic_with_error!(env, UpgradeError::NotAuthorized);
        }
    }
}

fn validate_required_approvals(required_approvals: u32) -> Result<(), UpgradeError> {
    if required_approvals == 0 || required_approvals > MAX_UPGRADE_APPROVERS {
        return Err(UpgradeError::InvalidThreshold);
    }
    Ok(())
}

fn checked_approval_count(approvals: &Vec<Address>) -> Result<u32, UpgradeError> {
    let approval_count = approvals.len();
    if approval_count > MAX_UPGRADE_APPROVERS {
        return Err(UpgradeError::TooManyApprovers);
    }
    Ok(approval_count)
}

fn validate_stage_invariants(
    proposal: &UpgradeProposal,
    approval_count: u32,
    required_approvals: u32,
) -> Result<(), UpgradeError> {
    match proposal.stage {
        UpgradeStage::Proposed if approval_count >= required_approvals => {
            Err(UpgradeError::StorageCorrupted)
        }
        UpgradeStage::Approved if approval_count < required_approvals => {
            Err(UpgradeError::StorageCorrupted)
        }
        UpgradeStage::Executed | UpgradeStage::RolledBack
            if approval_count < required_approvals =>
        {
            Err(UpgradeError::StorageCorrupted)
        }
        UpgradeStage::Executed | UpgradeStage::RolledBack
            if proposal.prev_wasm_hash.is_none() || proposal.prev_version.is_none() =>
        {
            Err(UpgradeError::StorageCorrupted)
        }
        _ => Ok(()),
    }
}

fn unwrap_or_panic<T>(env: &Env, result: Result<T, UpgradeError>) -> T {
    match result {
        Ok(value) => value,
        Err(error) => panic_with_error!(env, error),
    }
}

#[cfg(test)]
mod tests {
    extern crate std;

    use super::*;
    use soroban_sdk::{contract, contractimpl, testutils::Address as _};

    #[contract]
    struct DummyContract;

    #[contractimpl]
    impl DummyContract {
        pub fn noop(_env: Env) {}
    }

    fn setup() -> (Env, Address, Address, Address, BytesN<32>) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register(DummyContract, ());
        let admin = Address::generate(&env);
        let approver = Address::generate(&env);
        let wasm_hash = BytesN::from_array(&env, &[7; 32]);
        (env, contract_id, admin, approver, wasm_hash)
    }

    #[test]
    fn proposal_to_status_validates_counts_and_stage() {
        let (env, _contract_id, admin, approver, wasm_hash) = setup();
        let mut approvals = Vec::new(&env);
        approvals.push_back(admin);
        approvals.push_back(approver);

        let proposal = UpgradeProposal {
            id: 3,
            proposer: Address::generate(&env),
            new_wasm_hash: wasm_hash,
            new_version: 9,
            approvals,
            stage: UpgradeStage::Approved,
            prev_wasm_hash: None,
            prev_version: None,
        };

        let status = proposal.try_into_status(2).unwrap();
        assert_eq!(status.id, 3);
        assert_eq!(status.stage, UpgradeStage::Approved);
        assert_eq!(status.approval_count, 2);
        assert_eq!(status.required_approvals, 2);
        assert_eq!(status.target_version, 9);
    }

    #[test]
    fn proposal_to_status_rejects_corrupted_stage_data() {
        let (env, _contract_id, admin, _approver, wasm_hash) = setup();
        let mut approvals = Vec::new(&env);
        approvals.push_back(admin);

        let proposal = UpgradeProposal {
            id: 1,
            proposer: Address::generate(&env),
            new_wasm_hash: wasm_hash,
            new_version: 2,
            approvals,
            stage: UpgradeStage::Approved,
            prev_wasm_hash: None,
            prev_version: None,
        };

        assert_eq!(
            proposal.try_into_status(2),
            Err(UpgradeError::StorageCorrupted)
        );
    }

    #[test]
    fn init_rejects_zero_threshold() {
        let (env, contract_id, admin, _approver, wasm_hash) = setup();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            env.as_contract(&contract_id, || {
                UpgradeManager::init(env.clone(), admin.clone(), wasm_hash.clone(), 0);
            });
        }));

        assert!(result.is_err());
    }

    #[test]
    fn add_remove_approver_and_status_flow() {
        let (env, contract_id, admin, approver, wasm_hash) = setup();

        env.as_contract(&contract_id, || {
            UpgradeManager::init(env.clone(), admin.clone(), wasm_hash.clone(), 2);
            UpgradeManager::add_approver(env.clone(), admin.clone(), approver.clone());

            let proposal_id =
                UpgradeManager::upgrade_propose(env.clone(), admin.clone(), wasm_hash.clone(), 1);
            let status_before = UpgradeManager::upgrade_status(env.clone(), proposal_id);
            assert_eq!(status_before.stage, UpgradeStage::Proposed);
            assert_eq!(status_before.approval_count, 1);

            let count = UpgradeManager::upgrade_approve(env.clone(), approver.clone(), proposal_id);
            assert_eq!(count, 2);

            let status_after = UpgradeManager::upgrade_status(env.clone(), proposal_id);
            assert_eq!(status_after.stage, UpgradeStage::Approved);
            assert_eq!(status_after.approval_count, 2);

            UpgradeManager::remove_approver(env.clone(), admin.clone(), approver.clone());
        });
    }

    #[test]
    fn remove_approver_rejects_unsatisfied_threshold() {
        let (env, contract_id, admin, approver, wasm_hash) = setup();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            env.as_contract(&contract_id, || {
                UpgradeManager::init(env.clone(), admin.clone(), wasm_hash.clone(), 2);
                UpgradeManager::add_approver(env.clone(), admin.clone(), approver.clone());
                UpgradeManager::remove_approver(env.clone(), admin.clone(), approver.clone());
                UpgradeManager::remove_approver(env.clone(), admin.clone(), admin.clone());
            });
        }));

        assert!(result.is_err());
    }

    #[test]
    fn execute_and_rollback_update_recorded_version_state() {
        let (env, contract_id, admin, approver, wasm_hash) = setup();
        let next_hash = BytesN::from_array(&env, &[9; 32]);

        env.as_contract(&contract_id, || {
            UpgradeManager::init(env.clone(), admin.clone(), wasm_hash.clone(), 2);
            UpgradeManager::add_approver(env.clone(), admin.clone(), approver.clone());

            let proposal_id =
                UpgradeManager::upgrade_propose(env.clone(), admin.clone(), next_hash.clone(), 1);
            UpgradeManager::upgrade_approve(env.clone(), approver.clone(), proposal_id);
            UpgradeManager::upgrade_execute(env.clone(), approver.clone(), proposal_id);

            assert_eq!(UpgradeManager::current_version(env.clone()), 1);
            assert_eq!(UpgradeManager::current_wasm_hash(env.clone()), next_hash);

            let executed_status = UpgradeManager::upgrade_status(env.clone(), proposal_id);
            assert_eq!(executed_status.stage, UpgradeStage::Executed);

            UpgradeManager::upgrade_rollback(env.clone(), admin.clone(), proposal_id);
            assert_eq!(UpgradeManager::current_version(env.clone()), 0);
            assert_eq!(UpgradeManager::current_wasm_hash(env.clone()), wasm_hash);

            let rolled_back_status = UpgradeManager::upgrade_status(env.clone(), proposal_id);
            assert_eq!(rolled_back_status.stage, UpgradeStage::RolledBack);
        });
    }

    #[test]
    fn proposal_id_overflow_is_rejected() {
        let (env, contract_id, admin, _approver, wasm_hash) = setup();

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            env.as_contract(&contract_id, || {
                UpgradeManager::init(env.clone(), admin.clone(), wasm_hash.clone(), 1);
                env.storage()
                    .persistent()
                    .set(&UpgradeKey::NextProposalId, &u64::MAX);
                UpgradeManager::upgrade_propose(env.clone(), admin.clone(), wasm_hash.clone(), 1);
            });
        }));

        assert!(result.is_err());
    }
}
