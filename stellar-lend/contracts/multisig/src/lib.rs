#![no_std]
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, symbol_short, xdr::ToXdr,
    Address, Bytes, BytesN, Env, IntoVal, Symbol, Vec,
};

/// Domain separator for multisig approval-authorization payloads (issue #1278).
///
/// Every approval is cryptographically scoped by hashing:
///
/// ```text
/// sha256(DOMAIN_SEPARATOR || contract_id_xdr || proposal_id_be64 ||
/// signer_set_hash || approver_xdr)
/// ```
///
/// The resulting hash is what `approve_proposal` requires the signer to authorize
/// via `require_auth_for_args`, so an authorization gathered for proposal `A`
/// cannot satisfy approval of a different proposal `B`. Bump the `_V1` suffix on
/// any breaking change to the payload layout.
///
/// See `APPROVAL_DOMAIN_BINDING.md` for the full layout and threat model.
pub const APPROVAL_DOMAIN_SEPARATOR: &[u8] = b"STELLARLEND_MULTISIG_APPROVAL_V1";

/// Domain separator for signer-set-bound proposal approvals.
pub const SIGNER_SET_DOMAIN_SEPARATOR: &[u8] = b"STELLARLEND_MULTISIG_SIGNER_SET_V1";

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
    /// Invoke an arbitrary contract entrypoint via cross-contract call
    InvokeContract(Address, Symbol, Vec<soroban_sdk::Val>),
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
#[derive(Clone, Debug, PartialEq)]
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

/// Event emitted when a new proposal is created.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ProposalCreatedEvent {
    pub id: u64,
    pub proposer: Address,
    pub action_kind: Symbol,
    pub expires_at: u64,
}

/// Event emitted when a signer approves a proposal.
/// `passed` is `true` when this approval pushed the proposal to `Passed`.
#[contracttype]
#[derive(Clone, Debug)]
pub struct ProposalApprovedEvent {
    pub id: u64,
    pub approver: Address,
    pub approval_count: u32,
    pub passed: bool,
}

/// Event emitted after a proposal has been executed.
#[contractevent]
#[derive(Clone, Debug)]
pub struct ProposalExecutedEvent {
    pub id: u64,
    pub action_kind: Symbol,
    pub ok: bool,
}

/// Event emitted after a batch of proposals has been atomically executed.
#[contractevent]
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
    /// Monotonic nonce allocated when a proposal is created.
    NextNonce,
    /// Unique execution nonce associated with a proposal.
    ProposalNonce(u64),
    /// Marker written only after the proposal action succeeds.
    ConsumedNonce(u64),
    /// Current signer-set fingerprint.
    SignerSetHash,
    /// Signer-set fingerprint captured when a proposal is created.
    ProposalSignerSetHash(u64),
    /// Domain-separated approval binding for `(proposal_id, approver)`.
    ///
    /// Stores
    /// `sha256(DOMAIN_SEPARATOR || contract_id || proposal_id || signer_set_hash || approver)`
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
    ProposalIdOverflow = 17,
    SignerSetChanged = 18,
    LegacyProposal = 19,
    NonceOverflow = 20,
}

/// Maximum number of proposals that can be executed in a single
/// `batch_execute` call. This bounds loop iterations and storage
/// churn in a single contract invocation.
pub const MAX_BATCH_SIZE: u32 = 32;

