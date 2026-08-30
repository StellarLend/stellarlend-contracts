//! # Lending Lifecycle State Transitions
//!
//! Deposit, withdraw, borrow, repay and liquidate are the five transitions that
//! move value between a borrower's position and the protocol treasury. This
//! module makes the rules that govern those transitions *explicit, pure and
//! testable*, and pairs them with a **bounded** diagnostics ring buffer so that
//! degraded behaviour (rejections, throttling, slow settlement) is observable
//! by operators without adding unbounded storage, unbounded reads, or leaking
//! borrower identities.
//!
//! ## 1. Invariants
//!
//! Every transition is evaluated against four invariant families before any
//! state is written. They are encoded once, here, so the entrypoints in
//! `lib.rs` cannot drift apart from each other.
//!
//! ### State invariants (`S`)
//! * `S1` — A position is always well formed: `collateral >= 0 && debt >= 0`.
//! * `S2` — `Withdraw` may not reduce collateral below zero.
//! * `S3` — `Repay` may not reduce debt below zero (overpayment is rejected,
//!   not silently clamped, so the caller learns the true settlement amount).
//! * `S4` — `Liquidate` repays debt *and* seizes collateral, so it requires a
//!   position with both `debt > 0` and `collateral > 0`.
//! * `S5` — A rejected transition is a no-op: the caller receives the reason
//!   and the *unmodified* snapshot.
//!
//! ### Data invariants (`D`)
//! * `D1` — `amount > 0`. Zero-amount transitions are rejected rather than
//!   accepted as no-ops, so telemetry never counts phantom activity.
//! * `D2` — `amount <= MAX_TRANSITION_AMOUNT`. A hard magnitude bound keeps
//!   every subsequent multiplication (rates, bonuses, bps) inside `i128`
//!   without relying on the caller to be honest.
//! * `D3` — Additive updates use checked arithmetic; overflow is a rejection,
//!   never a wrap.
//! * `D4` — Conservation: the applied delta equals exactly the requested
//!   amount on the affected leg, and the untouched leg is bit-identical.
//!   [`verify_post`] re-checks this *after* application.
//!
//! ### Authorization invariants (`A`)
//! * `A1` — Every transition requires an authorized caller.
//! * `A2` — `Deposit`, `Withdraw`, `Borrow` and `Repay` are owner-only.
//! * `A3` — `Liquidate` is third-party only: an owner may not liquidate their
//!   own position.
//!
//! ### Failure invariants (`F`)
//! * `F1` — Rejections are *classified* ([`FailureClass`]) so dashboards can
//!   separate user error from protocol degradation.
//! * `F2` — Rejection reasons are stable, non-overlapping codes; they are safe
//!   to expose because they carry no address, balance or price.
//! * `F3` — A retry of a previously rejected transition is counted, so a
//!   client stuck in a retry loop is visible without log scraping.
//!
//! ## 2. Bounds
//!
//! On-chain analogues of the usual client-side budget knobs. Every one of them
//! is a compile-time constant so the worst case is auditable:
//!
//! | Concern | Constant | Value |
//! |---|---|---|
//! | Memory / stored history | [`MAX_LIFECYCLE_RECORDS`] | 64 records |
//! | "Pagination" / read fan-out | [`MAX_LIFECYCLE_PAGE`] | 16 records per call |
//! | Concurrent writes per actor | [`MAX_TRANSITIONS_PER_LEDGER`] | 8 per ledger |
//! | "Upload size" / value magnitude | [`MAX_TRANSITION_AMOUNT`] | `i128::MAX / 4` |
//! | Retry budget before escalation | [`MAX_RETRY_ATTEMPTS`] | 3 |
//! | Latency series points | [`MAX_LATENCY_BUCKETS`] | 4 buckets |
//!
//! ## 3. Redundant work avoidance
//!
//! Rapid interaction (a user double-submitting, an indexer replaying, a
//! reconnecting client re-sending) must not multiply storage writes:
//!
//! * A record identical to the newest one *within the same ledger* is folded
//!   into that record's `repeat_count` instead of appending a new entry.
//! * Counters and per-actor windows are only written back when at least one
//!   field actually changed.
//! * Per-ledger actor activity is capped by [`MAX_TRANSITIONS_PER_LEDGER`];
//!   beyond that the attempt is reported as [`Outcome::Throttled`] and costs
//!   one counter bump rather than a history append.
//!
//! ## 4. Diagnostics without secrets
//!
//! Records store an `actor_tag: u32` — the leading four bytes of
//! `sha256(actor_xdr)`. It is stable enough to correlate a session and
//! non-reversible, so history reads never disclose which account transacted.
//! The counters carry no amounts at all; the bounded record ring carries only
//! the requested amount, which is already public via the paired lifecycle
//! event.

