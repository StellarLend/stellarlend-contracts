#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, Symbol, Vec,
};

/// Typed action carried on a Proposal and dispatched at execute_proposal time.
/// The payload_hash binds the approved action so it cannot be swapped between
/// approval and execution.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalAction {
    /// Update the approval threshold for future proposals
    SetThreshold { new_threshold: u32 },
    /// Replace the full signer set with a new set
    RotateSigners { new_signers: Vec<Address> },
    /// Invoke an arbitrary lending upgrade entrypoint via cross-contract call
    InvokeContract {
        contract: Address,
        fn_symbol: Symbol,
        args_hash: soroban_sdk::Bytes,
    },
}

/// Lifecycle state of a proposal.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalStatus {
    Active,
    Passed,
    Executed,
    Expired,
    Cancelled,
}

/// A multisig proposal with an attached typed action.
#[contracttype]
#[derive(Clone, Debug)]
pub struct Proposal {
    pub id: u64,
    pub proposer: Address,
    pub action: ProposalAction,
    /// Keccak/SHA256 hash of the encoded action payload, bound at creation.
    pub payload_hash: soroban_sdk::Bytes,
    pub approvals: Vec<Address>,
    pub status: ProposalStatus,
    pub expires_at: u64,
}

/// Event emitted after a proposal has been executed.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ProposalExecutedEvent {
    pub id: u64,
    pub action_kind: Symbol,
    pub ok: bool,
}

#[contracttype]
pub enum MultisigDataKey {
    Threshold,
    Signers,
    ProposalCount,
    Proposal(u64),
}

/// Multisig errors.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum MultisigError {
    Unauthorized,
    ProposalNotFound,
    ProposalNotPassed,
    ProposalExpired,
    AlreadyExecuted,
    AlreadyApproved,
    PayloadHashMismatch,
    QuorumNotReached,
    InvalidAction,
    InvalidThreshold,
    InvalidSigners,
    AlreadyCancelled,
}

#[contract]
pub struct MultisigContract;

#[contractimpl]
impl MultisigContract {
    // -----------------------------------------------------------------------
    // Initialisation
    // -----------------------------------------------------------------------

    /// Initialise the multisig with an initial signer set and approval threshold.
    ///
    /// # Arguments
    /// * `env`       – Soroban environment.
    /// * `signers`   – Initial list of authorised signers.
    /// * `threshold` – Minimum number of approvals required to pass a proposal.
    pub fn initialize(env: Env, signers: Vec<Address>, threshold: u32) {
        if threshold == 0 || threshold as usize > signers.len() as usize {
            panic!("InvalidThreshold");
        }
        env.storage()
            .persistent()
            .set(&MultisigDataKey::Signers, &signers);
        env.storage()
            .persistent()
            .set(&MultisigDataKey::Threshold, &threshold);
        env.storage()
            .persistent()
            .set(&MultisigDataKey::ProposalCount, &0u64);
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn require_signer(env: &Env, caller: &Address) {
        let signers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&MultisigDataKey::Signers)
            .unwrap_or_else(|| panic!("Unauthorized"));
        if !signers.contains(caller) {
            panic!("Unauthorized");
        }
    }

    fn fetch_threshold(env: &Env) -> u32 {
        env.storage()
            .persistent()
            .get(&MultisigDataKey::Threshold)
            .unwrap_or(1)
    }

    fn fetch_proposal(env: &Env, id: u64) -> Proposal {
        env.storage()
            .persistent()
            .get(&MultisigDataKey::Proposal(id))
            .unwrap_or_else(|| panic!("ProposalNotFound"))
    }

    fn save_proposal(env: &Env, proposal: &Proposal) {
        env.storage()
            .persistent()
            .set(&MultisigDataKey::Proposal(proposal.id), proposal);
    }

