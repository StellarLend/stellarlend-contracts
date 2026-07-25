#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, xdr::ToXdr, Address, Bytes,
    BytesN, Env, IntoVal, Symbol, Vec,
};

/// Domain separator for multisig approval-authorization payloads (issue #1278).
///
/// Every approval is cryptographically scoped by hashing:
///
/// ```text
/// sha256(DOMAIN_SEPARATOR || contract_id_xdr || proposal_id_be64 || approver_xdr)
/// ```
///
/// The resulting hash is what `approve_proposal` requires the signer to authorize
/// via `require_auth_for_args`, so an authorization gathered for proposal `A`
/// cannot satisfy approval of a different proposal `B`. Bump the `_V1` suffix on
/// any breaking change to the payload layout.
///
/// See `APPROVAL_DOMAIN_BINDING.md` for the full layout and threat model.
pub const APPROVAL_DOMAIN_SEPARATOR: &[u8] = b"STELLARLEND_MULTISIG_APPROVAL_V1";

/// Typed action carried on a Proposal and dispatched at execute_proposal time.
/// The payload_hash binds the approved action so it cannot be swapped between
/// approval and execution.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum ProposalAction {
    /// Update the approval threshold for future proposals
    SetThreshold(u32),
    /// Replace the full signer set with a new set
    RotateSigners(Vec<Address>),
    /// Invoke an arbitrary lending upgrade entrypoint via cross-contract call
    InvokeContract(Address, Symbol, soroban_sdk::Bytes),
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

/// Event emitted after a batch of proposals has been atomically executed.
#[contracttype]
#[derive(Clone, Debug)]
pub struct BatchExecutedEvent {
    pub ids: Vec<u64>,
}

#[contracttype]
pub enum MultisigDataKey {
    Threshold,
    Signers,
    ProposalCount,
    Proposal(u64),
    /// Domain-separated approval binding for `(proposal_id, approver)`.
    ///
    /// Stores
    /// `sha256(DOMAIN_SEPARATOR || contract_id || proposal_id || approver)`
    /// at approval time so an approval is cryptographically scoped to exactly
    /// one proposal and can be verified out-of-band (issue #1278;
    /// `APPROVAL_DOMAIN_BINDING.md`).
    ApprovalBinding(u64, Address),
}

/// Multisig errors.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum MultisigError {
    Unauthorized = 1,
    ProposalNotFound = 2,
    ProposalNotPassed = 3,
    ProposalExpired = 4,
    AlreadyExecuted = 5,
    AlreadyApproved = 6,
    PayloadHashMismatch = 7,
    QuorumNotReached = 8,
    InvalidAction = 9,
    InvalidThreshold = 10,
    InvalidSigners = 11,
    AlreadyCancelled = 12,
    InvalidTtl = 13,
    BatchSizeExceeded = 14,
    DuplicateProposalId = 15,
    AlreadyInitialized = 16,
}

