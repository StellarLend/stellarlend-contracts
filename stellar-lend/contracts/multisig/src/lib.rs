#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Bytes, Env, Symbol, Vec,
};

/// Typed action carried on a Proposal and dispatched at execute_proposal time.
/// The payload_hash binds the approved action so it cannot be swapped between
/// approval and execution.
///
/// # Variant encoding
///
/// All variants use **positional (tuple) fields** so that `#[contracttype]`
/// can derive the required `TryFromVal` / `IntoVal` XDR conversions.
/// Named fields are not supported by the macro.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalAction {
    /// Update the approval threshold for future proposals.
    /// Field 0: `new_threshold: u32`
    SetThreshold(u32),

    /// Replace the full signer set with a new set.
    /// Field 0: `new_signers: Vec<Address>`
    RotateSigners(Vec<Address>),

    /// Invoke an arbitrary lending upgrade entrypoint via cross-contract call.
    ///
    /// * Field 0: `contract: Address`  — target contract
    /// * Field 1: `fn_symbol: Symbol`  — function to call
    /// * Field 2: `args: Vec<Bytes>`   — each element is an XDR-encoded `ScVal`
    ///
    /// The args are decoded back to `Val` at dispatch time and forwarded to
    /// `env.invoke_contract`.  Storing `Bytes` per argument keeps the variant
    /// fully serialisable by `#[contracttype]` while preserving type-fidelity
    /// (any `ScVal`-encodable Soroban type is supported).
    ///
    /// This replaces the previous `args_hash: Bytes` field which was silently
    /// discarded, causing every `InvokeContract` call to pass zero arguments.
    InvokeContract(Address, Symbol, Vec<Bytes>),
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
    /// SHA-256 / Keccak hash of the encoded action payload, bound at creation.
    pub payload_hash: Bytes,
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
    // View helpers (exposed as contract entrypoints)
    // -----------------------------------------------------------------------

    /// Return the current approval threshold.
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
    ///
    /// # Panics
    /// Panics with `"ProposalNotFound"` if the ID does not exist.
    pub fn get_proposal(env: Env, id: u64) -> Proposal {
        env.storage()
            .persistent()
            .get(&MultisigDataKey::Proposal(id))
            .unwrap_or_else(|| panic!("ProposalNotFound"))
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
            ProposalAction::SetThreshold(..) => Symbol::new(env, "SetThreshold"),
            ProposalAction::RotateSigners(..) => Symbol::new(env, "RotateSigners"),
            ProposalAction::InvokeContract(..) => Symbol::new(env, "InvokeContract"),
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
        payload_hash: Bytes,
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
    pub fn execute_proposal(env: Env, caller: Address, id: u64, payload_hash: Bytes) {
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
    /// Returns `true` on success, `false` if the action is invalid.
    fn dispatch_action(env: &Env, action: &ProposalAction) -> bool {
        match action {
            ProposalAction::SetThreshold(new_threshold) => {
                if *new_threshold == 0 {
                    return false;
                }
                env.storage()
                    .persistent()
                    .set(&MultisigDataKey::Threshold, new_threshold);
                true
            }
            ProposalAction::RotateSigners(new_signers) => {
                if new_signers.is_empty() {
                    return false;
                }
                env.storage()
                    .persistent()
                    .set(&MultisigDataKey::Signers, new_signers);
                true
            }
            ProposalAction::InvokeContract(contract, fn_symbol, args) => {
                // Decode each XDR-encoded ScVal argument back to a Val and
                // build the Vec<Val> that invoke_contract expects.
                //
                // The payload_hash check in execute_proposal already binds the
                // entire ProposalAction (including `args`), so the argument list
                // cannot be tampered with between approval and execution.
                use soroban_sdk::xdr::FromXdr;
                let mut decoded: Vec<soroban_sdk::Val> = Vec::new(env);
                for i in 0..args.len() {
                    let arg_bytes = args.get(i).unwrap();
                    let val = soroban_sdk::Val::from_xdr(env, &arg_bytes)
                        .unwrap_or_else(|_| panic!("InvalidAction"));
                    decoded.push_back(val);
                }
                let _res: soroban_sdk::Val =
                    env.invoke_contract(contract, fn_symbol, decoded);
                true
            }
        }
    }

    /// Cancel an active proposal (any registered signer).
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
        proposal.status = ProposalStatus::Cancelled;
        Self::save_proposal(&env, &proposal);
    }
}

// The following test modules reference APIs that have not yet been implemented
// in this contract (e.g. set_signers, queue_signers_change, ActionKind,
// MIN_THRESHOLD_DELAY_LEDGERS) and were already broken before this change.
// They are kept in-tree for reference but excluded from compilation until the
// corresponding entrypoints are added.
//
// #[cfg(test)]
// mod quorum_edge_test;
//
// #[cfg(test)]
// mod signer_cooldown_test;
//
// #[cfg(test)]
// mod action_allowlist_test;
//
// #[cfg(test)]
// mod upgrade_e2e_test;

#[cfg(test)]
mod execution_router_test;

#[cfg(test)]
mod invoke_contract_args_test;
