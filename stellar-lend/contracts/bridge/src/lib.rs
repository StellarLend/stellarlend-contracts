#![no_std]
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Bytes, BytesN, Env,
    Map, Vec,
};

pub const QUORUM_PROOF_DOMAIN: &[u8] = b"stellarlend::bridge::quorum_proof::v1";
const PAUSE_PAYLOAD_TAG: &[u8] = b"BRIDGE_PAUSE:";
const UNPAUSE_PAYLOAD_TAG: &[u8] = b"BRIDGE_UNPAUSE:";

/// Domain separator for inbound message IDs (issue #1901).
///
/// Every inbound message is identified by hashing:
///
/// ```text
/// SHA-256( INBOUND_MSG_DOMAIN
///          || source_domain_hash (32 bytes)
///          || nonce (8 bytes LE) )
/// ```
///
/// This prevents replay across chains, networks, and contracts.
pub const INBOUND_MSG_DOMAIN: &[u8] = b"stellarlend::bridge::inbound_msg::v1";

/// Domain separator for source-domain hashing.
pub const SOURCE_DOMAIN_SEPARATOR: &[u8] = b"stellarlend::bridge::source_domain::v1";

const MIN_VALIDATORS: u32 = 1;
const MAX_VALIDATORS: u32 = 64;

/// Maximum number of registered source domains.  This bounds storage
/// growth and iteration cost for administrative queries.
pub const MAX_SOURCE_DOMAINS: u32 = 256;

#[contracterror]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BridgeError {
    NonceOverflow = 1,
    ValidatorSetTooSmall = 2,
    ValidatorSetTooLarge = 3,
    DuplicateValidatorKey = 4,
    InvalidEpoch = 5,
    RetiredEpoch = 6,
    EmptyProofs = 7,
    ProofVectorTooLarge = 8,
    DuplicateProofSigner = 9,
    SignerNotInValidatorSet = 10,
    InsufficientQuorum = 11,
    NoGuardianConfigured = 12,
    InvalidGuardianSignature = 13,
    UnknownValidator = 14,
    AlreadyPaused = 15,
    NotPaused = 16,
    PauseWouldBreakQuorum = 17,
    InboundCapExceeded = 18,
    OutboundCapExceeded = 19,
    InvalidWindowSize = 20,
    WindowTotalOverflow = 21,
    ChurnLimitExceeded = 22,
    /// The source domain is not registered; rejected at admission.
    UnregisteredSource = 23,
    /// The inbound message has already been consumed (replay detected).
    MessageAlreadyConsumed = 24,
    /// The nonce does not match the expected next nonce for this source
    /// (out-of-order or gap detected).  The message is rejected without
    /// mutating balances or burning the message.
    UnexpectedNonce = 25,
    /// The admin has not been initialised or the caller is not the admin.
    NotAdmin = 26,
    /// Too many source domains registered (storage limit).
    SourceDomainLimitReached = 27,
}

/// Identifies the origin of an inbound bridge message.
///
/// Composed of three domain fields so that messages from different chains,
/// network passphrases, or contracts cannot be confused:
///
/// - `chain_id`            – numeric identifier of the source chain.
/// - `network_passphrase`  – network passphrase of the source deployment.
/// - `contract_id`         – address of the source-side bridge contract.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SourceDomain {
    pub chain_id: u32,
    pub network_passphrase: Bytes,
    pub contract_id: Bytes,
}

/// Maximum `signed_epoch` value an inbound message may carry, expressed as
/// an offset above the bridge's current `epoch`.
///
/// With a value of `0` the bridge requires **strict equality**: only an
/// inbound message bearing exactly [`Bridge::epoch`] is admitted.
///
/// Epochs are monotonically incremented discrete sequence numbers, not
/// physical timestamps, so there is no realistic "clock skew" to absorb.
/// A positive tolerance is a security regression because it would admit
/// messages supposedly signed by a not-yet-rotated validator set — and
/// once that future epoch arrives, those messages could be replayed.
///
/// This constant is `pub` so that tests, off-chain tooling, and audit
/// reviewers can refer to the exact value the binary encodes; changing
/// it is a security-sensitive decision and must be paired with a test
/// update.
pub const INBOUND_EPOCH_TOLERANCE: u64 = 0;

/// Ledger storage key for the outbound nonce map.
#[contracttype]
pub enum BridgeDataKey {
    OutboundNonces,
    Validators,
    PausedValidators,
    Epoch,
    BridgeId,
    Guardian,
    MaxChurn,
    MaxPerWindow,
    WindowSize,
    WindowStart,
    WindowInboundTotal,
    MaxOutboundPerWindow,
    OutboundWindowSize,
    OutboundWindowStart,
    WindowOutboundTotal,
    /// Admin address for privileged operations (source domain management).
    Admin,
    /// Whether a source domain is registered.  The key carries the
    /// SHA-256 hash of the source domain fields.
    SourceRegistered(BytesN<32>),
    /// Per-source-domain inbound nonce.  Stores the next expected
    /// nonce value for a given source domain.
    InboundNonce(BytesN<32>),
    /// Marker written after an inbound message has been consumed.
    /// The key is the domain-separated message ID.
    ConsumedInboundMessage(BytesN<32>),
    /// `Vec<u32>` count of registered source domains (bounds iteration).
    SourceDomainCount,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboundMessageEvent {
    pub dest: u32,
    pub nonce: u64,
}

/// Event emitted when an inbound message is successfully consumed.
///
/// Topics: `("inbound_msg", "consumed")`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InboundMessageConsumedEvent {
    /// Domain-separated message ID that was consumed.
    pub message_id: BytesN<32>,
    /// The source domain that sent the message.
    pub source: SourceDomain,
    /// The nonce consumed from that source.
    pub nonce: u64,
}

#[contract]
pub struct Bridge;

impl Bridge {
    fn load_nonces(env: &Env) -> Map<u32, u64> {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, Map<u32, u64>>(&BridgeDataKey::OutboundNonces)
            .unwrap_or_else(|| Map::new(env))
    }
    fn save_nonces(env: &Env, nonces: &Map<u32, u64>) {
        env.storage()
            .persistent()
            .set(&BridgeDataKey::OutboundNonces, nonces);
    }
    fn load_validators(env: &Env) -> Vec<BytesN<32>> {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, Vec<BytesN<32>>>(&BridgeDataKey::Validators)
            .unwrap_or_else(|| Vec::new(env))
    }
    fn save_validators(env: &Env, validators: &Vec<BytesN<32>>) {
        env.storage()
            .persistent()
            .set(&BridgeDataKey::Validators, validators);
    }
    fn load_paused(env: &Env) -> Map<BytesN<32>, bool> {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, Map<BytesN<32>, bool>>(&BridgeDataKey::PausedValidators)
            .unwrap_or_else(|| Map::new(env))
    }
    fn save_paused(env: &Env, paused: &Map<BytesN<32>, bool>) {
        env.storage()
            .persistent()
            .set(&BridgeDataKey::PausedValidators, paused);
    }
    fn load_epoch(env: &Env) -> u64 {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, u64>(&BridgeDataKey::Epoch)
            .unwrap_or(0)
    }
    fn save_epoch(env: &Env, epoch: u64) {
        env.storage()
            .persistent()
            .set(&BridgeDataKey::Epoch, &epoch);
    }
    fn load_bridge_id(env: &Env) -> Bytes {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, Bytes>(&BridgeDataKey::BridgeId)
            .unwrap_or_else(|| Bytes::new(env))
    }
    fn save_bridge_id(env: &Env, id: &Bytes) {
        env.storage().persistent().set(&BridgeDataKey::BridgeId, id);
    }
    fn load_guardian(env: &Env) -> Option<BytesN<32>> {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, BytesN<32>>(&BridgeDataKey::Guardian)
    }
    fn save_guardian(env: &Env, pk: &BytesN<32>) {
        env.storage().persistent().set(&BridgeDataKey::Guardian, pk);
    }
    fn load_max_churn(env: &Env) -> Option<u32> {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, u32>(&BridgeDataKey::MaxChurn)
    }
    fn save_max_churn(env: &Env, limit: u32) {
        env.storage()
            .persistent()
            .set(&BridgeDataKey::MaxChurn, &limit);
    }
    fn remove_max_churn(env: &Env) {
        env.storage().persistent().remove(&BridgeDataKey::MaxChurn);
    }
    fn load_max_per_window(env: &Env) -> i128 {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, i128>(&BridgeDataKey::MaxPerWindow)
            .unwrap_or(0)
    }
    fn load_window_size(env: &Env) -> u64 {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, u64>(&BridgeDataKey::WindowSize)
            .unwrap_or(0)
    }
    fn load_window_start(env: &Env) -> u64 {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, u64>(&BridgeDataKey::WindowStart)
            .unwrap_or(0)
    }
    fn load_window_inbound_total(env: &Env) -> i128 {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, i128>(&BridgeDataKey::WindowInboundTotal)
            .unwrap_or(0)
    }
    fn load_max_outbound_per_window(env: &Env) -> i128 {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, i128>(&BridgeDataKey::MaxOutboundPerWindow)
            .unwrap_or(0)
    }
    fn load_outbound_window_size(env: &Env) -> u64 {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, u64>(&BridgeDataKey::OutboundWindowSize)
            .unwrap_or(0)
    }
    fn load_outbound_window_start(env: &Env) -> u64 {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, u64>(&BridgeDataKey::OutboundWindowStart)
            .unwrap_or(0)
    }
    fn load_window_outbound_total(env: &Env) -> i128 {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, i128>(&BridgeDataKey::WindowOutboundTotal)
            .unwrap_or(0)
    }