use soroban_sdk::{contracttype, xdr::ToXdr, Address, Bytes, BytesN, Env, Vec};

// ═══════════════════════════════════════════════════════════════════════════
// Bounds
// ═══════════════════════════════════════════════════════════════════════════

/// Maximum number of transition records retained on-chain. Oldest entries are
/// evicted first, so storage for the ring is O(1) regardless of protocol age.
pub const MAX_LIFECYCLE_RECORDS: u32 = 64;

/// Maximum number of records returnable from a single paginated read. Callers
/// asking for more are silently clamped rather than rejected, so a naive
/// client cannot force an unbounded response.
pub const MAX_LIFECYCLE_PAGE: u32 = 16;

/// Maximum number of recorded transitions a single actor may append within one
/// ledger. Further attempts in the same ledger are throttled: they still bump
/// counters but do not grow the ring.
pub const MAX_TRANSITIONS_PER_LEDGER: u32 = 8;

/// Hard upper bound on any single transition amount. Chosen as `i128::MAX / 4`
/// so that a bps-scaled product and a paired addition both stay representable.
pub const MAX_TRANSITION_AMOUNT: i128 = i128::MAX / 4;

/// Number of consecutive rejections by the same actor after which the attempt
/// is counted as `escalated` in [`LifecycleCounters`].
pub const MAX_RETRY_ATTEMPTS: u32 = 3;

/// Number of latency histogram buckets exposed by the diagnostics view.
/// Deliberately tiny: a fixed four-point series is enough to spot settlement
/// degradation and costs a constant amount of storage.
pub const MAX_LATENCY_BUCKETS: u32 = 4;

/// Upper edges (in seconds) of the first three latency buckets. Anything above
/// the last edge lands in the trailing overflow bucket.
pub const LATENCY_BUCKET_EDGES_SECS: [u64; 3] = [5, 30, 300];

/// Persistent-entry TTL for lifecycle telemetry, in ledgers.
const LIFECYCLE_TTL_LEDGERS: u32 = 1_000_000;

// ═══════════════════════════════════════════════════════════════════════════
// Types
// ═══════════════════════════════════════════════════════════════════════════

/// The five value-moving transitions of the lending lifecycle.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LifecycleAction {
    /// Supply collateral to the protocol.
    Deposit,
    /// Remove previously supplied collateral.
    Withdraw,
    /// Draw debt against supplied collateral.
    Borrow,
    /// Settle outstanding debt.
    Repay,
    /// Third-party settlement of an unhealthy position.
    Liquidate,
}

impl LifecycleAction {
    /// Whether the transition must be initiated by the position owner (`A2`).
    pub fn requires_owner(&self) -> bool {
        !matches!(self, LifecycleAction::Liquidate)
    }

    /// Whether the transition must be initiated by someone other than the
    /// position owner (`A3`).
    pub fn forbids_owner(&self) -> bool {
        matches!(self, LifecycleAction::Liquidate)
    }

    /// Stable discriminant used in records. Kept explicit so the stored
    /// encoding does not shift if the variants are ever reordered.
    pub fn code(&self) -> u32 {
        match self {
            LifecycleAction::Deposit => 1,
            LifecycleAction::Withdraw => 2,
            LifecycleAction::Borrow => 3,
            LifecycleAction::Repay => 4,
            LifecycleAction::Liquidate => 5,
        }
    }
}

/// The accounting legs a transition may touch, captured before and after.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PositionSnapshot {
    /// Collateral supplied by the position owner.
    pub collateral: i128,
    /// Debt principal owed by the position owner.
    pub debt: i128,
}

impl PositionSnapshot {
    /// Construct a snapshot.
    pub fn new(collateral: i128, debt: i128) -> Self {
        PositionSnapshot { collateral, debt }
    }