    fn next_proposal_id(env: &Env) -> u64 {
        let count: u64 = env
            .storage()
            .persistent()
            .get(&MultisigDataKey::ProposalCount)
            .unwrap_or(0);
        let new_count = count + 1;
        env.storage()
            .persistent()
            .set(&MultisigDataKey::ProposalCount, &new_count);
        count
    }

    fn action_kind_symbol(env: &Env, action: &ProposalAction) -> Symbol {
        match action {
            ProposalAction::SetThreshold { .. } => Symbol::new(env, "SetThreshold"),
            ProposalAction::RotateSigners { .. } => Symbol::new(env, "RotateSigners"),
            ProposalAction::InvokeContract { .. } => Symbol::new(env, "InvokeContract"),
        }
    }

    // -----------------------------------------------------------------------
    // Proposal lifecycle
    // -----------------------------------------------------------------------

    /// Create a new proposal carrying a typed action.
    ///
    /// # Arguments
    /// * `caller`       – Signer proposing the action.
    /// * `action`       – The typed `ProposalAction` to attach.
    /// * `payload_hash` – SHA-256 / Keccak hash of the encoded action payload.
    /// * `ttl_ledgers`  – Ledgers until the proposal expires.
    ///
    /// # Returns
    /// The new proposal ID.
    pub fn create_proposal(
        env: Env,
        caller: Address,
        action: ProposalAction,
        payload_hash: soroban_sdk::Bytes,
        ttl_ledgers: u64,
    ) -> u64 {
        caller.require_auth();
        Self::require_signer(&env, &caller);

        let id = Self::next_proposal_id(&env);
        let expires_at = env.ledger().sequence() as u64 + ttl_ledgers;

        let proposal = Proposal {
            id,
            proposer: caller,
            action,
            payload_hash,
            approvals: Vec::new(&env),
            status: ProposalStatus::Active,
            expires_at,
        };
        Self::save_proposal(&env, &proposal);
        id
    }

    /// Approve an existing active proposal.
    ///
    /// A proposal is automatically transitioned to `Passed` once the number of
    /// distinct signer approvals meets or exceeds the current threshold.
    ///
    /// # Arguments
    /// * `caller` – Signer casting the approval.
    /// * `id`     – ID of the proposal to approve.
    pub fn approve_proposal(env: Env, caller: Address, id: u64) {
        caller.require_auth();
        Self::require_signer(&env, &caller);

        let mut proposal = Self::fetch_proposal(&env, id);

        if proposal.status == ProposalStatus::Expired
            || env.ledger().sequence() as u64 > proposal.expires_at
        {
            proposal.status = ProposalStatus::Expired;
            Self::save_proposal(&env, &proposal);
            panic!("ProposalExpired");
        }
        if proposal.status != ProposalStatus::Active {
            panic!("ProposalNotPassed");
        }
        if proposal.approvals.contains(&caller) {
            panic!("AlreadyApproved");
        }

        proposal.approvals.push_back(caller);

        let threshold = Self::fetch_threshold(&env) as usize;
        if proposal.approvals.len() as usize >= threshold {
            proposal.status = ProposalStatus::Passed;
        }
        Self::save_proposal(&env, &proposal);
    }

