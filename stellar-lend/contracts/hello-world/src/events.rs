/// events.rs — Structured event definitions and emit helpers for the StellarLend
/// hello-world contract.
///
/// # Schema versioning
///
/// A single [`EVENT_SCHEMA_VERSION`] constant is the source of truth for the
/// active schema version.  Versioned event structs carry a `schema_version: u32`
/// field populated with this constant at emit time.  See
/// `docs/EVENT_SCHEMA_VERSIONING.md` for the full upgrade policy.
///
/// # Adding a new event
///
/// 1. Define a struct with `#[contracttype]` below.
/// 2. Write a `pub fn emit_<name>(env: &Env, event: <Struct>)` helper that calls
///    [`publish_event`].
/// 3. Call the helper from the business-logic site.
/// 4. Add a row to the versioned-events table in `docs/EVENT_SCHEMA_VERSIONING.md`.
use soroban_sdk::{contracttype, symbol_short, Address, Env, Symbol};

// ---------------------------------------------------------------------------
// Schema version
// ---------------------------------------------------------------------------

/// Single source of truth for the active event schema version.
///
/// Increment this constant whenever a **breaking** change is made to a versioned
/// event (field added, removed, or type-changed).  See
/// `docs/EVENT_SCHEMA_VERSIONING.md` for the full procedure.
pub const EVENT_SCHEMA_VERSION: u32 = 1;

// ---------------------------------------------------------------------------
// Internal publish helper
// ---------------------------------------------------------------------------

/// Publish a raw `(topics, data)` event pair.
///
/// All emit helpers funnel through here so topic construction is consistent.
fn publish_event<T: soroban_sdk::IntoVal<Env, soroban_sdk::Val>>(
    env: &Env,
    topics: impl soroban_sdk::IntoVal<Env, soroban_sdk::Val>,
    data: T,
) {
    env.events().publish(topics, data);
}

// ---------------------------------------------------------------------------
// PriceUpdatedEvent
// ---------------------------------------------------------------------------

/// Emitted whenever a price feed entry is successfully written by
/// [`oracle::update_price_feed`].
///
/// This is an **unversioned** event: new optional fields may be appended across
/// upgrades but existing fields will not be removed or reordered.
#[contracttype]
#[derive(Clone, Debug)]
pub struct PriceUpdatedEvent {
    /// Caller that submitted the price update.
    pub actor: Address,
    /// Asset whose price was updated.
    pub asset: Address,
    /// New price value (raw oracle units).
    pub price: i128,
    /// Decimal precision of `price`.
    pub decimals: u32,
    /// Oracle contract address that signed / submitted the price.
    pub oracle: Address,
    /// Ledger timestamp at which the update was written.
    pub timestamp: u64,
}

/// Emit a [`PriceUpdatedEvent`].
///
/// Topics: `("oracle", "price_updated")`
pub fn emit_price_updated(env: &Env, event: PriceUpdatedEvent) {
    env.events()
        .publish((symbol_short!("oracle"), symbol_short!("priceUpd")), event);
}

// ---------------------------------------------------------------------------
// TwapFallbackUsedEvent  (versioned, schema_version = 1)
// ---------------------------------------------------------------------------

/// Emitted by [`oracle::try_twap_fallback`] each time the oracle resolution
/// path falls back to the AMM TWAP price instead of the primary feed.
///
/// # Semantics
///
/// This event fires **only when the fallback is actually used** — it is never
/// emitted on the primary-feed happy path.  Its presence in the event stream
/// is an unambiguous signal that:
///
/// - The primary oracle feed for `asset` was either absent or stale
///   (age > `max_staleness_seconds`).
/// - The resolved price for this call is derived from AMM pool reserves
///   over a [`TWAP_FALLBACK_WINDOW_SECS`]-second window.
///
/// # Versioning
///
/// This is a **versioned** event (`schema_version` field present).  Any
/// future breaking change must follow the bump-and-dual-emit procedure in
/// `docs/EVENT_SCHEMA_VERSIONING.md`.
///
/// [`TWAP_FALLBACK_WINDOW_SECS`]: crate::oracle::TWAP_FALLBACK_WINDOW_SECS
#[contracttype]
#[derive(Clone, Debug)]
pub struct TwapFallbackUsedEvent {
    /// Schema version — always [`EVENT_SCHEMA_VERSION`] at emit time.
    /// Indexers must read this field before decoding the rest of the payload.
    pub schema_version: u32,
    /// Asset for which the TWAP fallback was used.
    pub asset: Address,
    /// TWAP price resolved for this call, scaled by `PRICE_SCALE` (1 × 10^18).
    /// Divide by 10^18 to obtain the human-readable price.
    pub twap_price: u128,
    /// Age in seconds of the primary feed at the time the fallback fired.
    ///
    /// Set to `u64::MAX` when the primary feed record was absent entirely
    /// (as opposed to present but stale).  Indexers should treat `u64::MAX`
    /// as "feed missing" rather than an actual age measurement.
    pub primary_age_secs: u64,
}

/// Sentinel value for [`TwapFallbackUsedEvent::primary_age_secs`] indicating
/// the primary feed record was absent (not merely stale).
pub const PRIMARY_FEED_ABSENT: u64 = u64::MAX;

/// Emit a [`TwapFallbackUsedEvent`].
///
/// Topics: `("oracle", "v1", "twapFallbk")`
///
/// The three-segment topic mirrors the AMM event convention
/// (`"amm"`, `"v1"`, `<kind>`) so that indexers can subscribe to versioned
/// oracle events using the same `("oracle", "v1", *)` filter they use for
/// AMM events.
pub fn emit_twap_fallback_used(
    env: &Env,
    asset: &Address,
    twap_price: u128,
    primary_age_secs: u64,
) {
    env.events().publish(
        (
            symbol_short!("oracle"),
            symbol_short!("v1"),
            symbol_short!("twapFallbk"),
        ),
        TwapFallbackUsedEvent {
            schema_version: EVENT_SCHEMA_VERSION,
            asset: asset.clone(),
            twap_price,
            primary_age_secs,
        },
    );
}