    /// `S1`: both legs are non-negative.
    pub fn is_well_formed(&self) -> bool {
        self.collateral >= 0 && self.debt >= 0
    }
}

/// Why a transition was refused. Codes are stable and carry no borrower data
/// (`F2`), so they are safe to surface in client telemetry verbatim.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RejectReason {
    /// `D1` — amount was zero or negative.
    NonPositiveAmount,
    /// `D2` — amount exceeded [`MAX_TRANSITION_AMOUNT`].
    AmountAboveBound,
    /// `S1` — the stored position was malformed (negative leg).
    MalformedPosition,
    /// `A1` — caller was not authorized.
    Unauthorized,
    /// `A2` — an owner-only action was attempted by a third party.
    NotPositionOwner,
    /// `A3` — an owner attempted to liquidate their own position.
    SelfLiquidation,
    /// `S2` — withdrawal exceeded available collateral.
    InsufficientCollateral,
    /// `S3` — repayment exceeded outstanding debt.
    RepayExceedsDebt,
    /// `S4` — liquidation targeted a position with no debt or no collateral.
    NothingToLiquidate,
    /// `D3` — the resulting balance would overflow `i128`.
    Overflow,
    /// `D4` — post-application conservation check failed.
    ConservationViolated,
}

impl RejectReason {
    /// Stable numeric code for indexers and dashboards.
    pub fn code(&self) -> u32 {
        match self {
            RejectReason::NonPositiveAmount => 1,
            RejectReason::AmountAboveBound => 2,
            RejectReason::MalformedPosition => 3,
            RejectReason::Unauthorized => 4,
            RejectReason::NotPositionOwner => 5,
            RejectReason::SelfLiquidation => 6,
            RejectReason::InsufficientCollateral => 7,
            RejectReason::RepayExceedsDebt => 8,
            RejectReason::NothingToLiquidate => 9,
            RejectReason::Overflow => 10,
            RejectReason::ConservationViolated => 11,
        }
    }

    /// Coarse class used to split operator dashboards between "the caller got
    /// it wrong" and "the protocol is degraded" (`F1`).
    pub fn class(&self) -> FailureClass {
        match self {
            RejectReason::NonPositiveAmount
            | RejectReason::AmountAboveBound
            | RejectReason::MalformedPosition => FailureClass::Validation,
            RejectReason::Unauthorized
            | RejectReason::NotPositionOwner
            | RejectReason::SelfLiquidation => FailureClass::Authorization,
            RejectReason::InsufficientCollateral
            | RejectReason::RepayExceedsDebt
            | RejectReason::NothingToLiquidate => FailureClass::Accounting,
            RejectReason::Overflow | RejectReason::ConservationViolated => FailureClass::Internal,
        }
    }
}

/// Coarse failure taxonomy exposed in diagnostics.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureClass {
    /// Malformed request: bad amount, or a corrupt stored position.
    Validation,
    /// Caller lacked the right to perform the transition.
    Authorization,
    /// Request was well formed but inconsistent with the position's balances.
    Accounting,
    /// Per-ledger activity budget exhausted.
    Throttle,
    /// Arithmetic or conservation failure — always operator-actionable.
    Internal,
}

impl FailureClass {
    /// Stable numeric code.
    pub fn code(&self) -> u32 {
        match self {
            FailureClass::Validation => 1,
            FailureClass::Authorization => 2,
            FailureClass::Accounting => 3,
            FailureClass::Throttle => 4,
            FailureClass::Internal => 5,
        }
    }
}

/// Terminal state of an observed transition attempt.
#[contracttype]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Guards passed and the new snapshot was produced.
    Committed,
    /// Guards refused the transition; the position is unchanged.
    Rejected,
    /// Per-ledger recording budget was exhausted; the attempt was not
    /// appended to the ring.
    Throttled,
}

/// A single evaluated transition request. Kept free of `Env` so the whole
/// guard suite is a pure function of its inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransitionRequest {
    /// Which lifecycle transition is being attempted.
    pub action: LifecycleAction,
    /// Magnitude of the transition, in the asset's raw units.
    pub amount: i128,
    /// Whether the caller produced a valid authorization for this call.
    pub authorized: bool,
    /// Whether the caller owns the position being modified.
    pub caller_is_owner: bool,
}

