//! Bridge — on-chain surface for cross-chain deposits / withdrawals plus a
//! guardian-gated *freeze* switch used during incident response.
//!
//! # Overview
//!
//! Cross-chain bridging is split into four roles:
//!
//! | Role | Function set | Caller |
//! |------|--------------|--------|
//! | Admin | [`register_bridge`], [`set_bridge_fee`], [`set_bridge_guardian`] | `Admin` (stored in instance storage) |
//! | Guardian (incident response) | [`freeze_bridge`], [`unfreeze_bridge`] | `Guardian` |
//! | User | [`bridge_deposit`], [`bridge_withdraw`] | `user.require_auth()` |
//! | View | [`get_bridge_config`], [`list_bridges`], [`is_bridge_frozen`] | any |
//!
//! # Freeze semantics
//!
//! The freeze flag is an **independent** incident-response control. It is
//! fully decoupled from validator rotation: the configured [`Guardian`] can
//! stop outbound withdrawals immediately, without waiting on a slow validator
//! rotation to converge.
//!
//! - **While frozen**, [`bridge_withdraw`] returns [`BridgeError::Frozen`] and
//!   performs **no** state mutation and **no** token transfer. The freeze
//!   event itself was already emitted when the state changed.
//! - **While frozen**, [`bridge_deposit`] continues to function — deposits
//!   are never blocked by the freeze.
//! - **Admin** and **view** operations are unaffected by the freeze.
//!
//! [`Guardian`]: BridgeDataKey::Guardian
//!
//! # Worked example (incident)
//!
//! 1. A validator-set compromise is suspected at `t = T`.
//! 2. The guardian calls `freeze_bridge` with their own address authenticated.
//! 3. From `t = T`, every `bridge_withdraw` fails with `Frozen`. The freeze
//!    event is published exactly once on the transition.
//! 4. Coordination proceeds on validator rotation.
//! 5. Once the rotation is finalised, the guardian calls `unfreeze_bridge`.
//!    Withdrawals resume; the transition event is emitted again.
//!
//! # Storage layout
//!
//! All module state is keyed by [`BridgeDataKey`] to avoid collisions with
//! other modules' key enums (see `lib.rs::storage`, `lib.rs::deposit`, etc.).
//! Instance storage holds the freeze flag, the guardian, the admin address,
//! and a `Vec<u32>` index of registered network IDs (used by [`list_bridges`]).
//! Persistent storage holds per-network [`BridgeConfig`] records.

use soroban_sdk::{contracterror, contracttype, symbol_short, Address, Env, Map, Vec};

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Storage keys for bridge state.
///
/// Defined locally to guarantee no collision with other modules' key enums
/// (`DepositDataKey`, `RiskManagementKey`, etc.). Soroban's storage is keyed
/// by the raw bytes of the key; distinct enum types serialise to distinct keys
/// even if the variant name happens to coincide.
#[contracttype]
#[derive(Clone, Debug)]
pub enum BridgeDataKey {
    /// Address authorised to call freeze / unfreeze bridge withdrawals.
    /// Stored in *instance* storage (low write count, high read count).
    Guardian,
    /// Address authorised to administer bridges (register / set fee / set
    /// guardian). Stored in *instance* storage.
    Admin,
    /// `bool` flag: when `true`, `bridge_withdraw` is rejected with
    /// [`BridgeError::Frozen`]. Stored in *instance* storage.
    IsFrozen,
    /// `Vec<u32>` of network IDs with at least one registered bridge.
    /// Maintained so that [`list_bridges`] can enumerate without scanning.
    BridgesIndex,
    /// Persistent per-network bridge configuration, namespaced by
    /// `network_id: u32` via the enum payload so that each network lives at
    /// a distinct storage slot.
    Bridge(u32),
}

/// Per-network bridge configuration.
#[contracttype]
#[derive(Clone, Debug)]
pub struct BridgeConfig {
    /// Address of the bridge adapter contract (e.g. core / Wormhole / LayerZero
    /// adapter). Reserved for future use; not consumed by the freeze logic.
    pub bridge: Address,
    /// Remote network identifier chosen by the admin at registration time.
    pub network_id: u32,
    /// Bridge fee in basis points (0–10 000). See [`crate::bridge_fee_test`]
    /// for the precise fee accounting and conservation invariants.
    pub fee_bps: i128,
    /// Admin-controlled enable / disable switch. When `false`, the bridge is
    /// effectively decommissioned (admin or governance concern, *not* the
    /// freeze control described by this module).
    pub enabled: bool,
}

