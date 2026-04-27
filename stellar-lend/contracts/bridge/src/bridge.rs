//! # Bridge Registry
//!
//! Manages cross-chain bridge configurations for the StellarLend protocol.
//! Each bridge entry represents a connection to a remote blockchain network
//! and carries its own fee schedule, minimum-amount floor, and active flag.
//!
//! ## Architecture
//!
//! This contract is a **registry and accounting layer only**. It records
//! deposit and withdrawal events and tracks cumulative totals; it does **not**
//! hold or transfer tokens. Actual cross-chain asset movement is performed by
//! an off-chain relayer that reads the events emitted here.
//!
//! ## Trust Boundaries
//!
//! | Actor           | Capabilities |
//! |-----------------|--------------|
//! | Protocol admin  | `register_bridge`, `set_bridge_fee`, `set_bridge_active`, `set_relayer`, `transfer_admin` |
//! | Designated relayer | `bridge_withdraw` (record outbound cross-chain transfers) |
//! | Any caller      | `bridge_deposit` (record inbound cross-chain transfers) |
//!
//! Admin and relayer are strictly separated: the relayer can only execute
//! withdrawals, never mutate bridge configuration.
//!
//! ## Replay Protection
//!
//! Every [`BridgeConfig`] stores a `network_id` that identifies the remote
//! chain this bridge connects to. Deposit and withdrawal events include the
//! `network_id` so off-chain indexers and relayers can verify the intended
//! destination chain before executing a transfer. Transaction-level uniqueness
//! is provided by the Stellar ledger's sequence-number mechanism; this
//! contract does not maintain a per-operation nonce.
//!
//! ## Reentrancy
//!
//! Soroban contracts execute atomically within a single transaction invocation
//! and cannot be re-entered. This contract makes no cross-contract calls, so
//! reentrancy is structurally impossible.
//!
//! ## Arithmetic Safety
//!
//! All fee arithmetic uses [`I256`] intermediate values to prevent overflow.
//! Cumulative accounting uses `checked_add` / `checked_sub`; overflows return
//! [`ContractError::Overflow`] rather than wrapping silently.

use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, log, symbol_short, Address,
    BytesN, Env, String, Symbol, Vec, I256,
};

// ── Error type ────────────────────────────────────────────────────────────────

/// All errors that may be returned by the bridge registry contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum ContractError {
    /// Contract has already been initialised via [`BridgeContract::init`].
    AlreadyInitialised = 1,
    /// Contract has not yet been initialised.
    NotInitialised = 2,
    /// Caller is not the admin (or relayer, where applicable).
    Unauthorised = 3,
    /// A bridge with the given ID is already registered.
    BridgeAlreadyExists = 4,
    /// No bridge found with the given ID.
    BridgeNotFound = 5,
    /// Bridge is inactive; deposits are not accepted.
    BridgeInactive = 6,
    /// `fee_bps` exceeds [`MAX_FEE_BPS`] (1 000 — 10%).
    FeeTooHigh = 7,
    /// Bridge ID length is 0 or exceeds [`MAX_ID_LEN`] (64 bytes).
    InvalidBridgeIdLen = 8,
    /// Bridge ID contains a character outside `[a-zA-Z0-9_-]`.
    InvalidBridgeIdChar = 9,
    /// `min_amount` is negative.
    NegativeMinAmount = 10,
    /// Deposit or withdrawal `amount` is zero or negative.
    AmountNotPositive = 11,
    /// `amount` is below the bridge's `min_amount` floor.
    AmountBelowMinimum = 12,
    /// Integer overflow in accounting arithmetic.
    Overflow = 13,
}

// ── Events ────────────────────────────────────────────────────────────────────

/// Emitted when a new bridge endpoint is registered.
#[contractevent]
#[derive(Clone, Debug)]
pub struct BridgeRegisteredEvent {
    /// Identifier of the newly registered bridge.
    pub bridge_id: String,
    /// Remote chain identifier.
    pub network_id: u32,
    /// Initial fee in basis points.
    pub fee_bps: u64,
    /// Minimum deposit / withdrawal amount.
    pub min_amount: i128,
}