impl TransitionRequest {
    /// Build a request.
    pub fn new(
        action: LifecycleAction,
        amount: i128,
        authorized: bool,
        caller_is_owner: bool,
    ) -> Self {
        TransitionRequest {
            action,
            amount,
            authorized,
            caller_is_owner,
        }
    }
}

/// One bounded-ring diagnostics entry.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TransitionRecord {
    /// Non-reversible correlation tag for the caller — never the address.
    pub actor_tag: u32,
    /// [`LifecycleAction::code`] of the attempted transition.
    pub action: u32,
    /// Terminal state of the attempt.
    pub outcome: Outcome,
    /// [`RejectReason::code`], or `0` when the transition committed.
    pub reason: u32,
    /// Requested amount. Already public via the paired lifecycle event.
    pub amount: i128,
    /// Ledger sequence at which the attempt was observed.
    pub ledger: u32,
    /// Ledger timestamp at which the attempt was observed.
    pub timestamp: u64,
    /// Seconds between this attempt and the actor's previous attempt. `0` for
    /// a first attempt. This is the inter-arrival / settlement latency signal.
    pub latency_secs: u64,
    /// How many identical attempts within this ledger folded into this record.
    /// `1` means "seen once"; higher values mean a client is re-submitting.
    pub repeat_count: u32,
}

/// Aggregate lifecycle telemetry. Every field saturates rather than wrapping.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleCounters {
    /// Attempts evaluated, including rejected and throttled ones.
    pub attempted: u64,
    /// Attempts whose guards passed.
    pub committed: u64,
    /// Attempts refused by a guard.
    pub rejected: u64,
    /// Attempts refused because the per-ledger recording budget was exhausted.
    pub throttled: u64,
    /// Duplicate submissions folded into an existing record.
    pub deduplicated: u64,
    /// Commits that directly followed a rejection by the same actor — the
    /// recovery signal.
    pub recovered: u64,
    /// Rejections at or beyond [`MAX_RETRY_ATTEMPTS`] consecutive failures.
    pub escalated: u64,
    /// Class code of the most recent failure, `0` when none.
    pub last_failure_class: u32,
    /// Reason code of the most recent failure, `0` when none or throttled.
    pub last_failure_reason: u32,
    /// Ledger sequence of the most recent failure, `0` when none.
    pub last_failure_ledger: u32,
    /// Latency histogram, [`MAX_LATENCY_BUCKETS`] wide, with edges given by
    /// [`LATENCY_BUCKET_EDGES_SECS`] plus a trailing overflow bucket.
    pub latency_buckets: Vec<u64>,
    /// Largest observed inter-attempt latency, in seconds.
    pub max_latency_secs: u64,
}

impl LifecycleCounters {
    /// Zeroed counters with an allocated, correctly sized histogram.
    pub fn empty(env: &Env) -> Self {
        let mut latency_buckets = Vec::new(env);
        for _ in 0..MAX_LATENCY_BUCKETS {
            latency_buckets.push_back(0u64);
        }
        LifecycleCounters {
            attempted: 0,
            committed: 0,
            rejected: 0,
            throttled: 0,
            deduplicated: 0,
            recovered: 0,
            escalated: 0,
            last_failure_class: 0,
            last_failure_reason: 0,
            last_failure_ledger: 0,
            latency_buckets,
            max_latency_secs: 0,
        }
    }
}

/// Read-only operator view combining counters with the ring's occupancy and
/// the bounds that produced them, so a dashboard can render "how close are we
/// to the cap" without hard-coding the constants.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LifecycleDiagnostics {
    /// Aggregate counters.
    pub counters: LifecycleCounters,
    /// Records currently retained in the ring.
    pub records_retained: u32,
    /// Ring capacity ([`MAX_LIFECYCLE_RECORDS`]).
    pub records_capacity: u32,
    /// Maximum page size accepted by [`read_records`] ([`MAX_LIFECYCLE_PAGE`]).
    pub max_page_size: u32,
    /// Per-actor per-ledger recording budget ([`MAX_TRANSITIONS_PER_LEDGER`]).
    pub max_transitions_per_ledger: u32,
    /// Consecutive-rejection budget ([`MAX_RETRY_ATTEMPTS`]).
    pub max_retry_attempts: u32,
}