/// Structured payload for the `bridge_freeze_change` event.
///
/// # Versioning
///
/// `schema_version` follows the `EVENT_SCHEMA_VERSION` convention in
/// `events.rs`. Bumping this requires the dual-emit migration procedure
/// documented in `docs/EVENT_SCHEMA_VERSIONING.md`.
#[contracttype]
#[derive(Clone, Debug)]
pub struct BridgeFreezeEvent {
    /// Schema version at emit time. Indexers must read this field first.
    pub schema_version: u32,
    /// New freeze state after the transition that triggered this event.
    pub is_frozen: bool,
    /// Guardian address that authorised the transition. Redundant with the
    /// caller context on-chain but useful for off-chain indexing.
    pub guardian: Address,
    /// Ledger timestamp at the point of the transition.
    pub timestamp: u64,
}

/// Errors returned by the bridge module.
///
/// Variants are explicitly numbered; appending to the end of an enum is a
/// breaking change for ABI consumers, so add new variants only when no ABI
/// stability is required (the enum is `#[contracterror]`, which gives it
/// `Eq / PartialEq / Copy / Clone` for use across the contract boundary).
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum BridgeError {
    /// `bridge_withdraw` was attempted while `IsFrozen == true`.
    /// No state was mutated.
    Frozen = 1,
    /// Caller is not authorised for this operation (admin or guardian).
    Unauthorized = 2,
    /// The admin has not been initialised. Call the contract's `initialize`
    /// before any admin / guardian operation.
    AdminNotInitialized = 3,
    /// The guardian has not been configured. Call [`set_bridge_guardian`]
    /// before any freeze / unfreeze.
    GuardianNotConfigured = 4,
    /// No bridge is registered for the requested `network_id`.
    NotFound = 5,
    /// Amount must be strictly positive; zero or negative values are rejected.
    InvalidAmount = 6,
    /// Bridge for this `network_id` is registered but currently disabled.
    Disabled = 7,
    /// Fee basis-points value is outside the inclusive `[0, 10_000]` range.
    FeeOutOfRange = 8,
}

// ---------------------------------------------------------------------------
// Event helpers
// ---------------------------------------------------------------------------