/// Emitted when the fee for an existing bridge is updated.
#[contractevent]
#[derive(Clone, Debug)]
pub struct BridgeFeeUpdatedEvent {
    /// Identifier of the affected bridge.
    pub bridge_id: String,
    /// New fee in basis points.
    pub fee_bps: u64,
}

/// Emitted when a bridge's active status changes.
#[contractevent]
#[derive(Clone, Debug)]
pub struct BridgeActiveUpdatedEvent {
    /// Identifier of the affected bridge.
    pub bridge_id: String,
    /// New active status.
    pub active: bool,
}

/// Emitted when tokens are deposited through a bridge.
#[contractevent]
#[derive(Clone, Debug)]
pub struct BridgeDepositEvent {
    /// Bridge used for this deposit.
    pub bridge_id: String,
    /// Remote chain identifier (for replay-protection verification).
    pub network_id: u32,
    /// Address that initiated the deposit.
    pub sender: Address,
    /// Gross amount deposited.
    pub amount: i128,
    /// Fee deducted from `amount`.
    pub fee: i128,
    /// Net amount after fee (`amount - fee`); relayer credits this to recipient.
    pub net: i128,
}

/// Emitted when tokens are withdrawn through a bridge.
#[contractevent]
#[derive(Clone, Debug)]
pub struct BridgeWithdrawalEvent {
    /// Bridge used for this withdrawal.
    pub bridge_id: String,
    /// Remote chain identifier (for replay-protection verification).
    pub network_id: u32,
    /// Address to receive the withdrawn tokens on the destination chain.
    pub recipient: Address,
    /// Gross withdrawal amount.
    pub amount: i128,
}

/// Emitted when the designated relayer address is updated.
#[contractevent]
#[derive(Clone, Debug)]
pub struct RelayerUpdatedEvent {
    /// New relayer address.
    pub relayer: Address,
}

/// Emitted when admin rights are transferred.
#[contractevent]
#[derive(Clone, Debug)]
pub struct AdminTransferredEvent {
    /// Address that received admin rights.
    pub new_admin: Address,
}

// ── Constants ─────────────────────────────────────────────────────────────────

/// Maximum allowed fee: 10% (1 000 basis points).
pub const MAX_FEE_BPS: u64 = 1_000;

/// Maximum byte length of a bridge identifier.
pub const MAX_ID_LEN: u32 = 64;

/// Instance-storage symbol for the protocol admin address.
const ADMIN_KEY: Symbol = symbol_short!("ADMIN");

/// Instance-storage symbol for the optional relayer address.
const RELAYER_KEY: Symbol = symbol_short!("RELAYER");

// ── Storage types ─────────────────────────────────────────────────────────────

/// On-chain configuration for a single bridge endpoint.
///
/// Stored in **persistent** storage keyed by [`DataKey::Bridge`].
/// Fields are versioned implicitly by the contract upgrade mechanism;
/// future migrations should introduce a new struct version if fields change.
#[contracttype]
#[derive(Clone, Debug)]
pub struct BridgeConfig {
    /// Human-readable identifier, e.g. `"eth-mainnet"` or `"bsc-testnet"`.
    pub bridge_id: String,
    /// Remote chain identifier. Used by the relayer to prevent cross-chain
    /// replay of withdrawal messages.
    pub network_id: u32,
    /// Protocol fee in basis points (0 – 1 000, i.e. 0% – 10%).
    pub fee_bps: u64,
    /// Minimum gross amount for deposits and withdrawals. Guards against dust
    /// transactions that generate disproportionate gas overhead.
    pub min_amount: i128,
    /// `true` while the bridge accepts new deposits; `false` when paused.
    /// Withdrawals remain possible while inactive so in-flight transfers
    /// can be settled.
    pub active: bool,
    /// Cumulative gross amount deposited through this bridge (accounting only).
    pub total_deposited: i128,
    /// Cumulative gross amount withdrawn through this bridge (accounting only).
    pub total_withdrawn: i128,
}