/// Emitted when a signer revokes a previous approval from an open proposal.
#[contractevent]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalRevokedEvent {
    pub proposal_id: u64,
    pub signer: Address,
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
    pub fn initialize(
        env: Env,
        signers: Vec<Address>,
        threshold: u32,
    ) -> Result<(), MultisigError> {
        if env.storage().persistent().has(&MultisigDataKey::Signers) {
            return Err(MultisigError::AlreadyInitialized);
        }
        if signers.is_empty() {
            return Err(MultisigError::InvalidSigners);
        }
        if threshold == 0 || threshold as usize > signers.len() as usize {
            return Err(MultisigError::InvalidThreshold);
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
        env.storage()
            .persistent()
            .set(&MultisigDataKey::NextNonce, &0u64);
        let signer_set_hash = Self::signer_set_hash(&env, &signers);
        env.storage()
            .persistent()
            .set(&MultisigDataKey::SignerSetHash, &signer_set_hash);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Internal helpers
    // -----------------------------------------------------------------------

    fn require_signer(env: &Env, caller: &Address) -> Result<(), MultisigError> {
        let signers: Vec<Address> = env
            .storage()
            .persistent()
            .get(&MultisigDataKey::Signers)
            .unwrap_or_else(|| Vec::new(env));
        if !signers.contains(caller) {
            return Err(MultisigError::Unauthorized);
        }
        Ok(())
    }

    fn fetch_threshold(env: &Env) -> u32 {
        env.storage()
            .persistent()
            .get(&MultisigDataKey::Threshold)
            .unwrap_or(1)
    }

    fn fetch_proposal(env: &Env, id: u64) -> Result<Proposal, MultisigError> {
        env.storage()
            .persistent()
            .get(&MultisigDataKey::Proposal(id))
            .ok_or(MultisigError::ProposalNotFound)
    }

    fn fetch_signers(env: &Env) -> Vec<Address> {
        env.storage()
            .persistent()
            .get(&MultisigDataKey::Signers)
            .unwrap_or_else(|| Vec::new(env))
    }

    /// Hashes the signer set in its stored canonical order. The hash is
    /// captured per proposal so approvals cannot survive signer rotation.
    fn signer_set_hash(env: &Env, signers: &Vec<Address>) -> BytesN<32> {
        let mut payload = Bytes::new(env);
        payload.extend_from_slice(SIGNER_SET_DOMAIN_SEPARATOR);
        for signer in signers.iter() {
            payload.append(&signer.to_xdr(env));
        }
        env.crypto().sha256(&payload).into()
    }

    fn current_signer_set_hash(env: &Env) -> BytesN<32> {
        Self::signer_set_hash(env, &Self::fetch_signers(env))
    }

    fn fetch_proposal_signer_set_hash(env: &Env, id: u64) -> Result<BytesN<32>, MultisigError> {
        env.storage()
            .persistent()
            .get(&MultisigDataKey::ProposalSignerSetHash(id))
            .ok_or(MultisigError::LegacyProposal)
    }

    fn fetch_proposal_nonce(env: &Env, id: u64) -> Result<u64, MultisigError> {
        env.storage()
            .persistent()
            .get(&MultisigDataKey::ProposalNonce(id))
            // Existing proposal records predate nonce metadata. Treating the
            // immutable proposal id as a legacy nonce lets already-executed
            // records remain readable without weakening new proposals.
            .or_else(|| {
                env.storage()
                    .persistent()
                    .get::<MultisigDataKey, Proposal>(&MultisigDataKey::Proposal(id))
                    .map(|_| id)
            })
            .ok_or(MultisigError::ProposalNotFound)
    }

    fn allocate_proposal_nonce(env: &Env) -> Result<u64, MultisigError> {
        let nonce: u64 = env
            .storage()
            .persistent()
            .get(&MultisigDataKey::NextNonce)
            .unwrap_or_else(|| {
                // On an older deployment ProposalCount is the only monotonic
                // counter. Starting from it avoids colliding with legacy ids.
                env.storage()
                    .persistent()
                    .get(&MultisigDataKey::ProposalCount)
                    .unwrap_or(0)
            });
        let next = nonce.checked_add(1).ok_or(MultisigError::NonceOverflow)?;
        env.storage()
            .persistent()
            .set(&MultisigDataKey::NextNonce, &next);
        Ok(nonce)
    }

    fn require_current_proposal_signer_set(
        env: &Env,
        id: u64,
    ) -> Result<BytesN<32>, MultisigError> {
        let captured = Self::fetch_proposal_signer_set_hash(env, id)?;
        if captured != Self::current_signer_set_hash(env) {
            return Err(MultisigError::SignerSetChanged);
        }
        Ok(captured)
    }

    fn save_proposal(env: &Env, proposal: &Proposal) {
        env.storage()
            .persistent()
            .set(&MultisigDataKey::Proposal(proposal.id), proposal);
    }

    fn next_proposal_id(env: &Env) -> Result<u64, MultisigError> {
        let count: u64 = env
            .storage()
            .persistent()
            .get(&MultisigDataKey::ProposalCount)
            .unwrap_or(0);
        let new_count = count
            .checked_add(1)
            .ok_or(MultisigError::ProposalIdOverflow)?;
        env.storage()
            .persistent()
            .set(&MultisigDataKey::ProposalCount, &new_count);
        Ok(count)
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
    /// DOMAIN_SEPARATOR || contract_id_xdr || proposal_id (8-byte BE)
    /// || signer_set_hash || approver_xdr
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
    fn approval_auth_payload(
        env: &Env,
        proposal_id: u64,
        approver: &Address,
        signer_set_hash: &BytesN<32>,
    ) -> Bytes {
        let mut payload = Bytes::new(env);
        payload.extend_from_slice(APPROVAL_DOMAIN_SEPARATOR);
        payload.append(&env.current_contract_address().to_xdr(env));
        payload.extend_from_slice(&proposal_id.to_be_bytes());
        payload.append(&signer_set_hash.to_bytes());
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
    fn approval_auth_hash(
        env: &Env,
        proposal_id: u64,
        approver: &Address,
        signer_set_hash: &BytesN<32>,
    ) -> BytesN<32> {
        let payload = Self::approval_auth_payload(env, proposal_id, approver, signer_set_hash);
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
        Self::require_signer(&env, &caller)?;

        if ttl_ledgers > 3_110_400 {
            return Err(MultisigError::InvalidTtl);
        }

        let id = Self::next_proposal_id(&env)?;
        let nonce = Self::allocate_proposal_nonce(&env)?;
        let expires_at = (env.ledger().sequence() as u64).saturating_add(ttl_ledgers);
        let signer_set_hash = Self::current_signer_set_hash(&env);

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
        env.storage()
            .persistent()
            .set(&MultisigDataKey::ProposalNonce(id), &nonce);
        env.storage().persistent().set(
            &MultisigDataKey::ProposalSignerSetHash(id),
            &signer_set_hash,
        );

        let action_kind = Self::action_kind_symbol(&env, &proposal.action);
        env.events().publish(
            (symbol_short!("multisig"), Symbol::new(&env, "created")),
            ProposalCreatedEvent {
                id,
                proposer: proposal.proposer,
                action_kind,
                expires_at,
            },
        );

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
    ///     || signer_set_hash
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
    /// * `caller` – Signer casting the approval.
    /// * `id`     – ID of the proposal to approve.
    pub fn approve_proposal(env: Env, caller: Address, id: u64) -> Result<(), MultisigError> {
        Self::require_signer(&env, &caller)?;

        let mut proposal = Self::fetch_proposal(&env, id)?;
        let signer_set_hash = Self::require_current_proposal_signer_set(&env, id)?;

        // Domain-separated binding (issue #1278): instead of a bare
        // `require_auth()`, require the caller to have authorized this
        // exact `(contract, proposal_id, approver)` hash. See
        // APPROVAL_DOMAIN_BINDING.md for the full threat model.
        let binding = Self::approval_auth_hash(&env, id, &caller, &signer_set_hash);
        caller.require_auth_for_args((binding.clone(),).into_val(&env));

        if proposal.status == ProposalStatus::Expired
            || env.ledger().sequence() as u64 > proposal.expires_at
        {
            proposal.status = ProposalStatus::Expired;
            Self::save_proposal(&env, &proposal);
            return Err(MultisigError::ProposalExpired);
        }
        if proposal.status == ProposalStatus::Executed {
            return Err(MultisigError::AlreadyExecuted);
        }
        if proposal.status == ProposalStatus::Cancelled {
            return Err(MultisigError::AlreadyCancelled);
        }
        if proposal.status != ProposalStatus::Active {
            return Err(MultisigError::ProposalNotPassed);
        }
        if proposal.approvals.contains(&caller) {
            return Err(MultisigError::AlreadyApproved);
        }

        proposal.approvals.push_back(caller.clone());

        // Persist the domain-separated binding for off-chain / indexer checks
        // and for `verify_approval_binding`.
        env.storage().persistent().set(
            &MultisigDataKey::ApprovalBinding(id, caller.clone()),
            &binding,
        );

        let threshold = Self::fetch_threshold(&env) as usize;
        let approval_count = proposal.approvals.len();
        let passed = approval_count as usize >= threshold;
        if passed {
            proposal.status = ProposalStatus::Passed;
        }
        Self::save_proposal(&env, &proposal);

        env.events().publish(
            (symbol_short!("multisig"), Symbol::new(&env, "approved")),
            ProposalApprovedEvent {
                id,
                approver: caller,
                approval_count,
                passed,
            },
        );

        Ok(())
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
    ) -> Result<(), MultisigError> {
        caller.require_auth();
        Self::require_signer(&env, &caller)?;

        let mut proposal = Self::fetch_proposal(&env, id)?;

        // Expiry guard
        if env.ledger().sequence() as u64 > proposal.expires_at {
            proposal.status = ProposalStatus::Expired;
            Self::save_proposal(&env, &proposal);
            return Err(MultisigError::ProposalExpired);
        }
        // Status guards
        if proposal.status == ProposalStatus::Executed {
            return Err(MultisigError::AlreadyExecuted);
        }
        if proposal.status == ProposalStatus::Cancelled {
            return Err(MultisigError::AlreadyCancelled);
        }
        if proposal.status != ProposalStatus::Passed {
            return Err(match proposal.status {
                ProposalStatus::Active => MultisigError::QuorumNotReached,
                _ => MultisigError::ProposalNotPassed,
            });
        }
        let _signer_set_hash = Self::require_current_proposal_signer_set(&env, id)?;
        let nonce = Self::fetch_proposal_nonce(&env, id)?;
        if env
            .storage()
            .persistent()
            .has(&MultisigDataKey::ConsumedNonce(nonce))
        {
            return Err(MultisigError::AlreadyExecuted);
        }
        // Payload-hash binding: prevents action swap between approval and execution
        if proposal.payload_hash != payload_hash {
            return Err(MultisigError::PayloadHashMismatch);
        }

        let action_kind = Self::action_kind_symbol(&env, &proposal.action);
        Self::dispatch_action(&env, &proposal.action)?;

        // The nonce is consumed only after the action succeeds. Soroban
        // transaction rollback therefore leaves it available after a failed
        // cross-contract invocation, while a retry after success is rejected.
        env.storage()
            .persistent()
            .set(&MultisigDataKey::ConsumedNonce(nonce), &true);
        proposal.status = ProposalStatus::Executed;
        Self::save_proposal(&env, &proposal);

        // Emit ProposalExecutedEvent
        ProposalExecutedEvent {
            id,
            action_kind,
            ok: true,
        }
        .publish(&env);
        Ok(())
    }

    /// Internal router: dispatches a `ProposalAction` to its handler.
    fn dispatch_action(env: &Env, action: &ProposalAction) -> Result<(), MultisigError> {
        match action {
            ProposalAction::SetThreshold(new_threshold) => {
                if *new_threshold == 0 {
                    return Err(MultisigError::InvalidThreshold);
                }
                env.storage()
                    .persistent()
                    .set(&MultisigDataKey::Threshold, new_threshold);
                Ok(())
            }
            ProposalAction::RotateSigners(new_signers) => {
                if new_signers.is_empty() {
                    return Err(MultisigError::InvalidSigners);
                }
                // Signer-shrink guard: the new signer set must be at least as
                // large as the current threshold, otherwise quorum could never
                // be reached again and the multisig would be permanently bricked.
                let threshold = Self::fetch_threshold(env);
                if (new_signers.len() as u32) < threshold {
                    return Err(MultisigError::InvalidSigners);
                }
                env.storage()
                    .persistent()
                    .set(&MultisigDataKey::Signers, new_signers);
                let signer_set_hash = Self::signer_set_hash(env, new_signers);
                env.storage()
                    .persistent()
                    .set(&MultisigDataKey::SignerSetHash, &signer_set_hash);
                Ok(())
            }
            ProposalAction::InvokeContract(contract, fn_symbol, args) => {
                // Dispatch to the target contract entrypoint with the concrete
                // arguments carried on the proposal action. The payload hash
                // still binds the approved action so it cannot be swapped.
                let _res: soroban_sdk::Val = env.invoke_contract(contract, fn_symbol, args.clone());
                Ok(())
            }
        }
    }

    /// Cancel an active proposal (proposer or any signer).
    ///
    /// # Arguments
    /// * `caller` – Signer requesting cancellation.
    /// * `id`     – ID of the proposal to cancel.
    pub fn cancel_proposal(env: Env, caller: Address, id: u64) -> Result<(), MultisigError> {
        caller.require_auth();
        Self::require_signer(&env, &caller)?;

        let mut proposal = Self::fetch_proposal(&env, id)?;

        // Ledger-based expiry guard (consistent with execute_proposal
        // and approve_proposal): if the current ledger has passed
        // expires_at the proposal is expired regardless of the stored
        // status field.
        if env.ledger().sequence() as u64 > proposal.expires_at {
            proposal.status = ProposalStatus::Expired;
            Self::save_proposal(&env, &proposal);
            return Err(MultisigError::ProposalExpired);
        }
        if proposal.status == ProposalStatus::Expired {
            return Err(MultisigError::ProposalExpired);
        }
        if proposal.status == ProposalStatus::Executed {
            return Err(MultisigError::AlreadyExecuted);
        }
        if proposal.status == ProposalStatus::Cancelled {
            return Err(MultisigError::AlreadyCancelled);
        }
        if proposal.status != ProposalStatus::Active {
            return Err(MultisigError::ProposalNotPassed);
        }
        proposal.status = ProposalStatus::Cancelled;
        Self::save_proposal(&env, &proposal);
        Ok(())
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

    /// Return the fingerprint of the current signer set used in approval
    /// authorization payloads.
    pub fn get_signer_set_hash(env: Env) -> BytesN<32> {
        Self::current_signer_set_hash(&env)
    }

    /// Return the full state of a proposal.
    ///
    /// # Arguments
    /// * `id` – Proposal ID.
    pub fn get_proposal(env: Env, id: u64) -> Result<Proposal, MultisigError> {
        Self::fetch_proposal(&env, id)
    }

    /// Return the nonce allocated to a proposal.
    pub fn get_proposal_nonce(env: Env, id: u64) -> Result<u64, MultisigError> {
        Self::fetch_proposal_nonce(&env, id)
    }

    /// Return whether an execution nonce has been consumed successfully.
    pub fn is_nonce_consumed(env: Env, nonce: u64) -> bool {
        env.storage()
            .persistent()
            .has(&MultisigDataKey::ConsumedNonce(nonce))
    }

    /// Return the signer-set fingerprint captured for a proposal.
    pub fn get_proposal_signer_set_hash(env: Env, id: u64) -> Option<BytesN<32>> {
        env.storage()
            .persistent()
            .get(&MultisigDataKey::ProposalSignerSetHash(id))
    }

    /// Add replay-protection metadata to a proposal created before this
    /// metadata was introduced. Migration is intentionally conservative:
    /// the proposer and every recorded approver must still be current
    /// signers. If that cannot be proven, the legacy proposal stays blocked.
    pub fn migrate_proposal_security(
        env: Env,
        caller: Address,
        id: u64,
    ) -> Result<(), MultisigError> {
        caller.require_auth();
        Self::require_signer(&env, &caller)?;
        let proposal = Self::fetch_proposal(&env, id)?;
        if proposal.status == ProposalStatus::Executed {
            return Err(MultisigError::AlreadyExecuted);
        }
        if proposal.status == ProposalStatus::Cancelled {
            return Err(MultisigError::AlreadyCancelled);
        }
        if proposal.status != ProposalStatus::Active && proposal.status != ProposalStatus::Passed {
            return Err(MultisigError::ProposalNotPassed);
        }

        let signers = Self::fetch_signers(&env);
        if !signers.contains(&proposal.proposer)
            || proposal
                .approvals
                .iter()
                .any(|approver| !signers.contains(&approver))
        {
            return Err(MultisigError::SignerSetChanged);
        }

        if env
            .storage()
            .persistent()
            .get::<MultisigDataKey, BytesN<32>>(&MultisigDataKey::ProposalSignerSetHash(id))
            .is_some()
        {
            return Ok(());
        }

        let nonce = Self::fetch_proposal_nonce(&env, id)?;
        let signer_set_hash = Self::current_signer_set_hash(&env);
        env.storage()
            .persistent()
            .set(&MultisigDataKey::ProposalNonce(id), &nonce);
        env.storage().persistent().set(
            &MultisigDataKey::ProposalSignerSetHash(id),
            &signer_set_hash,
        );
        Ok(())
    }

    /// Returns the stored domain-separated approval binding hash for
    /// `(id, approver)`, if an approval was recorded.
    ///
    /// The hash is
    /// `sha256(APPROVAL_DOMAIN_SEPARATOR || contract_id || id || signer_set_hash || approver)`.
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
    /// `approval_auth_hash(id, approver, captured_signer_set_hash)`.
    ///
    /// See issue #1278 and `APPROVAL_DOMAIN_BINDING.md`.
    pub fn verify_approval_binding(env: Env, id: u64, approver: Address) -> bool {
        let signer_set_hash = match Self::fetch_proposal_signer_set_hash(&env, id) {
            Ok(hash) => hash,
            Err(_) => return false,
        };
        match Self::get_approval_binding(env.clone(), id, approver.clone()) {
            Some(stored) => {
                stored == Self::approval_auth_hash(&env, id, &approver, &signer_set_hash)
            }
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
    /// `sha256(APPROVAL_DOMAIN_SEPARATOR || contract_id || id || signer_set_hash || approver)`.
    ///
    /// See issue #1278 and `APPROVAL_DOMAIN_BINDING.md`.
    pub fn approval_binding_hash(env: Env, id: u64, approver: Address) -> BytesN<32> {
        let signer_set_hash = Self::fetch_proposal_signer_set_hash(&env, id)
            .unwrap_or_else(|_| Self::current_signer_set_hash(&env));
        Self::approval_auth_hash(&env, id, &approver, &signer_set_hash)
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
    ) -> Result<(), MultisigError> {
        caller.require_auth();
        Self::require_signer(&env, &caller)?;

        let batch_size = ids.len();
        if batch_size > MAX_BATCH_SIZE {
            return Err(MultisigError::BatchSizeExceeded);
        }
        if batch_size != payload_hashes.len() {
            return Err(MultisigError::PayloadHashMismatch);
        }

        // Phase 1 – validate every proposal before touching any state
        let mut proposals: Vec<Proposal> = Vec::new(&env);
        for i in 0..batch_size {
            let id = ids.get(i).unwrap();
            let payload_hash = payload_hashes.get(i).unwrap();

            // Duplicate check against earlier positions
            for j in 0..i {
                if ids.get(j).unwrap() == id {
                    return Err(MultisigError::DuplicateProposalId);
                }
            }

            let mut proposal = Self::fetch_proposal(&env, id)?;

            // Expiry guard
            if env.ledger().sequence() as u64 > proposal.expires_at {
                proposal.status = ProposalStatus::Expired;
                Self::save_proposal(&env, &proposal);
                return Err(MultisigError::ProposalExpired);
            }
            // Status guards
            if proposal.status == ProposalStatus::Executed {
                return Err(MultisigError::AlreadyExecuted);
            }
            if proposal.status == ProposalStatus::Cancelled {
                return Err(MultisigError::AlreadyCancelled);
            }
            if proposal.status != ProposalStatus::Passed {
                return Err(match proposal.status {
                    ProposalStatus::Active => MultisigError::QuorumNotReached,
                    _ => MultisigError::ProposalNotPassed,
                });
            }
            Self::require_current_proposal_signer_set(&env, id)?;
            let nonce = Self::fetch_proposal_nonce(&env, id)?;
            if env
                .storage()
                .persistent()
                .has(&MultisigDataKey::ConsumedNonce(nonce))
            {
                return Err(MultisigError::AlreadyExecuted);
            }
            // Payload-hash binding
            if proposal.payload_hash != payload_hash {
                return Err(MultisigError::PayloadHashMismatch);
            }

            proposals.push_back(proposal);
        }

        // Phase 2 – execute each proposal in order; if any dispatch fails
        // the panic rolls back all prior execution side-effects.
        for i in 0..batch_size {
            let mut proposal = proposals.get(i).unwrap();
            let nonce = Self::fetch_proposal_nonce(&env, proposal.id)?;
            Self::dispatch_action(&env, &proposal.action)?;
            env.storage()
                .persistent()
                .set(&MultisigDataKey::ConsumedNonce(nonce), &true);
            proposal.status = ProposalStatus::Executed;
            Self::save_proposal(&env, &proposal);
        }

        // Emit single BatchExecuted event with all applied IDs
        BatchExecutedEvent { ids }.publish(&env);
        Ok(())
    }
}

// Pre-existing test modules from an older API version – commented out
// until updated to match the current contract interface.
// #[cfg(test)]
// mod quorum_edge_test;
// #[cfg(test)]
// mod action_allowlist_test;
// #[cfg(test)]
// mod upgrade_e2e_test;

// revoke_approval_test is from an older API version: it imports
// MIN_THRESHOLD_DELAY_LEDGERS, calls set_signers / revoke_approval /
// get_proposal_approvals, and expects error variants (InsufficientApprovals,
// ApprovalNotFound, ProposalAlreadyExecuted) that do not exist in the current
// contract. Commented out until rewritten against the proposal-based API.
// #[cfg(test)]
// mod revoke_approval_test;

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

#[cfg(test)]
mod signer_shrink_guard_test;

#[cfg(test)]
mod replay_protection_test;