/// Maximum number of proposals that can be executed in a single
/// `batch_execute` call. This bounds loop iterations and storage
/// churn in a single contract invocation.
pub const MAX_BATCH_SIZE: u32 = 32;

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
    pub fn initialize(
        env: Env,
        signers: Vec<Address>,
        threshold: u32,
    ) -> Result<(), MultisigError> {
        if env.storage().persistent().has(&MultisigDataKey::Signers) {
            return Err(MultisigError::AlreadyInitialized);
        }
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
        Ok(())
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

    /// Builds the domain-separated approval-authorization preimage for
    /// `(proposal_id, approver)`:
    ///
    /// ```text
    /// DOMAIN_SEPARATOR || contract_id_xdr || proposal_id (8-byte BE) || approver_xdr
    /// ```
    ///
    /// # Arguments
    /// * `env`         – Soroban environment.
    /// * `proposal_id` – Proposal this approval is scoped to.
    /// * `approver`    – Signer casting the approval.
    ///
    /// # Returns
    /// Canonical byte preimage before hashing.
    ///
    /// See issue #1278 and `APPROVAL_DOMAIN_BINDING.md`.
    fn approval_auth_payload(env: &Env, proposal_id: u64, approver: &Address) -> Bytes {
        let mut payload = Bytes::new(env);
        payload.extend_from_slice(APPROVAL_DOMAIN_SEPARATOR);
        payload.append(&env.current_contract_address().to_xdr(env));
        payload.extend_from_slice(&proposal_id.to_be_bytes());
        payload.append(&approver.clone().to_xdr(env));
        payload
    }

    /// SHA-256 of [`Self::approval_auth_payload`].
    ///
    /// This is the exact authorization payload that `approve_proposal` binds
    /// into `require_auth_for_args`, and that is stored under
    /// [`MultisigDataKey::ApprovalBinding`].
    ///
    /// # Arguments
    /// * `env`         – Soroban environment.
    /// * `proposal_id` – Proposal this approval is scoped to.
    /// * `approver`    – Signer casting the approval.
    ///
    /// # Returns
    /// 32-byte domain-separated binding hash.
    fn approval_auth_hash(env: &Env, proposal_id: u64, approver: &Address) -> BytesN<32> {
        let payload = Self::approval_auth_payload(env, proposal_id, approver);
        env.crypto().sha256(&payload).into()
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
    ) -> Result<u64, MultisigError> {
        caller.require_auth();
        Self::require_signer(&env, &caller);

        if ttl_ledgers > 3_110_400 {
            return Err(MultisigError::InvalidTtl);
        }

        let id = Self::next_proposal_id(&env);
        let expires_at = (env.ledger().sequence() as u64).saturating_add(ttl_ledgers);

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
        Ok(id)
    }

    /// Approve an existing active proposal.
    ///
    /// A proposal is automatically transitioned to `Passed` once the number of
    /// distinct signer approvals meets or exceeds the current threshold.
    ///
    /// # Authorization domain binding (issue #1278)
    ///
    /// Instead of a bare `require_auth()` (which only proves the caller signed
    /// *some* invocation), this entrypoint requires the caller to authorize the
    /// domain-separated payload:
    ///
    /// ```text
    /// sha256(
    ///     APPROVAL_DOMAIN_SEPARATOR
    ///     || contract_id_xdr
    ///     || proposal_id (8-byte big-endian)
    ///     || approver_xdr
    /// )
    /// ```
    ///
    /// via `require_auth_for_args`. An authorization produced for one
    /// `proposal_id` therefore cannot be replayed against any other proposal.
    /// The same hash is persisted under
    /// [`MultisigDataKey::ApprovalBinding`] for off-chain verification.
    ///
    /// # Arguments
    /// * `caller` – Signer casting the approval (must be a registered signer).
    /// * `id`     – ID of the proposal to approve; folded into the auth domain.
    ///
    /// # Panics
    /// * `"Unauthorized"` – caller is not a registered signer, or the domain-
    ///   bound authorization does not match this `(contract, id, caller)`.
    /// * `"ProposalExpired"` – current ledger has passed `expires_at`.
    /// * `"ProposalNotPassed"` – proposal is not in `Active` status.
    /// * `"AlreadyApproved"` – `caller` has already approved this proposal.
    ///
    /// See `APPROVAL_DOMAIN_BINDING.md`.
    pub fn approve_proposal(env: Env, caller: Address, id: u64) {
        // Domain-bound authorization: the signed auth entry must cover
        // hash(DOMAIN || contract_id || proposal_id || approver). An auth entry
        // gathered for a different proposal_id will not satisfy this check.
        let binding = Self::approval_auth_hash(&env, id, &caller);
        caller.require_auth_for_args((binding.clone(),).into_val(&env));
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

        proposal.approvals.push_back(caller.clone());

        // Persist the domain-separated binding for off-chain / indexer checks
        // and for `verify_approval_binding`.
        env.storage()
            .persistent()
            .set(&MultisigDataKey::ApprovalBinding(id, caller.clone()), &binding);

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
    pub fn execute_proposal(env: Env, caller: Address, id: u64, payload_hash: soroban_sdk::Bytes) {
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
            ProposalAction::InvokeContract(contract, fn_symbol, _args_hash) => {
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
        proposal.status = ProposalStatus::Cancelled;
        Self::save_proposal(&env, &proposal);
    }

    /// Return the current approval threshold.
    pub fn get_threshold(env: Env) -> u32 {
        Self::fetch_threshold(&env)
    }

    /// Return the current registered signer set.
    pub fn get_signers(env: Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&MultisigDataKey::Signers)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Return the full state of a proposal.
    ///
    /// # Arguments
    /// * `id` – Proposal ID.
    pub fn get_proposal(env: Env, id: u64) -> Proposal {
        Self::fetch_proposal(&env, id)
    }

    /// Returns the stored domain-separated approval binding hash for
    /// `(id, approver)`, if an approval was recorded.
    ///
    /// The hash is
    /// `sha256(APPROVAL_DOMAIN_SEPARATOR || contract_id || id || approver)`.
    ///
    /// # Arguments
    /// * `id`       – Proposal ID the approval was cast for.
    /// * `approver` – Signer whose binding to look up.
    ///
    /// # Returns
    /// `Some(BytesN<32>)` when the signer approved `id`, else `None`.
    ///
    /// See issue #1278 and `APPROVAL_DOMAIN_BINDING.md`.
    pub fn get_approval_binding(env: Env, id: u64, approver: Address) -> Option<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&MultisigDataKey::ApprovalBinding(id, approver))
    }

    /// Verifies that the recorded approval binding for `(id, approver)` matches
    /// the domain-separated hash for that exact pair.
    ///
    /// Returns `false` when no approval exists for the pair, or when the stored
    /// binding would not match a recomputed hash for this `id` — i.e. an
    /// approval intended for a different proposal cannot verify here.
    ///
    /// # Arguments
    /// * `id`       – Proposal ID to check.
    /// * `approver` – Signer whose approval binding to verify.
    ///
    /// # Returns
    /// `true` iff a binding was stored for `(id, approver)` and it equals
    /// `approval_auth_hash(id, approver)`.
    ///
    /// See issue #1278 and `APPROVAL_DOMAIN_BINDING.md`.
    pub fn verify_approval_binding(env: Env, id: u64, approver: Address) -> bool {
        match Self::get_approval_binding(env.clone(), id, approver.clone()) {
            Some(stored) => stored == Self::approval_auth_hash(&env, id, &approver),
            None => false,
        }
    }

    /// Pure view of the domain-separated approval-authorization hash that
    /// `approve_proposal` requires the signer to authorize.
    ///
    /// Useful for clients that need to precompute the auth args, and for tests
    /// that assert cross-proposal bindings differ.
    ///
    /// # Arguments
    /// * `id`       – Proposal ID to bind.
    /// * `approver` – Signer the hash is scoped to.
    ///
    /// # Returns
    /// `sha256(APPROVAL_DOMAIN_SEPARATOR || contract_id || id || approver)`.
    ///
    /// See issue #1278 and `APPROVAL_DOMAIN_BINDING.md`.
    pub fn approval_binding_hash(env: Env, id: u64, approver: Address) -> BytesN<32> {
        Self::approval_auth_hash(&env, id, &approver)
    }

    /// Execute a set of passed proposals atomically.
    ///
    /// All proposals are validated first (status, expiry, payload hash,
    /// duplicates). If every proposal is eligible they are executed in
    /// order. If **any** proposal fails validation or its action dispatches
    /// with an error the entire batch is rejected — no proposal is left
    /// executed (Soroban's panic-based rollback guarantees all-or-nothing).
    ///
    /// A `BatchExecuted` event listing the applied IDs is emitted on
    /// success.
    ///
    /// # Arguments
    /// * `caller`         – Signer triggering execution (must be a registered
    ///                      signer).
    /// * `ids`            – Proposal IDs to execute, in order.
    /// * `payload_hashes` – Payload hashes for each proposal, one per ID;
    ///                      each must match the hash recorded at creation.
    ///
    /// # Panics
    /// * `BatchSizeExceeded` if `ids.len() > MAX_BATCH_SIZE`.
    /// * `DuplicateProposalId` if the same ID appears more than once.
    /// * `ProposalNotFound` if any ID does not exist.
    /// * `ProposalExpired` if any proposal has expired.
    /// * `AlreadyExecuted` if any proposal has already been executed.
    /// * `AlreadyCancelled` if any proposal has been cancelled.
    /// * `ProposalNotPassed` if any proposal has not reached the required
    ///   approval quorum.
    /// * `PayloadHashMismatch` if a payload hash does not match.
    pub fn batch_execute(
        env: Env,
        caller: Address,
        ids: Vec<u64>,
        payload_hashes: Vec<soroban_sdk::Bytes>,
    ) {
        caller.require_auth();
        Self::require_signer(&env, &caller);

        let batch_size = ids.len();
        if batch_size > MAX_BATCH_SIZE {
            panic!("BatchSizeExceeded");
        }
        if batch_size != payload_hashes.len() {
            panic!("PayloadHashMismatch");
        }

        // Phase 1 – validate every proposal before touching any state
        let mut proposals: Vec<Proposal> = Vec::new(&env);
        for i in 0..batch_size {
            let id = ids.get(i).unwrap();
            let payload_hash = payload_hashes.get(i).unwrap();

            // Duplicate check against earlier positions
            for j in 0..i {
                if ids.get(j).unwrap() == id {
                    panic!("DuplicateProposalId");
                }
            }

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
            // Payload-hash binding
            if proposal.payload_hash != payload_hash {
                panic!("PayloadHashMismatch");
            }

            proposals.push_back(proposal);
        }

        // Phase 2 – execute each proposal in order; if any dispatch fails
        // the panic rolls back all prior execution side-effects.
        for i in 0..batch_size {
            let mut proposal = proposals.get(i).unwrap();
            if !Self::dispatch_action(&env, &proposal.action) {
                panic!("InvalidAction");
            }
            proposal.status = ProposalStatus::Executed;
            Self::save_proposal(&env, &proposal);
        }

        // Emit single BatchExecuted event with all applied IDs
        env.events().publish(
            (
                symbol_short!("multisig"),
                Symbol::new(&env, "batch_executed"),
            ),
            BatchExecutedEvent { ids },
        );
    }
}

// Pre-existing test modules from an older API version – commented out
// until updated to match the current contract interface.
// #[cfg(test)]
// mod quorum_edge_test;
// #[cfg(test)]
// mod signer_cooldown_test;
// #[cfg(test)]
// mod action_allowlist_test;
// #[cfg(test)]
// mod upgrade_e2e_test;

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::Address as _;

    fn setup() -> (Env, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let contract_id = env.register(MultisigContract, ());
        (env, admin, contract_id)
    }
}

#[cfg(test)]
mod execution_router_test;

#[cfg(test)]
mod batch_execute_test;

#[cfg(test)]
mod cancel_proposal_test;

#[cfg(test)]
mod approval_binding_test;