/// Storage keys for bridge-specific data.
#[contracttype]
pub enum DataKey {
    /// Per-bridge configuration, keyed by the bridge ID string.
    Bridge(String),
    /// Ordered list of all registered bridge IDs (instance storage).
    BridgeList,
}

// ── Contract ──────────────────────────────────────────────────────────────────

#[contract]
pub struct BridgeContract;

#[contractimpl]
impl BridgeContract {
    // ── Initialisation ────────────────────────────────────────────────────────

    /// Initialise the bridge registry and set the protocol admin.
    ///
    /// Must be called exactly once, immediately after deployment.
    ///
    /// # Errors
    /// * [`ContractError::AlreadyInitialised`] – called more than once.
    ///
    /// # Security
    /// No authentication is required for `init` because the contract has no
    /// privileged state yet; the first caller becomes admin. Operators MUST
    /// call `init` in the same transaction as deployment to prevent
    /// front-running.
    pub fn init(env: Env, admin: Address) -> Result<(), ContractError> {
        if env.storage().instance().has(&ADMIN_KEY) {
            return Err(ContractError::AlreadyInitialised);
        }
        env.storage().instance().set(&ADMIN_KEY, &admin);
        Ok(())
    }

    // ── register_bridge ───────────────────────────────────────────────────────

    /// Register a new bridge endpoint (admin only).
    ///
    /// The bridge is created in the *active* state with zero cumulative totals.
    ///
    /// # Parameters
    /// * `bridge_id` – 1–64 ASCII chars from `[a-zA-Z0-9_-]`.
    /// * `network_id` – Remote chain identifier (e.g. EVM chain ID).
    /// * `fee_bps` – Fee in basis points; must be ≤ [`MAX_FEE_BPS`] (1 000).
    /// * `min_amount` – Minimum deposit / withdrawal amount; must be ≥ 0.
    ///
    /// # Errors
    /// * [`ContractError::Unauthorised`] – caller is not the admin.
    /// * [`ContractError::InvalidBridgeIdLen`] – `bridge_id` is empty or > 64 chars.
    /// * [`ContractError::InvalidBridgeIdChar`] – `bridge_id` contains illegal chars.
    /// * [`ContractError::FeeTooHigh`] – `fee_bps` > 1 000.
    /// * [`ContractError::NegativeMinAmount`] – `min_amount` < 0.
    /// * [`ContractError::BridgeAlreadyExists`] – ID already registered.
    ///
    /// # Security
    /// Admin authorisation is verified via Soroban's `require_auth` before any
    /// storage mutation. Bridge IDs are length- and character-validated to
    /// prevent storage-key injection.
    pub fn register_bridge(
        env: Env,
        caller: Address,
        bridge_id: String,
        network_id: u32,
        fee_bps: u64,
        min_amount: i128,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &caller)?;
        Self::validate_id(&bridge_id)?;

        if fee_bps > MAX_FEE_BPS {
            return Err(ContractError::FeeTooHigh);
        }
        if min_amount < 0 {
            return Err(ContractError::NegativeMinAmount);
        }
        if env
            .storage()
            .persistent()
            .has(&DataKey::Bridge(bridge_id.clone()))
        {
            return Err(ContractError::BridgeAlreadyExists);
        }

        let cfg = BridgeConfig {
            bridge_id: bridge_id.clone(),
            network_id,
            fee_bps,
            min_amount,
            active: true,
            total_deposited: 0,
            total_withdrawn: 0,
        };
        Self::save_bridge(&env, &bridge_id, &cfg);

        let mut list = Self::bridge_list(&env);
        list.push_back(bridge_id.clone());
        env.storage().instance().set(&DataKey::BridgeList, &list);