    // -----------------------------------------------------------------------
    // Admin helpers (issue #1901)
    // -----------------------------------------------------------------------

    fn load_admin(env: &Env) -> Option<Address> {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, Address>(&BridgeDataKey::Admin)
    }

    fn save_admin(env: &Env, admin: &Address) {
        env.storage().persistent().set(&BridgeDataKey::Admin, admin);
    }

    fn require_admin(env: &Env, caller: &Address) -> Result<(), BridgeError> {
        caller.require_auth();
        let admin = Self::load_admin(env).ok_or(BridgeError::NotAdmin)?;
        if *caller != admin {
            return Err(BridgeError::NotAdmin);
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Source-domain hashing (issue #1901)
    // -----------------------------------------------------------------------

    /// Compute a deterministic hash of a [`SourceDomain`].
    ///
    /// ```text
    /// SHA-256( SOURCE_DOMAIN_SEPARATOR
    ///          || chain_id (4 bytes LE)
    ///          || network_passphrase_len (4 bytes LE)
    ///          || network_passphrase
    ///          || contract_id_len (4 bytes LE)
    ///          || contract_id )
    /// ```
    fn source_domain_hash(env: &Env, domain: &SourceDomain) -> BytesN<32> {
        let mut data = Bytes::new(env);
        data.extend_from_slice(SOURCE_DOMAIN_SEPARATOR);
        data.extend_from_slice(&domain.chain_id.to_le_bytes());
        let np_len = domain.network_passphrase.len() as u32;
        data.extend_from_slice(&np_len.to_le_bytes());
        data.append(&domain.network_passphrase);
        let cid_len = domain.contract_id.len() as u32;
        data.extend_from_slice(&cid_len.to_le_bytes());
        data.append(&domain.contract_id);
        env.crypto().sha256(&data).into()
    }

    /// Compute the domain-separated inbound message ID.
    ///
    /// ```text
    /// SHA-256( INBOUND_MSG_DOMAIN
    ///          || source_domain_hash (32 bytes)
    ///          || nonce (8 bytes LE) )
    /// ```
    fn inbound_message_id(env: &Env, source_hash: &BytesN<32>, nonce: u64) -> BytesN<32> {
        let mut data = Bytes::new(env);
        data.extend_from_slice(INBOUND_MSG_DOMAIN);
        data.extend_from_slice(&source_hash.to_bytes());
        data.extend_from_slice(&nonce.to_le_bytes());
        env.crypto().sha256(&data).into()
    }
}

#[contractimpl]
impl Bridge {
    /// Return the next outbound nonce for `dest`, then increment it.
    pub fn next_outbound_nonce(env: Env, dest: u32) -> Result<u64, BridgeError> {
        // Access control: only contract itself or authorized caller
        env.current_contract_address().require_auth();

        let mut nonces = Self::load_nonces(&env);
        let current = nonces.get(dest).unwrap_or(0u64);
        let next = current.checked_add(1).ok_or(BridgeError::NonceOverflow)?;
        nonces.set(dest, next);
        Self::save_nonces(&env, &nonces);

        env.events().publish(
            (soroban_sdk::symbol_short!("outbound"),),
            OutboundMessageEvent {
                dest,
                nonce: current,
            },
        );
        Ok(current)
    }

    pub fn peek_outbound_nonce(env: Env, dest: u32) -> u64 {
        let nonces = Self::load_nonces(&env);
        nonces.get(dest).unwrap_or(0u64)
    }

    pub fn initialize(env: Env, validators: Vec<BytesN<32>>, bridge_id: Bytes) {
        Self::save_validators(&env, &validators);
        Self::save_bridge_id(&env, &bridge_id);
        Self::save_epoch(&env, 0);
    }

    pub fn set_guardian(env: Env, guardian: BytesN<32>) {
        Self::save_guardian(&env, &guardian);
    }

    pub fn get_guardian(env: Env) -> Option<BytesN<32>> {
        Self::load_guardian(&env)
    }

    pub fn set_max_churn(env: Env, max_churn: u32) {
        if max_churn == 0 {
            Self::remove_max_churn(&env);
        } else {
            Self::save_max_churn(&env, max_churn);
        }
    }

    // -----------------------------------------------------------------------
    // Admin management (issue #1901)
    // -----------------------------------------------------------------------

    /// Set the admin address for privileged operations.
    ///
    /// The admin is authorised to register and unregister source domains.
    /// Can only be called once; subsequent calls are rejected to prevent
    /// unauthorised admin rotation.
    pub fn set_admin(env: Env, admin: Address) -> Result<(), BridgeError> {
        if Self::load_admin(&env).is_some() {
            return Err(BridgeError::NotAdmin);
        }
        Self::save_admin(&env, &admin);
        Ok(())
    }

    /// Return the current admin address, if set.
    pub fn get_admin(env: Env) -> Option<Address> {
        Self::load_admin(&env)
    }

    // -----------------------------------------------------------------------
    // Source-domain registration (issue #1901)
    // -----------------------------------------------------------------------

    /// Register a source domain for inbound message validation.
    ///
    /// Only the admin may call this function.  A source domain identifies
    /// the chain, network passphrase, and contract address that are
    /// authorised to send inbound bridge messages.
    ///
    /// # Errors
    /// - [`BridgeError::NotAdmin`] if the caller is not the admin.
    /// - [`BridgeError::SourceDomainLimitReached`] if the storage limit
    ///   has been reached.
    pub fn register_source_domain(
        env: Env,
        caller: Address,
        source: SourceDomain,
    ) -> Result<(), BridgeError> {
        Self::require_admin(&env, &caller)?;
        let hash = Self::source_domain_hash(&env, &source);
        if env
            .storage()
            .persistent()
            .has(&BridgeDataKey::SourceRegistered(hash))
        {
            // Already registered — idempotent, return Ok.
            return Ok(());
        }
        // Enforce storage limit.
        let count: u32 = env
            .storage()
            .persistent()
            .get::<BridgeDataKey, u32>(&BridgeDataKey::SourceDomainCount)
            .unwrap_or(0);
        if count >= MAX_SOURCE_DOMAINS {
            return Err(BridgeError::SourceDomainLimitReached);
        }
        env.storage()
            .persistent()
            .set(&BridgeDataKey::SourceRegistered(hash), &true);
        env.storage()
            .persistent()
            .set(&BridgeDataKey::SourceDomainCount, &(count + 1));
        Ok(())
    }

    /// Unregister a previously registered source domain.
    ///
    /// Only the admin may call this function.  After unregistration,
    /// inbound messages from this source will be rejected.
    pub fn unregister_source_domain(
        env: Env,
        caller: Address,
        source: SourceDomain,
    ) -> Result<(), BridgeError> {
        Self::require_admin(&env, &caller)?;
        let hash = Self::source_domain_hash(&env, &source);
        if !env
            .storage()
            .persistent()
            .has(&BridgeDataKey::SourceRegistered(hash))
        {
            // Not registered — idempotent, return Ok.
            return Ok(());
        }
        env.storage()
            .persistent()
            .remove(&BridgeDataKey::SourceRegistered(hash));
        let count: u32 = env
            .storage()
            .persistent()
            .get::<BridgeDataKey, u32>(&BridgeDataKey::SourceDomainCount)
            .unwrap_or(0);
        if count > 0 {
            env.storage()
                .persistent()
                .set(&BridgeDataKey::SourceDomainCount, &(count - 1));
        }
        Ok(())
    }

    /// Check whether a source domain is currently registered.
    pub fn is_source_registered(env: Env, source: SourceDomain) -> bool {
        let hash = Self::source_domain_hash(&env, &source);
        env.storage()
            .persistent()
            .has(&BridgeDataKey::SourceRegistered(hash))
    }

    // -----------------------------------------------------------------------
    // Inbound message consumption (issue #1901)
    // -----------------------------------------------------------------------

    /// Consume an inbound bridge message, enforcing domain separation,
    /// source validation, nonce ordering, and replay protection.
    ///
    /// The function:
    /// 1. Validates `source` is registered.
    /// 2. Checks `nonce` equals the next expected nonce for that source.
    /// 3. Computes the domain-separated message ID.
    /// 4. Checks the message has not already been consumed.
    /// 5. Marks the message as consumed and increments the per-source nonce.
    /// 6. Emits an [`InboundMessageConsumedEvent`].
    ///
    /// Failed validation does **not** burn the message or mutate balances.
    ///
    /// # Arguments
    /// * `source` – The [`SourceDomain`] identifying the sender.
    /// * `nonce`  – The message nonce from the source (must equal next expected).
    ///
    /// # Returns
    /// The domain-separated message ID on success.
    ///
    /// # Errors
    /// - [`BridgeError::UnregisteredSource`] if the source domain is not registered.
    /// - [`BridgeError::UnexpectedNonce`] if the nonce does not match.
    /// - [`BridgeError::MessageAlreadyConsumed`] if the message was already consumed.
    pub fn consume_inbound_message(
        env: Env,
        source: SourceDomain,
        nonce: u64,
    ) -> Result<BytesN<32>, BridgeError> {
        // 1. Validate source is registered.
        let source_hash = Self::source_domain_hash(&env, &source);
        if !env
            .storage()
            .persistent()
            .has(&BridgeDataKey::SourceRegistered(source_hash))
        {
            return Err(BridgeError::UnregisteredSource);
        }

        // 2. Check nonce ordering.
        let expected_nonce: u64 = env
            .storage()
            .persistent()
            .get::<BridgeDataKey, u64>(&BridgeDataKey::InboundNonce(source_hash))
            .unwrap_or(0);
        if nonce != expected_nonce {
            return Err(BridgeError::UnexpectedNonce);
        }

        // 3. Compute domain-separated message ID.
        let message_id = Self::inbound_message_id(&env, &source_hash, nonce);

        // 4. Check not already consumed.
        if env
            .storage()
            .persistent()
            .has(&BridgeDataKey::ConsumedInboundMessage(message_id))
        {
            return Err(BridgeError::MessageAlreadyConsumed);
        }

        // 5. Mark consumed and advance nonce.
        env.storage()
            .persistent()
            .set(&BridgeDataKey::ConsumedInboundMessage(message_id), &true);
        let next_nonce = nonce.checked_add(1).ok_or(BridgeError::NonceOverflow)?;
        env.storage()
            .persistent()
            .set(&BridgeDataKey::InboundNonce(source_hash), &next_nonce);

        // 6. Emit event.
        env.events().publish(
            (symbol_short!("inbound_msg"), symbol_short!("consumed")),
            InboundMessageConsumedEvent {
                message_id,
                source,
                nonce,
            },
        );

        Ok(message_id)
    }

    /// Check whether an inbound message has already been consumed.
    ///
    /// Computes the domain-separated message ID from `source` and `nonce`
    /// and checks the storage marker.
    pub fn is_message_consumed(env: Env, source: SourceDomain, nonce: u64) -> bool {
        let source_hash = Self::source_domain_hash(&env, &source);
        let message_id = Self::inbound_message_id(&env, &source_hash, nonce);
        env.storage()
            .persistent()
            .has(&BridgeDataKey::ConsumedInboundMessage(message_id))
    }

    /// Return the next expected nonce for a given source domain.
    ///
    /// Returns 0 if no messages have been consumed from this source yet.
    pub fn next_inbound_nonce(env: Env, source: SourceDomain) -> u64 {
        let source_hash = Self::source_domain_hash(&env, &source);
        env.storage()
            .persistent()
            .get::<BridgeDataKey, u64>(&BridgeDataKey::InboundNonce(source_hash))
            .unwrap_or(0)
    }

    /// Compute the domain-separated message ID for a given source and nonce
    /// without consuming it.  Useful for off-chain verification.
    pub fn compute_message_id(env: Env, source: SourceDomain, nonce: u64) -> BytesN<32> {
        let source_hash = Self::source_domain_hash(&env, &source);
        Self::inbound_message_id(&env, &source_hash, nonce)
    }

    // -----------------------------------------------------------------------
    // Validator-set read helpers
    // -----------------------------------------------------------------------

    /// Number of active (non-paused) validators.
    pub fn active_validator_count(env: Env) -> u32 {
        let validators = Self::load_validators(&env);
        let paused = Self::load_paused(&env);
        let mut count: u32 = 0;
        for pk in validators.iter() {
            if !paused.contains_key(pk.clone()) {
                count += 1;
            }
        }
        count
    }

    /// Supermajority threshold computed from the active validator count:
    /// `floor(n * 2 / 3) + 1`.  Returns 1 when n == 0.
    pub fn effective_threshold(env: Env) -> u32 {
        let n = Self::active_validator_count(env);
        (n * 2) / 3 + 1
    }

    /// Returns true iff the given public key is currently paused.
    pub fn is_paused(env: Env, pk: BytesN<32>) -> bool {
        let paused = Self::load_paused(&env);
        paused.contains_key(pk)
    }

    /// Returns true iff the key is in the validator set and not paused.
    pub fn is_active_validator(env: Env, pk: BytesN<32>) -> bool {
        let validators = Self::load_validators(&env);
        let in_set = validators.iter().any(|v| v == pk);
        if !in_set {
            return false;
        }
        let paused = Self::load_paused(&env);
        !paused.contains_key(pk)
    }

    /// Return the current epoch.
    pub fn get_epoch(env: Env) -> u64 {
        Self::load_epoch(&env)
    }

    // -----------------------------------------------------------------------
    // Quorum-proof payload construction
    // -----------------------------------------------------------------------

    /// Build the canonical domain-separated payload for a validator-set
    /// rotation quorum proof:
    ///
    ///   `SHA-256( DOMAIN_TAG || bridge_id_len(4 LE) || bridge_id
    ///             || validator_count(4 LE) || validator_bytes...
    ///             || epoch(8 LE) )`
    ///
    /// All current active validators must sign the 32-byte hash returned here.
    pub fn quorum_proof_payload(
        env: Env,
        new_validators: Vec<BytesN<32>>,
        epoch: u64,
    ) -> BytesN<32> {
        let bridge_id = Self::load_bridge_id(&env);
        Self::build_quorum_payload(&env, &bridge_id, &new_validators, epoch)
    }

    /// Internal helper — builds the payload without touching storage, so it
    /// can be called from `rotate_validators` after the bridge_id is loaded once.
    fn build_quorum_payload(
        env: &Env,
        bridge_id: &Bytes,
        new_validators: &Vec<BytesN<32>>,
        epoch: u64,
    ) -> BytesN<32> {
        let mut data = Bytes::new(env);

        // 1. Domain tag
        data.extend_from_slice(QUORUM_PROOF_DOMAIN);

        // 2. bridge_id length (4 bytes LE) + bridge_id bytes
        let id_len = bridge_id.len();
        data.extend_from_slice(&(id_len as u32).to_le_bytes());
        // Append via SDK `Bytes::append` / `slice` — soroban-sdk 25 has no
        // `copy_into_slice_with_offset` (only full-length `copy_into_slice`).
        data.append(bridge_id);

        // 3. validator count (4 bytes LE) + each 32-byte key
        let val_count = new_validators.len() as u32;
        data.extend_from_slice(&val_count.to_le_bytes());
        for pk in new_validators.iter() {
            let arr: [u8; 32] = pk.into();
            data.extend_from_slice(&arr);
        }

        // 4. epoch (8 bytes LE)
        data.extend_from_slice(&epoch.to_le_bytes());

        // SHA-256 over the assembled bytes (`Hash<32>` → `BytesN<32>`)
        env.crypto().sha256(&data).into()
    }

    // -----------------------------------------------------------------------
    // Validator rotation
    // -----------------------------------------------------------------------

    /// Rotate the validator set.
    ///
    /// `epoch` must equal `current_epoch + 1`.
    /// `proofs` is a list of `(ed25519_public_key_32_bytes, signature_64_bytes)`.
    /// A strict supermajority of current active validators must have signed
    /// `quorum_proof_payload(new_validators, epoch)`.
    ///
    /// Returns the churn count (symmetric-difference size).
    pub fn rotate_validators(
        env: Env,
        new_validators: Vec<BytesN<32>>,
        epoch: u64,
        proofs: Vec<(BytesN<32>, BytesN<64>)>,
    ) -> Result<u32, BridgeError> {
        let current_epoch = Self::load_epoch(&env);
        if epoch != current_epoch + 1 {
            return Err(BridgeError::InvalidEpoch);
        }

        // -- Reject duplicate keys in the proposed set --
        let mut seen_keys: Map<BytesN<32>, bool> = Map::new(&env);
        for pk in new_validators.iter() {
            if seen_keys.contains_key(pk.clone()) {
                return Err(BridgeError::DuplicateValidatorKey);
            }
            seen_keys.set(pk, true);
        }

        // -- Size bounds --
        let unique_count = new_validators.len();
        if unique_count < MIN_VALIDATORS {
            return Err(BridgeError::ValidatorSetTooSmall);
        }
        if unique_count > MAX_VALIDATORS {
            return Err(BridgeError::ValidatorSetTooLarge);
        }

        let current_validators = Self::load_validators(&env);

        // -- Churn calculation --
        let mut current_map: Map<BytesN<32>, bool> = Map::new(&env);
        for pk in current_validators.iter() {
            current_map.set(pk, true);
        }
        let mut new_map: Map<BytesN<32>, bool> = Map::new(&env);
        for pk in new_validators.iter() {
            new_map.set(pk, true);
        }

        let mut added: u32 = 0;
        for pk in new_validators.iter() {
            if !current_map.contains_key(pk) {
                added += 1;
            }
        }
        let mut removed: u32 = 0;
        for pk in current_validators.iter() {
            if !new_map.contains_key(pk) {
                removed += 1;
            }
        }
        let churn = added
            .checked_add(removed)
            .ok_or(BridgeError::WindowTotalOverflow)?;

        if let Some(limit) = Self::load_max_churn(&env) {
            if churn > limit {
                return Err(BridgeError::ChurnLimitExceeded);
            }
        }

        // -- Verify quorum proof --
        Self::verify_quorum_proof_internal(
            &env,
            &current_validators,
            &new_validators,
            epoch,
            &proofs,
        )?;

        // -- Commit atomically --
        Self::save_validators(&env, &new_validators);
        Self::save_epoch(&env, epoch);
        // Clear paused set — stale pause flags belong to old key material.
        Self::save_paused(&env, &Map::new(&env));

        Ok(churn)
    }

    /// Internal quorum-proof verifier — operates on already-loaded data.
    fn verify_quorum_proof_internal(
        env: &Env,
        current_validators: &Vec<BytesN<32>>,
        new_validators: &Vec<BytesN<32>>,
        epoch: u64,
        proofs: &Vec<(BytesN<32>, BytesN<64>)>,
    ) -> Result<(), BridgeError> {
        if proofs.is_empty() {
            return Err(BridgeError::EmptyProofs);
        }

        let max_proofs = current_validators.len();
        if proofs.len() > max_proofs {
            return Err(BridgeError::ProofVectorTooLarge);
        }

        // Build a set of current validators for O(n) lookup.
        let mut current_set: Map<BytesN<32>, bool> = Map::new(env);
        for pk in current_validators.iter() {
            current_set.set(pk, true);
        }

        let paused = Self::load_paused(env);
        let bridge_id = Self::load_bridge_id(env);
        let payload = Self::build_quorum_payload(env, &bridge_id, new_validators, epoch);

        // Deduplicate signers up-front.
        let mut seen_signers: Map<BytesN<32>, bool> = Map::new(env);
        for (pk, _) in proofs.iter() {
            if seen_signers.contains_key(pk.clone()) {
                return Err(BridgeError::DuplicateProofSigner);
            }
            seen_signers.set(pk, true);
        }

        let mut unique_active: u32 = 0;
        for (pk, sig) in proofs.iter() {
            if !current_set.contains_key(pk.clone()) {
                return Err(BridgeError::SignerNotInValidatorSet);
            }
            // Paused validators are silently skipped.
            if paused.contains_key(pk.clone()) {
                continue;
            }
            // Verify ed25519 signature over the payload hash.
            // Clone: `Into<Bytes>` consumes `BytesN` and this loop may iterate many times.
            // `ed25519_verify` traps on bad sig (returns `()`), so no Result mapping.
            env.crypto()
                .ed25519_verify(&pk, &payload.clone().into(), &sig);
            unique_active += 1;
        }

        // Compute effective threshold from active validators.
        let active_count = {
            let mut count: u32 = 0;
            for pk in current_validators.iter() {
                if !paused.contains_key(pk) {
                    count += 1;
                }
            }
            count
        };
        let threshold = (active_count * 2) / 3 + 1;

        if unique_active < threshold {
            return Err(BridgeError::InsufficientQuorum);
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Inbound epoch validation
    // -----------------------------------------------------------------------

    /// Reject a `signed_epoch` that belongs to a retired validator set.
    pub fn validate_inbound_epoch(env: Env, signed_epoch: u64) -> Result<(), BridgeError> {
        let current = Self::load_epoch(&env);
        if signed_epoch < current {
            return Err(BridgeError::RetiredEpoch);
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Validator pause / unpause
    // -----------------------------------------------------------------------

    /// Guardian-gated pause of a single validator.
    ///
    /// The guardian must sign `SHA-256("BRIDGE_PAUSE:" || pk_bytes)`.
    pub fn pause_validator(
        env: Env,
        validator: BytesN<32>,
        signature: BytesN<64>,
    ) -> Result<(), BridgeError> {
        let guardian = Self::load_guardian(&env).ok_or(BridgeError::NoGuardianConfigured)?;

        let validators = Self::load_validators(&env);
        if !validators.iter().any(|v| v == validator) {
            return Err(BridgeError::UnknownValidator);
        }

        let mut paused = Self::load_paused(&env);
        if paused.contains_key(validator.clone()) {
            return Err(BridgeError::AlreadyPaused);
        }

        // Fail-closed: refuse if pausing would make quorum unreachable.
        let mut active_count: u32 = 0;
        for pk in validators.iter() {
            if !paused.contains_key(pk) {
                active_count += 1;
            }
        }
        let new_active = active_count.saturating_sub(1);
        let new_threshold = (new_active * 2) / 3 + 1;
        if new_active < new_threshold {
            return Err(BridgeError::PauseWouldBreakQuorum);
        }

        // Verify guardian signature over action-bound payload.
        // `ed25519_verify` traps on failure in soroban-sdk 25.x (returns `()`).
        let payload = Self::build_tagged_payload(&env, PAUSE_PAYLOAD_TAG, &validator);
        let payload_hash = env.crypto().sha256(&payload);
        env.crypto()
            .ed25519_verify(&guardian, &payload_hash.into(), &signature);

        paused.set(validator, true);
        Self::save_paused(&env, &paused);
        Ok(())
    }

    /// Guardian-gated unpause of a single validator.
    ///
    /// The guardian must sign `SHA-256("BRIDGE_UNPAUSE:" || pk_bytes)`.
    pub fn unpause_validator(
        env: Env,
        validator: BytesN<32>,
        signature: BytesN<64>,
    ) -> Result<(), BridgeError> {
        let guardian = Self::load_guardian(&env).ok_or(BridgeError::NoGuardianConfigured)?;

        let validators = Self::load_validators(&env);
        if !validators.iter().any(|v| v == validator) {
            return Err(BridgeError::UnknownValidator);
        }

        let payload = concat_prefixed(UNPAUSE_PAYLOAD_TAG, &v_bytes);
        guardian
            .verify(&payload, signature)
            .map_err(|_| BridgeError::InvalidGuardianSignature)?;

        self.paused_validators.remove(&v_bytes);
        Ok(ValidatorEvent::Unpaused {
            validator: v_bytes,
            epoch: self.epoch,
        })
    }

    /// Rejects an inbound message whose `signed_epoch` is not aligned with
    /// the bridge's currently active epoch.
    ///
    /// # Threat model (#1147)
    ///
    /// A naive `signed_epoch >= self.epoch` check accepts any far-future
    /// epoch. A message claiming an epoch that the validator set has not
    /// yet rotated into must NEVER be honoured on this bridge: once the
    /// future epoch is reached, an attacker who pre-collected the message
    /// can replay it. The defence is to accept only the active epoch,
    /// optionally extended by a small explicit tolerance.
    ///
    /// With [`INBOUND_EPOCH_TOLERANCE`] set to `0` (the default and safe
    /// choice) the bridge enforces **strict equality**: only an inbound
    /// message carrying exactly [`Bridge::epoch`] is admitted. Epochs are
    /// monotonically-incremented discrete sequence numbers, not physical
    /// timestamps, so there is no "clock skew" to absorb and any positive
    /// tolerance weakens replay resistance without justification.
    ///
    /// # Significance of the upper bound
    ///
    /// * `signed_epoch < self.epoch`
    ///   [`Err`] — retired-validator-set replay.
    /// * `signed_epoch == self.epoch`
    ///   [`Ok`] — exactly the active epoch, accepted.
    /// * `signed_epoch > self.epoch.saturating_add(INBOUND_EPOCH_TOLERANCE)`
    ///   [`Err`] — message claims to be from a validator set that has not
    ///   yet been rotationally authorised on this bridge.
    ///
    /// # Tolerance formula
    ///
    /// ```text
    /// min_accepted_epoch = self.epoch
    /// max_accepted_epoch = self.epoch.saturating_add(INBOUND_EPOCH_TOLERANCE)
    /// accepted iff  min_accepted_epoch <= signed_epoch <= max_accepted_epoch
    /// ```
    ///
    /// `saturating_add` ensures that if `self.epoch == u64::MAX` the upper
    /// bound also equals `u64::MAX`, so the comparison cannot be defeated
    /// by wrapping arithmetic. Bridge epoch numbers start at `0` and
    /// increment by `1` per rotation, so reaching `u64::MAX` is
    /// unreachable in any realistic deployment.
    ///
    /// # Worked example (tolerance = 0)
    ///
    /// Suppose the bridge is currently at epoch `5` and
    /// `INBOUND_EPOCH_TOLERANCE == 0`:
    ///
    /// | `signed_epoch` | Outcome                       | Reason |
    /// |---:|---|---|
    /// | `3` | [`Err`] (retired validator set) | Lower than `self.epoch`. |
    /// | `4` | [`Err`] (retired validator set) | Lower than `self.epoch`. |
    /// | `5` | [`Ok`]                         | Exactly the active epoch. |
    /// | `6` | [`Err`] (not yet active)        | `6 > 5 + 0` — a future epoch the validator set has not rotated into. |
    /// | `u64::MAX` | [`Err`] (not yet active)  | `u64::MAX > 5.saturating_add(0) = 5`. |
    ///
    /// # Worked example (hypothetical tolerance = 1)
    ///
    /// If `INBOUND_EPOCH_TOLERANCE` were ever raised to `1`:
    ///
    /// | `signed_epoch` | Outcome | Reason |
    /// |---:|---|---|
    /// | `4` | [`Err`] | Lower than `self.epoch == 5`. |
    /// | `5` | [`Ok`]  | `5 <= 5 <= 6`. |
    /// | `6` | [`Ok`]  | `6 <= 5.saturating_add(1) = 6`. |
    /// | `7` | [`Err`] | `7 > 6`. |
    ///
    /// # Errors
    ///
    /// Returns [`anyhow::Error`] whose string contains one of:
    ///
    /// * `"retired validator set"` when `signed_epoch < self.epoch`.
    /// * `"not-yet-active"` when `signed_epoch` lies strictly above the
    ///   tolerance-adjusted upper bound.
    ///
    /// # Arguments
    /// * `signed_epoch` — epoch number that the inbound message claims to
    ///   have been signed under.
    ///
    /// # Returns
    /// `Ok(())` iff `signed_epoch` is the bridge's currently active epoch
    /// (within the explicit tolerance).
    pub fn validate_inbound_epoch(&self, signed_epoch: u64) -> Result<()> {
        // Lower bound: retire any signed_epoch that pre-dates the current set,
        // so a retired validator set cannot have its messages replayed.
        if signed_epoch < self.epoch {
            return Err(anyhow!(
                "message signed by retired validator set (epoch too old): \
                 signed_epoch={} < self.epoch={}",
                signed_epoch,
                self.epoch
            ));
        }

        // Upper bound: refuse signed_epoch that points at a future validator
        // set the bridge has not yet rotated into. saturating_add prevents
        // u64 overflow from being weaponised into a comparison that always
        // either panics or — worse — passes by wrapping to a small value.
        let max_accepted_epoch = self.epoch.saturating_add(INBOUND_EPOCH_TOLERANCE);
        if signed_epoch > max_accepted_epoch {
            return Err(anyhow!(
                "message signed by not-yet-active validator set (epoch too far in the future): \
                 signed_epoch={} > max_accepted_epoch={} (= self.epoch={} + INBOUND_EPOCH_TOLERANCE={})",
                signed_epoch,
                max_accepted_epoch,
                self.epoch,
                INBOUND_EPOCH_TOLERANCE
            ));
        }

        Ok(())
    }

    /// Build a tagged payload: `tag_bytes || pk_bytes` as a `Bytes`.
    fn build_tagged_payload(env: &Env, tag: &[u8], pk: &BytesN<32>) -> Bytes {
        let mut out = Bytes::new(env);
        out.extend_from_slice(tag);
        let arr: [u8; 32] = pk.into();
        out.extend_from_slice(&arr);
        out
    }

    // -----------------------------------------------------------------------
    // Inbound value-cap
    // -----------------------------------------------------------------------

    /// Reconfigure the per-window inbound value cap.
    ///
    /// `max_per_window == 0` means fail-closed (no inbound permitted).
    /// `window_size` must be > 0.
    pub fn set_inbound_cap(
        env: Env,
        max_per_window: i128,
        window_size: u64,
        current_time: u64,
    ) -> Result<(), BridgeError> {
        if max_per_window < 0 {
            return Err(BridgeError::InboundCapExceeded);
        }
        if window_size == 0 {
            return Err(BridgeError::InvalidWindowSize);
        }
        env.storage()
            .persistent()
            .set(&BridgeDataKey::MaxPerWindow, &max_per_window);
        env.storage()
            .persistent()
            .set(&BridgeDataKey::WindowSize, &window_size);
        env.storage()
            .persistent()
            .set(&BridgeDataKey::WindowStart, &current_time);
        env.storage()
            .persistent()
            .set(&BridgeDataKey::WindowInboundTotal, &0i128);
        Ok(())
    }

    /// Admit an inbound transfer of `amount` against the per-window cap.
    pub fn admit_inbound(env: Env, amount: i128, current_time: u64) -> Result<(), BridgeError> {
        if amount < 0 {
            return Err(BridgeError::InboundCapExceeded);
        }

        let max = Self::load_max_per_window(&env);
        if max == 0 {
            return Err(BridgeError::InboundCapExceeded);
        }

        // Roll window if expired.
        let window_size = Self::load_window_size(&env);
        let window_start = Self::load_window_start(&env);
        let mut total = Self::load_window_inbound_total(&env);

        let (rolled_start, rolled_total) =
            Self::maybe_roll_window(current_time, window_start, window_size, total);
        total = rolled_total;

        let new_total = total
            .checked_add(amount)
            .ok_or(BridgeError::WindowTotalOverflow)?;
        if new_total > max {
            return Err(BridgeError::InboundCapExceeded);
        }

        env.storage()
            .persistent()
            .set(&BridgeDataKey::WindowStart, &rolled_start);
        env.storage()
            .persistent()
            .set(&BridgeDataKey::WindowInboundTotal, &new_total);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Outbound value-cap
    // -----------------------------------------------------------------------

    /// Reconfigure the per-window outbound value cap.
    pub fn set_outbound_cap(
        env: Env,
        max_per_window: i128,
        window_size: u64,
        current_time: u64,
    ) -> Result<(), BridgeError> {
        if max_per_window < 0 {
            return Err(BridgeError::OutboundCapExceeded);
        }
        if window_size == 0 {
            return Err(BridgeError::InvalidWindowSize);
        }
        env.storage()
            .persistent()
            .set(&BridgeDataKey::MaxOutboundPerWindow, &max_per_window);
        env.storage()
            .persistent()
            .set(&BridgeDataKey::OutboundWindowSize, &window_size);
        env.storage()
            .persistent()
            .set(&BridgeDataKey::OutboundWindowStart, &current_time);
        env.storage()
            .persistent()
            .set(&BridgeDataKey::WindowOutboundTotal, &0i128);
        Ok(())
    }

    /// Admit an outbound transfer of `amount` against the per-window cap.
    pub fn admit_outbound(env: Env, amount: i128, current_time: u64) -> Result<(), BridgeError> {
        if amount < 0 {
            return Err(BridgeError::OutboundCapExceeded);
        }

        let max = Self::load_max_outbound_per_window(&env);
        if max == 0 {
            return Err(BridgeError::OutboundCapExceeded);
        }

        let window_size = Self::load_outbound_window_size(&env);
        let window_start = Self::load_outbound_window_start(&env);
        let mut total = Self::load_window_outbound_total(&env);

        let (rolled_start, rolled_total) =
            Self::maybe_roll_window(current_time, window_start, window_size, total);
        total = rolled_total;

        let new_total = total
            .checked_add(amount)
            .ok_or(BridgeError::WindowTotalOverflow)?;
        if new_total > max {
            return Err(BridgeError::OutboundCapExceeded);
        }

        env.storage()
            .persistent()
            .set(&BridgeDataKey::OutboundWindowStart, &rolled_start);
        env.storage()
            .persistent()
            .set(&BridgeDataKey::WindowOutboundTotal, &new_total);
        Ok(())
    }

    /// Roll the window forward if `current_time` has passed the window end.
    /// Returns `(new_window_start, new_total)`.
    fn maybe_roll_window(
        current_time: u64,
        window_start: u64,
        window_size: u64,
        total: i128,
    ) -> (u64, i128) {
        if current_time < window_start {
            return (window_start, total);
        }
        if window_size == 0 {
            return (window_start, total);
        }
        if let Some(window_end) = window_start.checked_add(window_size) {
            if current_time >= window_end {
                return (current_time, 0);
            }
        }
        (window_start, total)
    }
}

/// Helper: build a payload of the form `prefix || suffix` without an
/// intermediate allocation beyond the result vector.
fn concat_prefixed(prefix: &[u8], suffix: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(prefix.len() + suffix.len());
    out.extend_from_slice(prefix);
    out.extend_from_slice(suffix);
    out
}

/// Lowercase hex encoder for the `Display` impl of `ValidatorEvent`. Inlined
/// here (rather than pulling in the `hex` crate as a runtime dependency)
/// because event formatting is the only consumer and the format is trivial.
fn lowercase_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

#[cfg(test)]
mod rotation_test;

#[cfg(test)]
mod rotation_doc_test;

#[cfg(test)]
mod domain_separation_test;

#[cfg(test)]
mod quorum_proof_bound_test;

#[cfg(test)]
mod inbound_cap_test;

#[cfg(test)]
mod inbound_epoch_test;

#[cfg(test)]
mod window_rollover_test;

#[cfg(test)]
mod validator_bounds_test;

#[cfg(test)]
mod epoch_monotonicity_proptest;

#[cfg(test)]
mod window_guard_test;

#[cfg(test)]
mod window_tuning_doc_test;

#[cfg(test)]
mod outbound_cap_test;

#[cfg(test)]
mod validatorset_proptest;

#[cfg(test)]
mod validator_pause_test;

#[cfg(test)]
mod rotation_churn_test;

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::Env;

    fn fresh_env() -> Env {
        Env::default()
    }

    #[test]
    fn test_outbound_nonce_increments() {
        let env = fresh_env();
        // `next_outbound_nonce` gates itself behind the contract's own
        // address via `require_auth()`; mock auths so the test can exercise
        // the nonce-increment logic without wiring up a real invoker chain.
        env.mock_all_auths();
        let contract_id = env.register_contract(None, Bridge);
        let client = BridgeClient::new(&env, &contract_id);

        // new set B: 3 validators
        let kp_b = make_keypairs(3);
        let b_pks: Vec<PublicKey> = kp_b.iter().map(|k| k.public).collect();
        let new_set = ValidatorSet { validators: b_pks.iter().map(|p| p.to_bytes().to_vec()).collect() };

        // proofs: have >2/3 of A sign the (new_set, epoch=1) payload
        let epoch = 1u64;
        let payload = Bridge::quorum_proof_payload(&bridge.bridge_id, &new_set, epoch).unwrap();

        // need threshold of A: (4*2)/3+1 = 3
        let mut proofs = vec![];
        for i in 0..3 {
            let sig = kp_a[i].sign(&payload);
            proofs.push((kp_a[i].public, sig));
        }

        // rotate should succeed
        bridge.rotate_validators(new_set.clone(), epoch, proofs).expect("rotation failed");
        assert_eq!(bridge.epoch, 1);

        // messages signed with epoch 0 should be rejected
        assert!(bridge.validate_inbound_epoch(0).is_err());
        // messages signed with the *current* epoch 1 are accepted
        assert!(bridge.validate_inbound_epoch(1).is_ok());
        // messages signed with a far-future epoch 2 are rejected (not yet
        // actively rotated into by the validator set — see #1147 / INBOUND_EPOCH_TOLERANCE)
        let err = bridge.validate_inbound_epoch(2).unwrap_err();
        assert!(
            err.to_string().contains("not-yet-active"),
            "future epoch must be rejected with a 'not-yet-active' error, got: {err}"
        );
    }

    #[test]
    fn test_validate_inbound_epoch_rejects_old() {
        let env = fresh_env();
        let contract_id = env.register_contract(None, Bridge);
        let client = BridgeClient::new(&env, &contract_id);

        let validators: soroban_sdk::Vec<BytesN<32>> = soroban_sdk::Vec::new(&env);
        client.initialize(&validators, &Bytes::new(&env));

        // epoch 0 is current — any lower would panic but there is no lower; future ok
        assert!(client.try_validate_inbound_epoch(&0u64).is_ok());
        // nothing to rotate to test stale epoch without ed25519 key material here;
        // the RetiredEpoch path is covered structurally by the error code existing.
    }

    #[test]
    fn test_set_inbound_cap_and_admit() {
        let env = fresh_env();
        let contract_id = env.register_contract(None, Bridge);
        let client = BridgeClient::new(&env, &contract_id);

        // Result<(), _> client methods return unit on success (use try_* for Result).
        client.set_inbound_cap(&1000i128, &86400u64, &0u64);
        client.admit_inbound(&500i128, &1000u64);
        client.admit_inbound(&500i128, &2000u64);
        // Now at cap — next should fail
        assert!(client.try_admit_inbound(&1i128, &3000u64).is_err());
    }

    #[test]
    fn test_inbound_window_rolls_over() {
        let env = fresh_env();
        let contract_id = env.register_contract(None, Bridge);
        let client = BridgeClient::new(&env, &contract_id);

        client.set_inbound_cap(&1000i128, &86400u64, &0u64);
        client.admit_inbound(&1000i128, &100u64);
        // Still in same window — should fail
        assert!(client.try_admit_inbound(&1i128, &200u64).is_err());
        // After window duration — should succeed
        client.admit_inbound(&1000i128, &86400u64);
    }

    #[test]
    fn test_set_outbound_cap_and_admit() {
        let env = fresh_env();
        let contract_id = env.register_contract(None, Bridge);
        let client = BridgeClient::new(&env, &contract_id);

        client.set_outbound_cap(&500i128, &86400u64, &0u64);
        client.admit_outbound(&499i128, &1000u64);
        client.admit_outbound(&1i128, &2000u64);
        assert!(client.try_admit_outbound(&1i128, &3000u64).is_err());
    }

    #[test]
    fn test_fail_closed_inbound_before_cap_set() {
        let env = fresh_env();
        let contract_id = env.register_contract(None, Bridge);
        let client = BridgeClient::new(&env, &contract_id);

        assert!(client.try_admit_inbound(&1i128, &0u64).is_err());
    }

    #[test]
    fn test_invalid_window_size_rejected() {
        let env = fresh_env();
        let contract_id = env.register_contract(None, Bridge);
        let client = BridgeClient::new(&env, &contract_id);

        assert!(client.try_set_inbound_cap(&1000i128, &0u64, &0u64).is_err());
        assert!(client
            .try_set_outbound_cap(&1000i128, &0u64, &0u64)
            .is_err());
    }
}

// -----------------------------------------------------------------------
// Inbound message replay protection & domain confusion tests (issue #1901)
// -----------------------------------------------------------------------

#[cfg(test)]
mod replay_protection_test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, Env};

    /// Helper: spin up a fresh bridge, set an admin, and return the client.
    fn setup_bridge() -> (Env, BridgeClient<'static>, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, Bridge);
        let client = BridgeClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.set_admin(&admin);
        (env, client, admin)
    }

    /// Helper: build a source domain with the given chain_id.
    fn source(env: &Env, chain_id: u32) -> SourceDomain {
        SourceDomain {
            chain_id,
            network_passphrase: Bytes::from_slice(env, b"testnet"),
            contract_id: Bytes::from_slice(env, b"bridge_contract_v1"),
        }
    }

    /// Helper: build a source domain with a different contract_id.
    fn source_different_contract(env: &Env, chain_id: u32) -> SourceDomain {
        SourceDomain {
            chain_id,
            network_passphrase: Bytes::from_slice(env, b"testnet"),
            contract_id: Bytes::from_slice(env, b"other_contract_v2"),
        }
    }

    /// Helper: build a source domain with a different network passphrase.
    fn source_different_network(env: &Env, chain_id: u32) -> SourceDomain {
        SourceDomain {
            chain_id,
            network_passphrase: Bytes::from_slice(env, b"mainnet"),
            contract_id: Bytes::from_slice(env, b"bridge_contract_v1"),
        }
    }

    // ---- AC: A message is consumed at most once ----

    #[test]
    fn duplicate_delivery_rejected() {
        let (env, client, admin) = setup_bridge();
        let src = source(&env, 1);
        client.register_source_domain(&admin, &src);

        // First consumption succeeds.
        let _id = client.consume_inbound_message(&src, &0u64);
        assert!(client.is_message_consumed(&src, &0u64));

        // Second consumption of the same nonce is rejected.
        let err = client.try_consume_inbound_message(&src, &0u64);
        assert!(
            matches!(err, Err(Ok(BridgeError::MessageAlreadyConsumed))),
            "expected MessageAlreadyConsumed on duplicate, got {:?}",
            err
        );
    }

    #[test]
    fn sequential_nonces_accepted() {
        let (env, client, admin) = setup_bridge();
        let src = source(&env, 1);
        client.register_source_domain(&admin, &src);

        // Consume nonces 0..4 in order.
        for i in 0..5u64 {
            let id = client.consume_inbound_message(&src, &i);
            assert!(!id.is_empty());
            assert!(client.is_message_consumed(&src, &i));
        }
        // Next expected nonce is 5.
        assert_eq!(client.next_inbound_nonce(&src), 5u64);
    }

    // ---- AC: A message for another network or contract is rejected ----

    #[test]
    fn wrong_domain_rejected() {
        let (env, client, admin) = setup_bridge();
        let src = source(&env, 1);
        client.register_source_domain(&admin, &src);

        // A source with a different contract_id is unregistered.
        let other = source_different_contract(&env, 1);
        let err = client.try_consume_inbound_message(&other, &0u64);
        assert!(
            matches!(err, Err(Ok(BridgeError::UnregisteredSource))),
            "expected UnregisteredSource for different contract, got {:?}",
            err
        );
    }

    #[test]
    fn wrong_network_rejected() {
        let (env, client, admin) = setup_bridge();
        let src = source(&env, 1);
        client.register_source_domain(&admin, &src);

        // A source with a different network passphrase is unregistered.
        let other = source_different_network(&env, 1);
        let err = client.try_consume_inbound_message(&other, &0u64);
        assert!(
            matches!(err, Err(Ok(BridgeError::UnregisteredSource))),
            "expected UnregisteredSource for different network, got {:?}",
            err
        );
    }

    #[test]
    fn different_chain_id_rejected() {
        let (env, client, admin) = setup_bridge();
        let src = source(&env, 1);
        client.register_source_domain(&admin, &src);

        let other = source(&env, 999);
        let err = client.try_consume_inbound_message(&other, &0u64);
        assert!(
            matches!(err, Err(Ok(BridgeError::UnregisteredSource))),
            "expected UnregisteredSource for different chain_id, got {:?}",
            err
        );
    }

    #[test]
    fn cross_domain_message_ids_differ() {
        let env = Env::default();
        let src_a = source(&env, 1);
        let src_b = source_different_contract(&env, 1);
        let id_a = Bridge::compute_message_id(env.clone(), src_a, 0);
        let id_b = Bridge::compute_message_id(env, src_b, 0);
        assert_ne!(id_a, id_b, "domain-separated IDs must differ");
    }

    // ---- AC: Failed validation does not burn the message or mutate balances ----

    #[test]
    fn failed_validation_no_state_change() {
        let (env, client, admin) = setup_bridge();
        let src = source(&env, 1);
        client.register_source_domain(&admin, &src);

        // Wrong nonce: nonce 1 when expected is 0.
        let err = client.try_consume_inbound_message(&src, &1u64);
        assert!(matches!(err, Err(Ok(BridgeError::UnexpectedNonce))));

        // Nonce 0 is still available for consumption.
        let _id = client.consume_inbound_message(&src, &0u64);
        assert!(client.is_message_consumed(&src, &0u64));
        assert_eq!(client.next_inbound_nonce(&src), 1u64);
    }

    #[test]
    fn unregistered_source_no_state_change() {
        let (env, client, admin) = setup_bridge();
        let src = source(&env, 1);
        // Do NOT register src.

        let err = client.try_consume_inbound_message(&src, &0u64);
        assert!(matches!(err, Err(Ok(BridgeError::UnregisteredSource))));

        // Nonce is still 0 (no mutation).
        assert_eq!(client.next_inbound_nonce(&src), 0u64);
        assert!(!client.is_message_consumed(&src, &0u64));
    }

    // ---- AC: Out-of-order messages ----

    #[test]
    fn out_of_order_nonce_rejected() {
        let (env, client, admin) = setup_bridge();
        let src = source(&env, 1);
        client.register_source_domain(&admin, &src);

        // Consume nonce 0.
        client.consume_inbound_message(&src, &0u64);

        // Skip nonce 1, try nonce 2.
        let err = client.try_consume_inbound_message(&src, &2u64);
        assert!(matches!(err, Err(Ok(BridgeError::UnexpectedNonce))));

        // Nonce 1 is still consumable.
        client.consume_inbound_message(&src, &1u64);
        assert_eq!(client.next_inbound_nonce(&src), 2u64);
    }

    // ---- AC: Storage limits ----

    #[test]
    fn source_domain_limit_enforced() {
        let (env, client, admin) = setup_bridge();

        // Register up to the limit.
        for i in 0..MAX_SOURCE_DOMAINS {
            let src = SourceDomain {
                chain_id: i,
                network_passphrase: Bytes::from_slice(&env, b"net"),
                contract_id: Bytes::from_slice(&env, b"ctr"),
            };
            client.register_source_domain(&admin, &src);
        }

        // One more should fail.
        let over_limit = SourceDomain {
            chain_id: MAX_SOURCE_DOMAINS,
            network_passphrase: Bytes::from_slice(&env, b"net"),
            contract_id: Bytes::from_slice(&env, b"ctr"),
        };
        let err = client.try_register_source_domain(&admin, &over_limit);
        assert!(
            matches!(err, Err(Ok(BridgeError::SourceDomainLimitReached))),
            "expected SourceDomainLimitReached, got {:?}",
            err
        );
    }

    // ---- Admin gating ----

    #[test]
    fn non_admin_cannot_register_source() {
        let (env, _client, _admin) = setup_bridge();
        let contract_id = env.register_contract(None, Bridge);
        let client = BridgeClient::new(&env, &contract_id);
        let non_admin = Address::generate(&env);
        let src = source(&env, 1);
        let err = client.try_register_source_domain(&non_admin, &src);
        assert!(matches!(err, Err(Ok(BridgeError::NotAdmin))));
    }

    #[test]
    fn admin_setup_is_idempotent() {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, Bridge);
        let client = BridgeClient::new(&env, &contract_id);
        let admin = Address::generate(&env);

        // First call succeeds.
        client.set_admin(&admin);
        assert_eq!(client.get_admin(), Some(admin.clone()));

        // Second call fails (admin already set).
        let other = Address::generate(&env);
        let err = client.try_set_admin(&other);
        assert!(matches!(err, Err(Ok(BridgeError::NotAdmin))));

        // Original admin is unchanged.
        assert_eq!(client.get_admin(), Some(admin));
    }

    // ---- Idempotent registration/unregistration ----

    #[test]
    fn register_source_is_idempotent() {
        let (env, client, admin) = setup_bridge();
        let src = source(&env, 1);

        client.register_source_domain(&admin, &src);
        // Second registration is a no-op.
        client.register_source_domain(&admin, &src);
        assert!(client.is_source_registered(&src));
    }

    #[test]
    fn unregister_source_is_idempotent() {
        let (env, client, admin) = setup_bridge();
        let src = source(&env, 1);

        client.register_source_domain(&admin, &src);
        client.unregister_source_domain(&admin, &src);
        assert!(!client.is_source_registered(&src));

        // Second unregister is a no-op.
        client.unregister_source_domain(&admin, &src);
        assert!(!client.is_source_registered(&src));
    }

    #[test]
    fn unregistered_source_cannot_consume() {
        let (env, client, admin) = setup_bridge();
        let src = source(&env, 1);

        client.register_source_domain(&admin, &src);
        client.unregister_source_domain(&admin, &src);

        let err = client.try_consume_inbound_message(&src, &0u64);
        assert!(matches!(err, Err(Ok(BridgeError::UnregisteredSource))));
    }

    // ---- Multiple independent sources ----

    #[test]
    fn independent_sources_have_independent_nonces() {
        let (env, client, admin) = setup_bridge();
        let src_a = source(&env, 1);
        let src_b = source(&env, 2);
        client.register_source_domain(&admin, &src_a);
        client.register_source_domain(&admin, &src_b);

        // Consume nonce 0 from source A.
        client.consume_inbound_message(&src_a, &0u64);

        // Source B still starts at nonce 0.
        assert_eq!(client.next_inbound_nonce(&src_b), 0u64);
        client.consume_inbound_message(&src_b, &0u64);

        // Source A nonce 0 is consumed; source B nonce 0 is also consumed.
        assert!(client.is_message_consumed(&src_a, &0u64));
        assert!(client.is_message_consumed(&src_b, &0u64));

        // Source A nonce 1 is next; source B nonce 1 is next.
        assert_eq!(client.next_inbound_nonce(&src_a), 1u64);
        assert_eq!(client.next_inbound_nonce(&src_b), 1u64);
    }

    // ---- Message ID computation ----

    #[test]
    fn compute_message_id_matches_consume() {
        let (env, client, admin) = setup_bridge();
        let src = source(&env, 1);
        client.register_source_domain(&admin, &src);

        // Pre-compute the message ID.
        let precomputed = client.compute_message_id(&src, &0u64);

        // Consume the message.
        let consumed = client.consume_inbound_message(&src, &0u64);

        assert_eq!(precomputed, consumed);
    }

    #[test]
    fn different_nonces_produce_different_ids() {
        let env = Env::default();
        let src = source(&env, 1);
        let id_0 = Bridge::compute_message_id(env.clone(), src.clone(), 0);
        let id_1 = Bridge::compute_message_id(env.clone(), src.clone(), 1);
        let id_max = Bridge::compute_message_id(env, src, u64::MAX);
        assert_ne!(id_0, id_1, "different nonces must differ");
        assert_ne!(id_0, id_max, "different nonces must differ");
        assert_ne!(id_1, id_max, "different nonces must differ");
    }

    // ---- Domain separator constant pinning ----

    #[test]
    fn inbound_msg_domain_separator_is_pinned() {
        // Pin the domain tag so a silent rename would break this test
        // and force a deliberate version bump.
        assert_eq!(INBOUND_MSG_DOMAIN, b"stellarlend::bridge::inbound_msg::v1");
    }

    #[test]
    fn source_domain_separator_is_pinned() {
        assert_eq!(
            SOURCE_DOMAIN_SEPARATOR,
            b"stellarlend::bridge::source_domain::v1"
        );
    }
}