/// Outcome of a pre-flight simulation.
///
/// Modelled as a plain struct rather than a `Result` so it can cross the
/// contract boundary: a client reads `allowed` first, then either `after` or
/// `reason`/`class`.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SimulationResult {
    /// Whether the guards would let this transition through.
    pub allowed: bool,
    /// The snapshot the transition would produce. Equal to the input snapshot
    /// when `allowed` is false, restating `S5`: a refusal changes nothing.
    pub after: PositionSnapshot,
    /// [`RejectReason::code`] when refused, `0` when allowed.
    pub reason: u32,
    /// [`FailureClass::code`] when refused, `0` when allowed.
    pub class: u32,
}

/// Per-actor bookkeeping backing throttling, retry and recovery detection.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActorWindow {
    /// Ledger the counts below apply to.
    pub ledger: u32,
    /// Records appended by this actor within `ledger`.
    pub count: u32,
    /// Timestamp of this actor's previous attempt, for latency deltas.
    pub last_seen: u64,
    /// Consecutive rejections since this actor's last commit.
    pub consecutive_rejections: u32,
}

/// Storage keys owned by this module. Namespaced separately from
/// `crate::DataKey` so lifecycle telemetry can never collide with accounting.
#[contracttype]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecycleKey {
    /// Bounded ring of [`TransitionRecord`], stored oldest-first.
    Records,
    /// Aggregate [`LifecycleCounters`].
    Counters,
    /// Per-actor [`ActorWindow`], keyed by the non-reversible actor tag.
    Actor(u32),
}

// ═══════════════════════════════════════════════════════════════════════════
// Pure guard evaluation — invariants S / D / A
// ═══════════════════════════════════════════════════════════════════════════

/// Evaluate a transition against every state, data and authorization
/// invariant and return the snapshot it would produce.
///
/// This is a pure function: no storage access, no logging, no panics. Callers
/// may therefore use it both as a pre-flight simulation and as the
/// authoritative guard in the write path with no behavioural divergence
/// between the two.
///
/// On rejection the caller keeps `before` untouched, satisfying `S5`.
pub fn evaluate(
    before: &PositionSnapshot,
    request: &TransitionRequest,
) -> Result<PositionSnapshot, RejectReason> {
    // `S1` — never trust a stored position that is already corrupt.
    if !before.is_well_formed() {
        return Err(RejectReason::MalformedPosition);
    }

    // `A1`/`A2`/`A3` — authorization is settled before any accounting so an
    // unauthorized caller cannot probe balances through the error codes.
    if !request.authorized {
        return Err(RejectReason::Unauthorized);
    }
    if request.action.requires_owner() && !request.caller_is_owner {
        return Err(RejectReason::NotPositionOwner);
    }
    if request.action.forbids_owner() && request.caller_is_owner {
        return Err(RejectReason::SelfLiquidation);
    }

    // `D1`/`D2` — bound the magnitude before it reaches any arithmetic.
    if request.amount <= 0 {
        return Err(RejectReason::NonPositiveAmount);
    }
    if request.amount > MAX_TRANSITION_AMOUNT {
        return Err(RejectReason::AmountAboveBound);
    }

    let amount = request.amount;
    let after = match request.action {
        LifecycleAction::Deposit => PositionSnapshot::new(
            // `D3`
            before
                .collateral
                .checked_add(amount)
                .ok_or(RejectReason::Overflow)?,
            before.debt,
        ),
        LifecycleAction::Withdraw => {
            // `S2`
            if amount > before.collateral {
                return Err(RejectReason::InsufficientCollateral);
            }
            PositionSnapshot::new(before.collateral - amount, before.debt)
        }
        LifecycleAction::Borrow => PositionSnapshot::new(
            before.collateral,
            // `D3`
            before
                .debt
                .checked_add(amount)
                .ok_or(RejectReason::Overflow)?,
        ),
        LifecycleAction::Repay => {
            // `S3` — reject rather than clamp, so the caller learns the exact
            // settlement amount instead of over-transferring.
            if amount > before.debt {
                return Err(RejectReason::RepayExceedsDebt);
            }
            PositionSnapshot::new(before.collateral, before.debt - amount)
        }
        LifecycleAction::Liquidate => {
            // `S4`
            if before.debt <= 0 || before.collateral <= 0 {
                return Err(RejectReason::NothingToLiquidate);
            }
            if amount > before.debt {
                return Err(RejectReason::RepayExceedsDebt);
            }
            // The liquidation bonus is settled by the caller's own incentive
            // calculation; this guard enforces the debt leg and that the
            // base seizure alone cannot drive collateral negative.
            if amount > before.collateral {
                return Err(RejectReason::InsufficientCollateral);
            }
            PositionSnapshot::new(before.collateral - amount, before.debt - amount)
        }
    };

    // `S1` again on the produced snapshot — defence in depth against a future
    // match arm forgetting its own bound.
    if !after.is_well_formed() {
        return Err(RejectReason::ConservationViolated);
    }

    Ok(after)
}

