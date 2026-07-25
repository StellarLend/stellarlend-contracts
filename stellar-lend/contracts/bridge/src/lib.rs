#![no_std]
use soroban_sdk::{contract, contractimpl, contracterror, contracttype, Bytes, BytesN, Env, Map, Vec};

pub const QUORUM_PROOF_DOMAIN: &[u8] = b"stellarlend::bridge::quorum_proof::v1";
const PAUSE_PAYLOAD_TAG: &[u8] = b"BRIDGE_PAUSE:";
const UNPAUSE_PAYLOAD_TAG: &[u8] = b"BRIDGE_UNPAUSE:";

const MIN_VALIDATORS: u32 = 1;
const MAX_VALIDATORS: u32 = 64;

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
}

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
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OutboundMessageEvent {
    pub dest: u32,
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
        env.storage().persistent().set(&BridgeDataKey::OutboundNonces, nonces);
    }
    fn load_validators(env: &Env) -> Vec<BytesN<32>> {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, Vec<BytesN<32>>>(&BridgeDataKey::Validators)
            .unwrap_or_else(|| Vec::new(env))
    }
    fn save_validators(env: &Env, validators: &Vec<BytesN<32>>) {
        env.storage().persistent().set(&BridgeDataKey::Validators, validators);
    }
    fn load_paused(env: &Env) -> Map<BytesN<32>, bool> {
        env.storage()
            .persistent()
            .get::<BridgeDataKey, Map<BytesN<32>, bool>>(&BridgeDataKey::PausedValidators)
            .unwrap_or_else(|| Map::new(env))
    }
    fn save_paused(env: &Env, paused: &Map<BytesN<32>, bool>) {
        env.storage().persistent().set(&BridgeDataKey::PausedValidators, paused);
    }
    fn load_epoch(env: &Env) -> u64 {
        env.storage().persistent().get::<BridgeDataKey, u64>(&BridgeDataKey::Epoch).unwrap_or(0)
    }
    fn save_epoch(env: &Env, epoch: u64) {
        env.storage().persistent().set(&BridgeDataKey::Epoch, &epoch);
    }
    fn load_bridge_id(env: &Env) -> Bytes {
        env.storage().persistent().get::<BridgeDataKey, Bytes>(&BridgeDataKey::BridgeId).unwrap_or_else(|| Bytes::new(env))
    }
    fn save_bridge_id(env: &Env, id: &Bytes) {
        env.storage().persistent().set(&BridgeDataKey::BridgeId, id);
    }
    fn load_guardian(env: &Env) -> Option<BytesN<32>> {
        env.storage().persistent().get::<BridgeDataKey, BytesN<32>>(&BridgeDataKey::Guardian)
    }
    fn save_guardian(env: &Env, pk: &BytesN<32>) {
        env.storage().persistent().set(&BridgeDataKey::Guardian, pk);
    }
    fn load_max_churn(env: &Env) -> Option<u32> {
        env.storage().persistent().get::<BridgeDataKey, u32>(&BridgeDataKey::MaxChurn)
    }
    fn save_max_churn(env: &Env, limit: u32) {
        env.storage().persistent().set(&BridgeDataKey::MaxChurn, &limit);
    }
    fn remove_max_churn(env: &Env) {
        env.storage().persistent().remove(&BridgeDataKey::MaxChurn);
    }
    fn load_max_per_window(env: &Env) -> i128 {
        env.storage().persistent().get::<BridgeDataKey, i128>(&BridgeDataKey::MaxPerWindow).unwrap_or(0)
    }
    fn load_window_size(env: &Env) -> u64 {
        env.storage().persistent().get::<BridgeDataKey, u64>(&BridgeDataKey::WindowSize).unwrap_or(0)
    }
    fn load_window_start(env: &Env) -> u64 {
        env.storage().persistent().get::<BridgeDataKey, u64>(&BridgeDataKey::WindowStart).unwrap_or(0)
    }
    fn load_window_inbound_total(env: &Env) -> i128 {
        env.storage().persistent().get::<BridgeDataKey, i128>(&BridgeDataKey::WindowInboundTotal).unwrap_or(0)
    }
    fn load_max_outbound_per_window(env: &Env) -> i128 {
        env.storage().persistent().get::<BridgeDataKey, i128>(&BridgeDataKey::MaxOutboundPerWindow).unwrap_or(0)
    }
    fn load_outbound_window_size(env: &Env) -> u64 {
        env.storage().persistent().get::<BridgeDataKey, u64>(&BridgeDataKey::OutboundWindowSize).unwrap_or(0)
    }
    fn load_outbound_window_start(env: &Env) -> u64 {
        env.storage().persistent().get::<BridgeDataKey, u64>(&BridgeDataKey::OutboundWindowStart).unwrap_or(0)
    }
    fn load_window_outbound_total(env: &Env) -> i128 {
        env.storage().persistent().get::<BridgeDataKey, i128>(&BridgeDataKey::WindowOutboundTotal).unwrap_or(0)
    }
}

#[contractimpl]
impl Bridge {
    /// Return the next outbound nonce for `dest`, then increment it.
    pub fn next_outbound_nonce(env: Env, dest: u32) -> Result<u64, BridgeError> {
        // Access control: only contract itself or authorized caller
        env.require_auth(&env.current_contract());

        let mut nonces = Self::load_nonces(&env);
        let current = nonces.get(dest).unwrap_or(0u64);
        let next = current.checked_add(1).ok_or(BridgeError::NonceOverflow)?;
        nonces.set(dest, next);
        Self::save_nonces(&env, &nonces);

        env.events().publish(
            (soroban_sdk::symbol_short!("outbound"),),
            OutboundMessageEvent { dest, nonce: current },
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

    // ... (rest of the file remains unchanged, including validator rotation, pause/unpause, inbound/outbound caps, etc.)
}