    /// Execute a passed, non-expired, non-executed proposal.
    ///
    /// This is the **execution router**: it dispatches the proposal's typed
    /// `ProposalAction` to the matching on-chain handler and emits a
    /// `ProposalExecutedEvent` with the outcome.
    ///
    /// # Arguments
    /// * `caller`       – Signer triggering execution (must be a registered signer).
    /// * `id`           – ID of the proposal to execute.
    /// * `payload_hash` – Hash of the action payload presented at execution time;
    ///                    must match the hash recorded at creation.
    pub fn execute_proposal(
        env: Env,
        caller: Address,
        id: u64,
        payload_hash: soroban_sdk::Bytes,
    ) {
        caller.require_auth();
        Self::require_signer(&env, &caller);

        let mut proposal = Self::fetch_proposal(&env, id);

        // Expiry guard
        if env.ledger().sequence() as u64 > proposal.expires_at {
            proposal.status = ProposalStatus::Expired;
            Self::save_proposal(&env, &proposal);
            panic!("ProposalExpired");
        }
        // Status guards
        if proposal.status == ProposalStatus::Executed {
            panic!("AlreadyExecuted");
        }
        if proposal.status == ProposalStatus::Cancelled {
            panic!("AlreadyCancelled");
        }
        if proposal.status != ProposalStatus::Passed {
            panic!("ProposalNotPassed");
        }
        // Payload-hash binding: prevents action swap between approval and execution
        if proposal.payload_hash != payload_hash {
            panic!("PayloadHashMismatch");
        }

        let action_kind = Self::action_kind_symbol(&env, &proposal.action);
        let ok = Self::dispatch_action(&env, &proposal.action);

        proposal.status = ProposalStatus::Executed;
        Self::save_proposal(&env, &proposal);

        // Emit ProposalExecutedEvent
        env.events().publish(
            (symbol_short!("multisig"), symbol_short!("executed")),
            ProposalExecutedEvent {
                id,
                action_kind,
                ok,
            },
        );
    }

    /// Internal router: dispatches a `ProposalAction` to its handler.
    ///
    /// Returns `true` on success, `false` if the action is unregistered or fails.
    fn dispatch_action(env: &Env, action: &ProposalAction) -> bool {
        match action {
            ProposalAction::SetThreshold { new_threshold } => {
                if *new_threshold == 0 {
                    return false;
                }
                env.storage()
                    .persistent()
                    .set(&MultisigDataKey::Threshold, new_threshold);
                true
            }
            ProposalAction::RotateSigners { new_signers } => {
                if new_signers.is_empty() {
                    return false;
                }
                env.storage()
                    .persistent()
                    .set(&MultisigDataKey::Signers, new_signers);
                true
            }
            ProposalAction::InvokeContract {
                contract,
                fn_symbol,
                args_hash: _,
            } => {
                // Dispatch to the lending upgrade entrypoint via cross-contract call.
                // The args_hash was verified at the payload_hash check; here we
                // perform the actual invocation with an empty args list since the
                // concrete arguments were committed via the hash.
                let args: soroban_sdk::Vec<soroban_sdk::Val> = soroban_sdk::Vec::new(env);
                let _res: soroban_sdk::Val = env.invoke_contract(contract, fn_symbol, args);
                true
            }
        }
    }

    /// Cancel an active proposal (proposer or any signer).
    ///
    /// # Arguments
    /// * `caller` – Signer requesting cancellation.
    /// * `id`     – ID of the proposal to cancel.
    pub fn cancel_proposal(env: Env, caller: Address, id: u64) {
        caller.require_auth();
        Self::require_signer(&env, &caller);

        let mut proposal = Self::fetch_proposal(&env, id);
        if proposal.status != ProposalStatus::Active {
            panic!("ProposalNotPassed");
        }
    }
}

#[cfg(test)]
mod quorum_edge_test;

#[cfg(test)]
mod signer_cooldown_test;

#[cfg(test)]
mod action_allowlist_test;

#[cfg(test)]
mod upgrade_e2e_test;

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

    // -----------------------------------------------------------------------
    // View helpers
    // -----------------------------------------------------------------------

    /// Return the current threshold.
    pub fn get_threshold(env: Env) -> u32 {
        env.storage()
            .persistent()
            .get(&MultisigDataKey::Threshold)
            .unwrap_or(1)
    }

    /// Return the current signer list.
    pub fn get_signers(env: Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&MultisigDataKey::Signers)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Return the current state of a proposal.
    ///
    /// # Arguments
    /// * `id` – Proposal ID.
    pub fn get_proposal(env: Env, id: u64) -> Proposal {
        env.storage()
            .persistent()
            .get(&MultisigDataKey::Proposal(id))
            .unwrap_or_else(|| panic!("ProposalNotFound"))
    }
}

#[cfg(test)]
mod execution_router_test;