/// Pre-flight a transition without touching state.
///
/// Wraps [`evaluate`] in a boundary-crossable shape. Because both share the
/// same guard body, a simulation and the real call can never disagree — which
/// is what makes it safe for a client to skip a transaction it already knows
/// will be refused.
pub fn simulate(before: &PositionSnapshot, request: &TransitionRequest) -> SimulationResult {
    match evaluate(before, request) {
        Ok(after) => SimulationResult {
            allowed: true,
            after,
            reason: 0,
            class: 0,
        },
        Err(reason) => SimulationResult {
            allowed: false,
            // `S5`: a refusal leaves the position exactly as it was.
            after: *before,
            reason: reason.code(),
            class: reason.class().code(),
        },
    }
}

/// Re-check `D4` conservation on an already-applied transition.
///
/// Callers that mutate storage themselves — rather than adopting [`evaluate`]'s
/// returned snapshot wholesale — run this against the reloaded position to
/// prove the write did exactly what the guard authorized: no more, no less.
pub fn verify_post(
    before: &PositionSnapshot,
    after: &PositionSnapshot,
    request: &TransitionRequest,
) -> Result<(), RejectReason> {
    let expected = evaluate(before, request)?;
    if expected == *after {
        Ok(())
    } else {
        Err(RejectReason::ConservationViolated)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Actor tagging — correlation without disclosure
// ═══════════════════════════════════════════════════════════════════════════

/// Derive a stable, non-reversible correlation tag for an address.
///
/// The tag is the leading four bytes of `sha256(address_xdr)`. It lets an
/// operator group one session's attempts together without the diagnostics
/// history disclosing which account produced them.
pub fn actor_tag(env: &Env, actor: &Address) -> u32 {
    let mut data = Bytes::new(env);
    data.append(&actor.clone().to_xdr(env));
    let digest: BytesN<32> = env.crypto().sha256(&data).into();
    let bytes = digest.to_array();
    u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

// ═══════════════════════════════════════════════════════════════════════════
// Bounded storage — ring buffer, counters, per-actor windows
// ═══════════════════════════════════════════════════════════════════════════

fn extend(env: &Env, key: &LifecycleKey) {
    let extend_to = env.storage().max_ttl().min(LIFECYCLE_TTL_LEDGERS);
    let threshold = extend_to / 2 + 1;
    if env.storage().persistent().has(key) {
        env.storage()
            .persistent()
            .extend_ttl(key, threshold, extend_to);
    }
}

/// Load the record ring in storage order (oldest first).
pub fn load_records(env: &Env) -> Vec<TransitionRecord> {
    env.storage()
        .persistent()
        .get(&LifecycleKey::Records)
        .unwrap_or_else(|| Vec::new(env))
}

/// Load aggregate counters, or a zeroed set when none have been written.
pub fn load_counters(env: &Env) -> LifecycleCounters {
    env.storage()
        .persistent()
        .get(&LifecycleKey::Counters)
        .unwrap_or_else(|| LifecycleCounters::empty(env))
}

/// Persist counters, but only when they differ from what is already stored.
///
/// Returns whether a write actually happened. Rapid interaction frequently
/// produces byte-identical counter states; comparing first turns those into
/// pure reads.
fn store_counters_if_changed(env: &Env, next: &LifecycleCounters) -> bool {
    let current: Option<LifecycleCounters> = env.storage().persistent().get(&LifecycleKey::Counters);
    if current.as_ref() == Some(next) {
        return false;
    }
    env.storage().persistent().set(&LifecycleKey::Counters, next);
    extend(env, &LifecycleKey::Counters);
    true
}

/// Load an actor's window, defaulting to a zeroed one.
pub fn load_actor_window(env: &Env, tag: u32) -> ActorWindow {
    env.storage()
        .persistent()
        .get(&LifecycleKey::Actor(tag))
        .unwrap_or(ActorWindow {
            ledger: 0,
            count: 0,
            last_seen: 0,
            consecutive_rejections: 0,
        })
}

fn store_actor_window_if_changed(env: &Env, tag: u32, window: &ActorWindow) -> bool {
    let key = LifecycleKey::Actor(tag);
    let current: Option<ActorWindow> = env.storage().persistent().get(&key);
    if current.as_ref() == Some(window) {
        return false;
    }
    env.storage().persistent().set(&key, window);
    extend(env, &key);
    true
}

/// Index of the latency bucket a given inter-arrival delay falls into.
pub fn bucket_for(latency_secs: u64) -> u32 {
    let mut index: u32 = 0;
    while (index as usize) < LATENCY_BUCKET_EDGES_SECS.len() {
        if latency_secs <= LATENCY_BUCKET_EDGES_SECS[index as usize] {
            return index;
        }
        index += 1;
    }
    MAX_LATENCY_BUCKETS - 1
}

fn bump_bucket(counters: &mut LifecycleCounters, latency_secs: u64) {
    let index = bucket_for(latency_secs);
    let current = counters.latency_buckets.get(index).unwrap_or(0);
    counters.latency_buckets.set(index, current.saturating_add(1));
    if latency_secs > counters.max_latency_secs {
        counters.max_latency_secs = latency_secs;
    }
}

/// Observe a transition attempt: update the bounded ring, the per-actor
/// window and the aggregate counters.
///
/// Returns the [`Outcome`] actually recorded, which is [`Outcome::Throttled`]
/// when the actor has exhausted [`MAX_TRANSITIONS_PER_LEDGER`] for the current
/// ledger — even if `result` itself was a commit. Throttling bounds the
/// *diagnostics ring*, never the money path, so callers must not turn a
/// throttled outcome into a transaction failure.
///
/// This function never panics: telemetry must not be able to break a
/// transition that the guards already approved.
pub fn observe(
    env: &Env,
    actor: &Address,
    request: &TransitionRequest,
    result: &Result<PositionSnapshot, RejectReason>,
) -> Outcome {
    let tag = actor_tag(env, actor);
    let ledger = env.ledger().sequence();
    let now = env.ledger().timestamp();

    let mut window = load_actor_window(env, tag);
    if window.ledger != ledger {
        // New ledger: the per-ledger recording budget resets. The retry streak
        // deliberately does not — a client retrying across ledgers is exactly
        // what `MAX_RETRY_ATTEMPTS` exists to surface.
        window.ledger = ledger;
        window.count = 0;
    }

    // Monotonic-clock defence: a replayed or reordered ledger must not produce
    // a negative (wrapping) latency.
    let latency_secs = if window.last_seen == 0 || now < window.last_seen {
        0
    } else {
        now - window.last_seen
    };

    let mut counters = load_counters(env);
    counters.attempted = counters.attempted.saturating_add(1);

    let outcome = if window.count >= MAX_TRANSITIONS_PER_LEDGER {
        counters.throttled = counters.throttled.saturating_add(1);
        counters.last_failure_class = FailureClass::Throttle.code();
        counters.last_failure_reason = 0;
        counters.last_failure_ledger = ledger;
        Outcome::Throttled
    } else {
        match result {
            Ok(_) => {
                counters.committed = counters.committed.saturating_add(1);
                if window.consecutive_rejections > 0 {
                    counters.recovered = counters.recovered.saturating_add(1);
                }
                window.consecutive_rejections = 0;
                Outcome::Committed
            }
            Err(reason) => {
                counters.rejected = counters.rejected.saturating_add(1);
                counters.last_failure_class = reason.class().code();
                counters.last_failure_reason = reason.code();
                counters.last_failure_ledger = ledger;
                window.consecutive_rejections = window.consecutive_rejections.saturating_add(1);
                if window.consecutive_rejections >= MAX_RETRY_ATTEMPTS {
                    counters.escalated = counters.escalated.saturating_add(1);
                }
                Outcome::Rejected
            }
        }
    };

    bump_bucket(&mut counters, latency_secs);

    if outcome != Outcome::Throttled {
        let reason_code = match result {
            Ok(_) => 0,
            Err(reason) => reason.code(),
        };
        let record = TransitionRecord {
            actor_tag: tag,
            action: request.action.code(),
            outcome,
            reason: reason_code,
            amount: request.amount,
            ledger,
            timestamp: now,
            latency_secs,
            repeat_count: 1,
        };
        if push_record(env, record) {
            window.count = window.count.saturating_add(1);
        } else {
            counters.deduplicated = counters.deduplicated.saturating_add(1);
        }
    }

    window.last_seen = now;
    store_actor_window_if_changed(env, tag, &window);
    store_counters_if_changed(env, &counters);

    outcome
}

/// Append a record to the bounded ring, folding same-ledger duplicates.
///
/// Returns `true` when a new entry was appended and `false` when the record
/// was folded into the existing newest entry — the redundant-write path.
fn push_record(env: &Env, record: TransitionRecord) -> bool {
    let mut records = load_records(env);

    if let Some(last) = records.last() {
        let last: TransitionRecord = last;
        if last.ledger == record.ledger
            && last.actor_tag == record.actor_tag
            && last.action == record.action
            && last.amount == record.amount
            && last.outcome == record.outcome
            && last.reason == record.reason
        {
            let index = records.len() - 1;
            let mut folded = last;
            folded.repeat_count = folded.repeat_count.saturating_add(1);
            records.set(index, folded);
            env.storage()
                .persistent()
                .set(&LifecycleKey::Records, &records);
            extend(env, &LifecycleKey::Records);
            return false;
        }
    }

    // Evict oldest-first so the ring never exceeds its declared capacity.
    while records.len() >= MAX_LIFECYCLE_RECORDS {
        records.remove(0);
    }
    records.push_back(record);

    env.storage()
        .persistent()
        .set(&LifecycleKey::Records, &records);
    extend(env, &LifecycleKey::Records);
    true
}

// ═══════════════════════════════════════════════════════════════════════════
// Bounded reads
// ═══════════════════════════════════════════════════════════════════════════

/// Read a page of transition records, newest first.
///
/// `offset` counts back from the newest record. `limit` is clamped to
/// [`MAX_LIFECYCLE_PAGE`] rather than rejected, so a client that asks for
/// everything receives a bounded response instead of an error. An `offset`
/// at or past the end yields an empty page, which is the natural termination
/// condition for a paging client.
pub fn read_records(env: &Env, offset: u32, limit: u32) -> Vec<TransitionRecord> {
    let records = load_records(env);
    let total = records.len();
    let mut page = Vec::new(env);

    if offset >= total {
        return page;
    }

    let effective_limit = if limit > MAX_LIFECYCLE_PAGE {
        MAX_LIFECYCLE_PAGE
    } else {
        limit
    };

    // Storage is oldest-first; walk backwards from `total - offset`.
    let mut cursor = total - offset;
    let mut taken = 0u32;
    while cursor > 0 && taken < effective_limit {
        cursor -= 1;
        if let Some(record) = records.get(cursor) {
            page.push_back(record);
            taken += 1;
        }
    }

    page
}

/// Build the operator diagnostics view.
pub fn diagnostics(env: &Env) -> LifecycleDiagnostics {
    LifecycleDiagnostics {
        counters: load_counters(env),
        records_retained: load_records(env).len(),
        records_capacity: MAX_LIFECYCLE_RECORDS,
        max_page_size: MAX_LIFECYCLE_PAGE,
        max_transitions_per_ledger: MAX_TRANSITIONS_PER_LEDGER,
        max_retry_attempts: MAX_RETRY_ATTEMPTS,
    }
}

/// Guard + observe in one call: the shape every lifecycle entrypoint should
/// use.
///
/// Runs [`evaluate`], records the attempt through [`observe`], and returns the
/// guard's verdict unchanged. Telemetry is best-effort and never alters the
/// verdict.
pub fn guard(
    env: &Env,
    actor: &Address,
    before: &PositionSnapshot,
    request: &TransitionRequest,
) -> Result<PositionSnapshot, RejectReason> {
    let result = evaluate(before, request);
    observe(env, actor, request, &result);
    result
}