        BridgeRegisteredEvent {
            bridge_id: bridge_id.clone(),
            network_id,
            fee_bps,
            min_amount,
        }
        .publish(&env);
        log!(
            &env,
            "register_bridge {} network_id={}",
            bridge_id,
            network_id
        );
        Ok(())
    }

    // ── set_bridge_fee ────────────────────────────────────────────────────────

    /// Update the fee (in basis points) for an existing bridge (admin only).
    ///
    /// Fee changes take effect immediately for all subsequent operations.
    ///
    /// # Errors
    /// * [`ContractError::Unauthorised`] – caller is not the admin.
    /// * [`ContractError::FeeTooHigh`] – `fee_bps` > [`MAX_FEE_BPS`].
    /// * [`ContractError::BridgeNotFound`] – no bridge with this ID.
    pub fn set_bridge_fee(
        env: Env,
        caller: Address,
        bridge_id: String,
        fee_bps: u64,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &caller)?;

        if fee_bps > MAX_FEE_BPS {
            return Err(ContractError::FeeTooHigh);
        }

        let mut cfg = Self::load_bridge(&env, &bridge_id)?;
        cfg.fee_bps = fee_bps;
        Self::save_bridge(&env, &bridge_id, &cfg);

        BridgeFeeUpdatedEvent {
            bridge_id: bridge_id.clone(),
            fee_bps,
        }
        .publish(&env);
        Ok(())
    }

    // ── set_bridge_active ─────────────────────────────────────────────────────

    /// Enable or disable deposits for a bridge (admin only).
    ///
    /// When `active` is set to `false` the bridge is *paused*:
    /// - [`bridge_deposit`] rejects new deposits.
    /// - [`bridge_withdraw`] continues to accept withdrawals so that
    ///   in-flight cross-chain transfers can still be settled.
    ///
    /// # Errors
    /// * [`ContractError::Unauthorised`] – caller is not the admin.
    /// * [`ContractError::BridgeNotFound`] – no bridge with this ID.
    pub fn set_bridge_active(
        env: Env,
        caller: Address,
        bridge_id: String,
        active: bool,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &caller)?;

        let mut cfg = Self::load_bridge(&env, &bridge_id)?;
        cfg.active = active;
        Self::save_bridge(&env, &bridge_id, &cfg);

        BridgeActiveUpdatedEvent {
            bridge_id: bridge_id.clone(),
            active,
        }
        .publish(&env);
        Ok(())
    }

    // ── bridge_deposit ────────────────────────────────────────────────────────

    /// Record an inbound cross-chain deposit (any authenticated caller).
    ///
    /// The protocol fee is calculated, deducted from `amount`, and the net
    /// amount is returned and emitted in [`BridgeDepositEvent`] for the
    /// off-chain relayer to credit on the destination chain.
    ///
    /// # Returns
    /// Net amount after fee deduction (`amount - fee`).
    ///
    /// # Errors
    /// * [`ContractError::AmountNotPositive`] – `amount` ≤ 0.
    /// * [`ContractError::BridgeNotFound`] – no bridge with this ID.
    /// * [`ContractError::BridgeInactive`] – bridge is paused.
    /// * [`ContractError::AmountBelowMinimum`] – `amount < cfg.min_amount`.
    /// * [`ContractError::Overflow`] – accounting overflow (practically impossible).
    ///
    /// # Security
    /// `sender.require_auth()` ensures the Stellar transaction is signed by
    /// the depositing party. This contract does **not** custody tokens; the
    /// caller must separately transfer tokens to the bridge escrow before or
    /// after this call, depending on the bridge protocol design.
    pub fn bridge_deposit(
        env: Env,
        sender: Address,
        bridge_id: String,
        amount: i128,
    ) -> Result<i128, ContractError> {
        sender.require_auth();

        if amount <= 0 {
            return Err(ContractError::AmountNotPositive);
        }

        let mut cfg = Self::load_bridge(&env, &bridge_id)?;

        if !cfg.active {
            return Err(ContractError::BridgeInactive);
        }
        if amount < cfg.min_amount {
            return Err(ContractError::AmountBelowMinimum);
        }

        let fee = Self::compute_fee(env.clone(), amount, cfg.fee_bps);
        let net = amount.checked_sub(fee).ok_or(ContractError::Overflow)?;

        cfg.total_deposited = cfg
            .total_deposited
            .checked_add(amount)
            .ok_or(ContractError::Overflow)?;
        Self::save_bridge(&env, &bridge_id, &cfg);

        BridgeDepositEvent {
            bridge_id: bridge_id.clone(),
            network_id: cfg.network_id,
            sender: sender.clone(),
            amount,
            fee,
            net,
        }
        .publish(&env);
        log!(
            &env,
            "bridge_deposit {} network_id={} amount={} fee={} net={}",
            bridge_id,
            cfg.network_id,
            amount,
            fee,
            net
        );

        Ok(net)
    }

    // ── bridge_withdraw ───────────────────────────────────────────────────────

    /// Record an outbound cross-chain withdrawal (admin or relayer).
    ///
    /// Marks `amount` as withdrawn for accounting purposes and emits a
    /// [`BridgeWithdrawalEvent`] for the off-chain relayer to execute the
    /// corresponding token transfer on the destination chain.
    ///
    /// Withdrawals are intentionally **allowed on inactive bridges** to
    /// permit settlement of in-flight transfers after a bridge pause.
    ///
    /// # Errors
    /// * [`ContractError::Unauthorised`] – caller is neither admin nor relayer.
    /// * [`ContractError::AmountNotPositive`] – `amount` ≤ 0.
    /// * [`ContractError::BridgeNotFound`] – no bridge with this ID.
    /// * [`ContractError::AmountBelowMinimum`] – `amount < cfg.min_amount`.
    /// * [`ContractError::Overflow`] – accounting overflow.
    ///
    /// # Security
    /// Authorization requires either the admin or the designated relayer address
    /// (set via [`set_relayer`]). The `recipient` is recorded in the event; this
    /// contract does **not** transfer tokens. Relayers MUST verify the emitted
    /// `network_id` matches the intended destination chain before executing any
    /// off-chain token transfer to prevent cross-chain replay.
    pub fn bridge_withdraw(
        env: Env,
        caller: Address,
        bridge_id: String,
        recipient: Address,
        amount: i128,
    ) -> Result<(), ContractError> {
        Self::require_admin_or_relayer(&env, &caller)?;

        if amount <= 0 {
            return Err(ContractError::AmountNotPositive);
        }

        let mut cfg = Self::load_bridge(&env, &bridge_id)?;

        if amount < cfg.min_amount {
            return Err(ContractError::AmountBelowMinimum);
        }

        cfg.total_withdrawn = cfg
            .total_withdrawn
            .checked_add(amount)
            .ok_or(ContractError::Overflow)?;
        Self::save_bridge(&env, &bridge_id, &cfg);

        BridgeWithdrawalEvent {
            bridge_id: bridge_id.clone(),
            network_id: cfg.network_id,
            recipient: recipient.clone(),
            amount,
        }
        .publish(&env);
        log!(
            &env,
            "bridge_withdraw {} network_id={} -> {} amount={}",
            bridge_id,
            cfg.network_id,
            recipient,
            amount
        );
        Ok(())
    }

    // ── set_relayer ───────────────────────────────────────────────────────────

    /// Designate an address that may execute `bridge_withdraw` (admin only).
    ///
    /// Setting a relayer is **optional**. When no relayer is configured, only
    /// the admin may call `bridge_withdraw`. Calling this function again
    /// overwrites the previous relayer.
    ///
    /// # Errors
    /// * [`ContractError::Unauthorised`] – caller is not the admin.
    ///
    /// # Security
    /// Separation of duties: the relayer can only record withdrawals;
    /// it cannot modify bridge configuration or transfer admin rights.
    pub fn set_relayer(env: Env, caller: Address, relayer: Address) -> Result<(), ContractError> {
        Self::require_admin(&env, &caller)?;
        env.storage().instance().set(&RELAYER_KEY, &relayer);
        RelayerUpdatedEvent {
            relayer: relayer.clone(),
        }
        .publish(&env);
        log!(&env, "set_relayer new={}", relayer);
        Ok(())
    }

    // ── transfer_admin ────────────────────────────────────────────────────────

    /// Transfer admin rights to a new address (admin only).
    ///
    /// The previous admin loses all privileges immediately.
    ///
    /// # Errors
    /// * [`ContractError::Unauthorised`] – caller is not the current admin.
    pub fn transfer_admin(
        env: Env,
        caller: Address,
        new_admin: Address,
    ) -> Result<(), ContractError> {
        Self::require_admin(&env, &caller)?;
        env.storage().instance().set(&ADMIN_KEY, &new_admin);
        AdminTransferredEvent {
            new_admin: new_admin.clone(),
        }
        .publish(&env);
        log!(&env, "transfer_admin new={}", new_admin);
        Ok(())
    }

    // ── Queries ───────────────────────────────────────────────────────────────

    /// Retrieve the full configuration for a bridge.
    ///
    /// # Errors
    /// * [`ContractError::BridgeNotFound`] – no bridge with this ID.
    pub fn get_bridge_config(env: Env, bridge_id: String) -> Result<BridgeConfig, ContractError> {
        Self::load_bridge(&env, &bridge_id)
    }

    /// Return the ordered list of all registered bridge identifiers.
    pub fn list_bridges(env: Env) -> Vec<String> {
        Self::bridge_list(&env)
    }

    /// Return the current protocol admin address.
    ///
    /// # Errors
    /// * [`ContractError::NotInitialised`] – contract not yet initialised.
    pub fn get_admin(env: Env) -> Result<Address, ContractError> {
        Self::load_admin(&env)
    }

    /// Return the designated relayer address, or `None` if not set.
    pub fn get_relayer(env: Env) -> Option<Address> {
        env.storage().instance().get(&RELAYER_KEY)
    }

    /// Compute the protocol fee for a given amount and fee rate.
    ///
    /// Uses 256-bit intermediate arithmetic to prevent overflow.
    /// Rounds **down** (floor division).
    ///
    /// # Parameters
    /// * `amount` – Gross amount (must be positive in practice).
    /// * `fee_bps` – Fee rate in basis points (0 – 1 000).
    ///
    /// # Returns
    /// `floor(amount * fee_bps / 10_000)`, or `0` on conversion failure.
    pub fn compute_fee(env: Env, amount: i128, fee_bps: u64) -> i128 {
        let amount_256 = I256::from_i128(&env, amount);
        let bps_256 = I256::from_i128(&env, fee_bps as i128);

        amount_256
            .mul(&bps_256)
            .div(&I256::from_i128(&env, 10_000))
            .to_i128()
            .unwrap_or(0)
    }

    // ── Upgrade Management ────────────────────────────────────────────────────

    /// Initialise the upgrade sub-system (admin, current WASM hash, approvals threshold).
    pub fn upgrade_init(
        env: Env,
        admin: Address,
        current_wasm_hash: BytesN<32>,
        required_approvals: u32,
    ) {
        stellarlend_common::upgrade::UpgradeManager::init(
            env,
            admin,
            current_wasm_hash,
            required_approvals,
        );
    }

    /// Add an address to the upgrade approver set (admin only).
    pub fn upgrade_add_approver(env: Env, caller: Address, approver: Address) {
        stellarlend_common::upgrade::UpgradeManager::add_approver(env, caller, approver);
    }

    /// Remove an address from the upgrade approver set (admin only).
    pub fn upgrade_remove_approver(env: Env, caller: Address, approver: Address) {
        stellarlend_common::upgrade::UpgradeManager::remove_approver(env, caller, approver);
    }

    /// Propose a contract upgrade to a new WASM hash and version (admin only).
    ///
    /// Returns the new proposal ID.
    pub fn upgrade_propose(
        env: Env,
        caller: Address,
        new_wasm_hash: BytesN<32>,
        new_version: u32,
    ) -> u64 {
        stellarlend_common::upgrade::UpgradeManager::upgrade_propose(
            env,
            caller,
            new_wasm_hash,
            new_version,
        )
    }

    /// Approve a pending upgrade proposal. Returns total approval count.
    pub fn upgrade_approve(env: Env, caller: Address, proposal_id: u64) -> u32 {
        stellarlend_common::upgrade::UpgradeManager::upgrade_approve(env, caller, proposal_id)
    }

    /// Execute an approved upgrade proposal (replaces contract WASM in-place).
    pub fn upgrade_execute(env: Env, caller: Address, proposal_id: u64) {
        stellarlend_common::upgrade::UpgradeManager::upgrade_execute(env, caller, proposal_id);
    }

    /// Roll back a previously executed upgrade to the prior WASM hash.
    pub fn upgrade_rollback(env: Env, caller: Address, proposal_id: u64) {
        stellarlend_common::upgrade::UpgradeManager::upgrade_rollback(env, caller, proposal_id);
    }

    /// Query the status of an upgrade proposal.
    pub fn upgrade_status(
        env: Env,
        proposal_id: u64,
    ) -> stellarlend_common::upgrade::UpgradeStatus {
        stellarlend_common::upgrade::UpgradeManager::upgrade_status(env, proposal_id)
    }

    /// Return the current contract WASM hash tracked by the upgrade manager.
    pub fn current_wasm_hash(env: Env) -> BytesN<32> {
        stellarlend_common::upgrade::UpgradeManager::current_wasm_hash(env)
    }

    /// Return the current contract version number.
    pub fn current_version(env: Env) -> u32 {
        stellarlend_common::upgrade::UpgradeManager::current_version(env)
    }

    // ── Private helpers ───────────────────────────────────────────────────────

    fn load_admin(env: &Env) -> Result<Address, ContractError> {
        env.storage()
            .instance()
            .get(&ADMIN_KEY)
            .ok_or(ContractError::NotInitialised)
    }

    /// Require `caller` to be the protocol admin.
    ///
    /// Calls `caller.require_auth()` first (Soroban host-enforced), then
    /// compares against the stored admin address.
    fn require_admin(env: &Env, caller: &Address) -> Result<(), ContractError> {
        caller.require_auth();
        if *caller != Self::load_admin(env)? {
            return Err(ContractError::Unauthorised);
        }
        Ok(())
    }

    /// Require `caller` to be either the admin or the designated relayer.
    ///
    /// Used exclusively by [`bridge_withdraw`] to allow the separation of
    /// withdrawal execution from full admin privileges.
    fn require_admin_or_relayer(env: &Env, caller: &Address) -> Result<(), ContractError> {
        caller.require_auth();
        // Admin check
        let admin = Self::load_admin(env)?;
        if *caller == admin {
            return Ok(());
        }
        // Relayer check
        let relayer: Option<Address> = env.storage().instance().get(&RELAYER_KEY);
        if relayer.as_ref() == Some(caller) {
            return Ok(());
        }
        Err(ContractError::Unauthorised)
    }

    /// Validate a bridge ID: 1–[`MAX_ID_LEN`] bytes, ASCII chars `[a-zA-Z0-9_-]`.
    ///
    /// Uses `copy_into_slice` to read the raw UTF-8 bytes of the Soroban
    /// `String` into a fixed stack buffer, then validates each byte.
    fn validate_id(id: &String) -> Result<(), ContractError> {
        let len = id.len();
        if len == 0 || len > MAX_ID_LEN {
            return Err(ContractError::InvalidBridgeIdLen);
        }
        let len_usize = len as usize;
        let mut buf = [0u8; MAX_ID_LEN as usize];
        id.copy_into_slice(&mut buf[..len_usize]);
        for &b in &buf[..len_usize] {
            if !matches!(b, b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_') {
                return Err(ContractError::InvalidBridgeIdChar);
            }
        }
        Ok(())
    }

    fn load_bridge(env: &Env, bridge_id: &String) -> Result<BridgeConfig, ContractError> {
        env.storage()
            .persistent()
            .get(&DataKey::Bridge(bridge_id.clone()))
            .ok_or(ContractError::BridgeNotFound)
    }

    fn save_bridge(env: &Env, bridge_id: &String, cfg: &BridgeConfig) {
        env.storage()
            .persistent()
            .set(&DataKey::Bridge(bridge_id.clone()), cfg);
    }

    fn bridge_list(env: &Env) -> Vec<String> {
        env.storage()
            .instance()
            .get(&DataKey::BridgeList)
            .unwrap_or_else(|| Vec::new(env))
    }
}