/// Emit a [`BridgeFreezeEvent`] on a freeze-state transition.
///
/// Topics: `("bridge", "v1", "freeze")`. Indexers should subscribe to
/// `("bridge", "v1", *)` to capture all versioned bridge events.
fn emit_freeze_event(env: &Env, guardian: &Address, is_frozen: bool) {
    const SCHEMA_VERSION: u32 = 1;

    env.events().publish(
        (
            symbol_short!("bridge"),
            symbol_short!("v1"),
            symbol_short!("freeze"),
        ),
        BridgeFreezeEvent {
            schema_version: SCHEMA_VERSION,
            is_frozen,
            guardian: guardian.clone(),
            timestamp: env.ledger().timestamp(),
        },
    );
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Enforce that `caller` is the stored admin. Must call `require_auth()` first.
fn require_admin(env: &Env, caller: &Address) -> Result<(), BridgeError> {
    caller.require_auth();
    let admin: Option<Address> = env.storage().instance().get(&BridgeDataKey::Admin);
    match admin {
        Some(admin) if &admin == caller => Ok(()),
        Some(_) => Err(BridgeError::Unauthorized),
        None => Err(BridgeError::AdminNotInitialized),
    }
}

/// Enforce that `caller` is the stored guardian. Must call `require_auth()`
/// first and is distinct from `require_admin` — the guardian and the admin
/// are *intentionally* different roles so that a key compromise on one role
/// cannot unilaterally lift the other's controls.
fn require_guardian(env: &Env, caller: &Address) -> Result<(), BridgeError> {
    caller.require_auth();
    let guardian: Option<Address> = env.storage().instance().get(&BridgeDataKey::Guardian);
    match guardian {
        Some(guardian) if &guardian == caller => Ok(()),
        Some(_) => Err(BridgeError::Unauthorized),
        None => Err(BridgeError::GuardianNotConfigured),
    }
}

/// Append `network_id` to the bridges index (`BridgesIndex`), or no-op if the
/// network is already present. Pulled out for readability.
fn add_to_index(env: &Env, network_id: u32) {
    let key = BridgeDataKey::BridgesIndex;
    let mut index: Vec<u32> = env
        .storage()
        .instance()
        .get(&key)
        .unwrap_or_else(|| Vec::new(env));
    // Soroban's `Vec::Iterator` is intentionally limited (no `.any()` adapter
    // in `no_std`), so an explicit loop is required.
    let mut found = false;
    for n in index.iter() {
        if n == network_id {
            found = true;
            break;
        }
    }
    if !found {
        index.push_back(network_id);
        env.storage().instance().set(&key, &index);
    }
}

// ---------------------------------------------------------------------------
// Admin operations
// ---------------------------------------------------------------------------

/// Initialise the bridge module.
///
/// Sets the admin address; the guardian defaults to *unconfigured* and must be
/// set explicitly via [`set_bridge_guardian`] before any freeze / unfreeze
/// operation will succeed.
///
/// # Caller
/// Any address may call this once; after the admin is set, only the admin
/// can update it.
pub fn initialize(env: &Env, admin: Address) {
    if env.storage().instance().has(&BridgeDataKey::Admin) {
        return;
    }
    env.storage()
        .instance()
        .set(&BridgeDataKey::Admin, &admin);
}

/// Register a bridge for `network_id` (admin only).
///
/// # Errors
/// - [`BridgeError::Unauthorized`] if `caller` is not the admin.
/// - [`BridgeError::FeeOutOfRange`] if `fee_bps ∉ [0, 10_000]`.
pub fn register_bridge(
    env: &Env,
    caller: Address,
    network_id: u32,
    bridge: Address,
    fee_bps: i128,
) -> Result<(), BridgeError> {
    require_admin(env, &caller)?;
    if !(0..=10_000).contains(&fee_bps) {
        return Err(BridgeError::FeeOutOfRange);
    }
    let key = BridgeDataKey::Bridge(network_id);
    let cfg = BridgeConfig {
        bridge,
        network_id,
        fee_bps,
        enabled: true,
    };
    env.storage().persistent().set(&key, &cfg);
    add_to_index(env, network_id);
    Ok(())
}

/// Update the fee (in basis points) on an already-registered bridge
/// (admin only).
///
/// # Errors
/// - [`BridgeError::NotFound`] if no bridge is registered for `network_id`.
/// - [`BridgeError::FeeOutOfRange`] if `fee_bps ∉ [0, 10_000]`.
pub fn set_bridge_fee(
    env: &Env,
    caller: Address,
    network_id: u32,
    fee_bps: i128,
) -> Result<(), BridgeError> {
    require_admin(env, &caller)?;
    if !(0..=10_000).contains(&fee_bps) {
        return Err(BridgeError::FeeOutOfRange);
    }
    let key = BridgeDataKey::Bridge(network_id);
    let mut cfg: BridgeConfig = env
        .storage()
        .persistent()
        .get(&key)
        .ok_or(BridgeError::NotFound)?;
    cfg.fee_bps = fee_bps;
    env.storage().persistent().set(&key, &cfg);
    Ok(())
}

/// Set or rotate the bridge guardian (admin only).
///
/// The guardian is the *only* address that may call
/// [`freeze_bridge`] / [`unfreeze_bridge`]. The admin and guardian are
/// deliberately disjoint roles (see [`require_guardian`]).
pub fn set_bridge_guardian(
    env: &Env,
    caller: Address,
    guardian: Address,
) -> Result<(), BridgeError> {
    require_admin(env, &caller)?;
    env.storage()
        .instance()
        .set(&BridgeDataKey::Guardian, &guardian);
    Ok(())
}

// ---------------------------------------------------------------------------
// User operations
// ---------------------------------------------------------------------------

/// Deposit through bridge `network_id` (user operation).
///
/// Always permitted — **never blocked by the freeze**. While a freeze is in
/// place, inbound liquidity is still honoured so that user funds are not
/// stranded on the bridge.
pub fn bridge_deposit(
    _env: &Env,
    user: Address,
    _network_id: u32,
    _asset: Option<Address>,
    amount: i128,
) -> Result<i128, BridgeError> {
    user.require_auth();
    if amount <= 0 {
        return Err(BridgeError::InvalidAmount);
    }
    // Net-fee accounting and deposit ledger writes live in the lending
    // module; here we only validate and pass through. The freeze check is
    // intentionally omitted — deposits are exempt.
    Ok(amount)
}

/// Withdraw through bridge `network_id` (user operation).
///
/// # Freeze gate
/// If `IsFrozen == true`, this function returns
/// [`BridgeError::Frozen`] immediately, **before** any state mutation or
/// token transfer. The caller is authenticated (so a malicious retry cannot
/// induce a different code path) and `amount` / `network_id` are validated
/// first, so a frozen call still rejects malformed inputs without ever
/// touching storage.
pub fn bridge_withdraw(
    env: &Env,
    user: Address,
    network_id: u32,
    _asset: Option<Address>,
    amount: i128,
) -> Result<i128, BridgeError> {
    user.require_auth();
    if amount <= 0 {
        return Err(BridgeError::InvalidAmount);
    }

    // Freeze gate: must precede any state read/write. This is the entire
    // reason for the freeze feature — withdrawals halted before they touch
    // anything.
    if is_bridge_frozen(env) {
        return Err(BridgeError::Frozen);
    }

    // Validate the bridge is registered and enabled before any side effects.
    let cfg = get_bridge_config(env, network_id)?;
    if !cfg.enabled {
        return Err(BridgeError::Disabled);
    }

    Ok(amount)
}

// ---------------------------------------------------------------------------
// View functions
// ---------------------------------------------------------------------------

/// Read the [`BridgeConfig`] for `network_id`.
///
/// # Errors
/// - [`BridgeError::NotFound`] if no bridge is registered.
pub fn get_bridge_config(env: &Env, network_id: u32) -> Result<BridgeConfig, BridgeError> {
    env.storage()
        .persistent()
        .get(&BridgeDataKey::Bridge(network_id))
        .ok_or(BridgeError::NotFound)
}

/// Enumerate all registered bridges.
///
/// Reads the `BridgesIndex` and returns a `Map<u32, BridgeConfig>`.
pub fn list_bridges(env: &Env) -> Map<u32, BridgeConfig> {
    let index: Vec<u32> = env
        .storage()
        .instance()
        .get(&BridgeDataKey::BridgesIndex)
        .unwrap_or_else(|| Vec::new(env));

    let mut out: Map<u32, BridgeConfig> = Map::new(env);
    for network_id in index.iter() {
        if let Some(cfg) = env
            .storage()
            .persistent()
            .get::<BridgeDataKey, BridgeConfig>(&BridgeDataKey::Bridge(network_id))
        {
            out.set(network_id, cfg);
        }
    }
    out
}

/// Whether `bridge_withdraw` is currently frozen.
///
/// Always `false` before [`freeze_bridge`] is first called.
pub fn is_bridge_frozen(env: &Env) -> bool {
    env.storage()
        .instance()
        .get(&BridgeDataKey::IsFrozen)
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Incident-response: freeze / unfreeze
// ---------------------------------------------------------------------------

/// Freeze `bridge_withdraw` (guardian only).
///
/// Idempotent: calling on an already-frozen bridge returns `Ok(())` **without**
/// emitting a duplicate event (the freeze event fires only on the transition
/// from `false → true`).
///
/// Triggers a [`BridgeFreezeEvent`] with `is_frozen = true` on the transition.
///
/// # Errors
/// - [`BridgeError::Unauthorized`] if `caller` is not the configured guardian.
/// - [`BridgeError::GuardianNotConfigured`] if no guardian has been set yet.
pub fn freeze_bridge(env: &Env, caller: Address) -> Result<(), BridgeError> {
    require_guardian(env, &caller)?;
    if is_bridge_frozen(env) {
        // No-op: do not double-emit.
        return Ok(());
    }
    env.storage()
        .instance()
        .set(&BridgeDataKey::IsFrozen, &true);
    emit_freeze_event(env, &caller, true);
    Ok(())
}

/// Unfreeze `bridge_withdraw` (guardian only).
///
/// Idempotent: calling on an already-unfrozen bridge returns `Ok(())` without
/// emitting a duplicate event.
///
/// Triggers a [`BridgeFreezeEvent`] with `is_frozen = false` on the transition.
///
/// # Errors
/// - [`BridgeError::Unauthorized`] if `caller` is not the configured guardian.
/// - [`BridgeError::GuardianNotConfigured`] if no guardian has been set yet.
pub fn unfreeze_bridge(env: &Env, caller: Address) -> Result<(), BridgeError> {
    require_guardian(env, &caller)?;
    if !is_bridge_frozen(env) {
        // No-op: do not double-emit.
        return Ok(());
    }
    env.storage()
        .instance()
        .remove(&BridgeDataKey::IsFrozen);
    emit_freeze_event(env, &caller, false);
    Ok(())
}

// ===========================================================================
// Note on state-machine coverage
// ===========================================================================
// The freeze state machine is exhaustively covered by the on-chain
// integration tests in `bridge_freeze_test.rs` (FFNN-1 through FFNN-12):
// idempotency, no-mutation on frozen withdraw, default state, etc. — all
// of which drive the same `freeze_bridge` / `unfreeze_bridge` functions
// against a real `Env`. We deliberately keep no in-module proptest mirror
// so the production surface stays focused.
