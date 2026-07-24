/// twap_fallback_event_test.rs — Structured TwapFallbackUsedEvent emission tests.
///
/// Coverage matrix
/// ───────────────
/// ✓  Fresh primary oracle → NO event emitted
/// ✓  Stale primary oracle → event emitted with correct asset, price, age
/// ✓  Missing primary oracle feed → event emitted with PRIMARY_FEED_ABSENT age
/// ✓  `schema_version` field equals EVENT_SCHEMA_VERSION
/// ✓  `twap_price` in event matches the raw TWAP returned by get_twap
/// ✓  `primary_age_secs` in event matches actual staleness duration
/// ✓  Fallback path is idempotent: second call emits a second event
/// ✓  get_price (full path) emits event when stale
/// ✓  get_price (full path) emits event when feed is absent

#[cfg(test)]
mod tests {
    use soroban_sdk::{testutils::Ledger, Address, Env};

    use crate::amm;
    use crate::amm_twap::{update_twap_accumulators, PRICE_SCALE};
    use crate::events::{TwapFallbackUsedEvent, EVENT_SCHEMA_VERSION, PRIMARY_FEED_ABSENT};
    use crate::oracle::{
        get_price, get_price_with_fallback, set_oracle_config, update_price_feed, ExternalOracle,
        OracleConfig, OracleDataKey, PriceFeed,
    };

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn advance(env: &Env, secs: u64) {
        let t = env.ledger().timestamp();
        env.ledger().set_timestamp(t + secs);
    }

    /// Build enough TWAP history for the fallback path to succeed.
    ///
    /// Writes snapshots at 60-second intervals so `get_twap(150)` finds a
    /// valid start anchor.
    fn seed_twap_history(env: &Env, asset: &Address) {
        env.ledger().set_timestamp(0);
        amm::initialise_pool(env, asset, 1_000_000, 1_000_000);
        for i in 1u64..=5 {
            env.ledger().set_timestamp(i * 60);
            amm::swap(env, asset, 100, true);
        }
        // Advance to a "current" time far enough to have a full window.
        env.ledger().set_timestamp(10_000);
    }

    /// A mock oracle that returns a configurable price at a configurable age,
    /// or simulates a total outage (price = None).
    struct MockOracle {
        price: Option<u128>,
        age_secs: u64,
    }

    impl ExternalOracle for MockOracle {
        fn get_price(&self, env: &Env, _asset: &Address) -> Option<(u128, u64)> {
            self.price.map(|p| {
                let obs_ts = env.ledger().timestamp().saturating_sub(self.age_secs);
                (p, obs_ts)
            })
        }
    }

    /// Decode the first TwapFallbackUsedEvent from `env.events().all()`.
    ///
    /// Returns `None` if no such event was emitted.
    fn find_fallback_event(env: &Env) -> Option<TwapFallbackUsedEvent> {
        // The Soroban test environment records events as (topics, data) pairs.
        // We iterate and try to decode each one — the first that decodes as
        // TwapFallbackUsedEvent is returned.
        //
        // Because soroban_sdk::testutils does not provide a typed filter, we
        // rely on the fact that TwapFallbackUsedEvent has a distinctive set of
        // fields; we scan for schema_version == EVENT_SCHEMA_VERSION as a
        // discriminant.
        //
        // In practice the test helpers below assert specific field values, so
        // a false-positive match from another versioned event with the same
        // version number would be caught by the field assertions.
        use soroban_sdk::testutils::Events;
        for event in env.events().all().iter() {
            // Try to deserialise the event data as TwapFallbackUsedEvent.
            // soroban_sdk provides IntoVal / TryFromVal; we use the raw XDR
            // round-trip available on test event data.
            let (_, _, raw_data) = event;
            if let Ok(decoded) = TwapFallbackUsedEvent::try_from_val(env, &raw_data) {
                if decoded.schema_version == EVENT_SCHEMA_VERSION {
                    return Some(decoded);
                }
            }
        }
        None
    }

    // ── 1. Fresh primary → no event ──────────────────────────────────────────

    /// When the primary oracle is fresh, `get_price_with_fallback` must return
    /// the primary price and must NOT emit a `TwapFallbackUsedEvent`.
    #[test]
    fn fresh_primary_emits_no_fallback_event() {
        let env = Env::default();
        let asset = Address::generate(&env);
        seed_twap_history(&env, &asset);

        set_oracle_config(
            &env,
            &OracleConfig {
                oracle_address: asset.clone(),
                max_age_secs: 300,
                twap_window_secs: 150,
            },
        );

        let oracle = MockOracle {
            price: Some(2 * PRICE_SCALE), // fresh 2:1 price
            age_secs: 10,                 // 10 s old — well within 300 s window
        };

        let result = get_price_with_fallback(&env, &asset, &oracle);

        assert!(!result.is_twap_fallback, "expected primary path");
        assert_eq!(result.price_scaled, 2 * PRICE_SCALE);

        // No TwapFallbackUsedEvent must have been emitted.
        assert!(
            find_fallback_event(&env).is_none(),
            "no fallback event should be emitted when primary is fresh"
        );
    }

    // ── 2. Stale primary → event emitted ─────────────────────────────────────

    /// When the primary oracle price is stale, `get_price_with_fallback` must
    /// use the TWAP and emit a `TwapFallbackUsedEvent` with the correct fields.
    #[test]
    fn stale_primary_emits_fallback_event_with_correct_fields() {
        let env = Env::default();
        let asset = Address::generate(&env);
        seed_twap_history(&env, &asset);

        let staleness = 500u64; // 500 s > 300 s max_age

        set_oracle_config(
            &env,
            &OracleConfig {
                oracle_address: asset.clone(),
                max_age_secs: 300,
                twap_window_secs: 150,
            },
        );

        let oracle = MockOracle {
            price: Some(5 * PRICE_SCALE),
            age_secs: staleness,
        };

        let result = get_price_with_fallback(&env, &asset, &oracle);
        assert!(result.is_twap_fallback, "expected TWAP fallback path");

        // Event must have been emitted.
        let event = find_fallback_event(&env)
            .expect("TwapFallbackUsedEvent must be emitted on stale primary");

        // schema_version
        assert_eq!(
            event.schema_version, EVENT_SCHEMA_VERSION,
            "schema_version must equal EVENT_SCHEMA_VERSION"
        );

        // asset
        assert_eq!(
            event.asset, asset,
            "event asset must match the queried asset"
        );

        // primary_age_secs — must match the staleness we set
        assert_eq!(
            event.primary_age_secs, staleness,
            "primary_age_secs must equal the measured staleness"
        );

        // twap_price — must match what was returned as the resolved price
        assert_eq!(
            event.twap_price, result.price_scaled,
            "event twap_price must equal the resolved TWAP price"
        );
    }

    // ── 3. Missing primary → event emitted with PRIMARY_FEED_ABSENT ──────────

    /// When the primary oracle returns `None` (outage), `get_price_with_fallback`
    /// must fall back to TWAP and emit an event with `primary_age_secs =
    /// PRIMARY_FEED_ABSENT`.
    #[test]
    fn missing_primary_emits_fallback_event_with_absent_sentinel() {
        let env = Env::default();
        let asset = Address::generate(&env);
        seed_twap_history(&env, &asset);

        set_oracle_config(
            &env,
            &OracleConfig {
                oracle_address: asset.clone(),
                max_age_secs: 300,
                twap_window_secs: 150,
            },
        );

        let oracle = MockOracle {
            price: None, // total outage
            age_secs: 0,
        };

        let result = get_price_with_fallback(&env, &asset, &oracle);
        assert!(result.is_twap_fallback, "expected TWAP fallback path");

        let event = find_fallback_event(&env)
            .expect("TwapFallbackUsedEvent must be emitted on oracle outage");

        assert_eq!(
            event.primary_age_secs, PRIMARY_FEED_ABSENT,
            "primary_age_secs must be PRIMARY_FEED_ABSENT when feed is missing"
        );
        assert_eq!(event.schema_version, EVENT_SCHEMA_VERSION);
        assert_eq!(event.asset, asset);
    }

    // ── 4. schema_version field is current version ────────────────────────────

    /// The `schema_version` field in every emitted event must equal
    /// `EVENT_SCHEMA_VERSION` at runtime.
    #[test]
    fn event_schema_version_matches_constant() {
        let env = Env::default();
        let asset = Address::generate(&env);
        seed_twap_history(&env, &asset);

        set_oracle_config(
            &env,
            &OracleConfig {
                oracle_address: asset.clone(),
                max_age_secs: 300,
                twap_window_secs: 150,
            },
        );

        get_price_with_fallback(
            &env,
            &asset,
            &MockOracle {
                price: None,
                age_secs: 0,
            },
        );

        let event = find_fallback_event(&env).expect("event must be emitted");
        assert_eq!(
            event.schema_version, EVENT_SCHEMA_VERSION,
            "schema_version mismatch: event has {}, expected {}",
            event.schema_version, EVENT_SCHEMA_VERSION
        );
    }

    // ── 5. twap_price matches get_twap output ─────────────────────────────────

    /// The `twap_price` in the event must be bit-for-bit identical to the value
    /// that `amm_twap::get_twap` would return directly for the same parameters.
    #[test]
    fn event_twap_price_matches_direct_get_twap() {
        use crate::amm_twap::get_twap;

        let env = Env::default();
        let asset = Address::generate(&env);
        seed_twap_history(&env, &asset);

        let window = 150u64;
        // Snapshot the TWAP directly before the fallback path touches it.
        let expected_twap = get_twap(&env, &asset, window);

        set_oracle_config(
            &env,
            &OracleConfig {
                oracle_address: asset.clone(),
                max_age_secs: 300,
                twap_window_secs: window,
            },
        );

        get_price_with_fallback(
            &env,
            &asset,
            &MockOracle {
                price: None,
                age_secs: 0,
            },
        );

        let event = find_fallback_event(&env).expect("event must be emitted");
        assert_eq!(
            event.twap_price, expected_twap,
            "event twap_price must match direct get_twap output"
        );
    }

    // ── 6. Fallback path is idempotent — second call emits second event ────────

    /// Calling `get_price_with_fallback` twice on a stale oracle must emit
    /// the event both times (cache is not bypassed for event emission).
    #[test]
    fn repeated_fallback_emits_event_each_time() {
        use soroban_sdk::testutils::Events;

        let env = Env::default();
        let asset = Address::generate(&env);
        seed_twap_history(&env, &asset);

        set_oracle_config(
            &env,
            &OracleConfig {
                oracle_address: asset.clone(),
                max_age_secs: 300,
                twap_window_secs: 150,
            },
        );

        let oracle = MockOracle {
            price: None,
            age_secs: 0,
        };

        get_price_with_fallback(&env, &asset, &oracle);
        let count_after_first = count_fallback_events(&env);
        assert_eq!(count_after_first, 1, "expected 1 event after first call");

        get_price_with_fallback(&env, &asset, &oracle);
        let count_after_second = count_fallback_events(&env);
        assert_eq!(count_after_second, 2, "expected 2 events after second call");
    }

    /// Count TwapFallbackUsedEvents in the current event log.
    fn count_fallback_events(env: &Env) -> usize {
        use soroban_sdk::testutils::Events;
        let mut count = 0;
        for event in env.events().all().iter() {
            let (_, _, raw_data) = event;
            if let Ok(decoded) = TwapFallbackUsedEvent::try_from_val(env, &raw_data) {
                if decoded.schema_version == EVENT_SCHEMA_VERSION {
                    count += 1;
                }
            }
        }
        count
    }

    // ── 7. get_price full path: stale feed emits event ─────────────────────────

    /// The main `get_price` path (not the external-oracle wrapper) must also
    /// emit `TwapFallbackUsedEvent` when the stored `PriceFeed` is stale and
    /// the TWAP fallback is used.
    #[test]
    fn get_price_emits_event_when_feed_is_stale() {
        let env = Env::default();
        let asset = Address::generate(&env);
        seed_twap_history(&env, &asset);

        // Write a stale PriceFeed directly to persistent storage.
        let stale_ts = 100u64; // 100 s — will be stale relative to t=10_000
        let stale_feed = PriceFeed {
            price: 1_000_000,
            last_updated: stale_ts,
            oracle: asset.clone(),
            decimals: 6,
        };
        env.storage()
            .persistent()
            .set(&OracleDataKey::PriceFeed(asset.clone()), &stale_feed);

        // get_price should detect staleness and use the TWAP fallback.
        let _price = get_price(&env, &asset).expect("price should resolve via TWAP fallback");

        let event = find_fallback_event(&env)
            .expect("TwapFallbackUsedEvent must be emitted when stored feed is stale");

        // primary_age_secs must reflect the actual age of the stored feed.
        let expected_age = 10_000u64.saturating_sub(stale_ts);
        assert_eq!(
            event.primary_age_secs, expected_age,
            "primary_age_secs must equal the measured staleness of the stored feed"
        );
        assert_eq!(event.schema_version, EVENT_SCHEMA_VERSION);
        assert_eq!(event.asset, asset);
    }

    // ── 8. get_price full path: absent feed emits event ───────────────────────

    /// When no `PriceFeed` entry exists at all, `get_price` falls through to
    /// the TWAP path and must emit `TwapFallbackUsedEvent` with the sentinel age.
    #[test]
    fn get_price_emits_event_when_feed_is_absent() {
        let env = Env::default();
        let asset = Address::generate(&env);
        seed_twap_history(&env, &asset);

        // No PriceFeed written — storage is empty for this asset.
        let _price = get_price(&env, &asset).expect("price should resolve via TWAP fallback");

        let event = find_fallback_event(&env)
            .expect("TwapFallbackUsedEvent must be emitted when no feed exists");

        assert_eq!(
            event.primary_age_secs, PRIMARY_FEED_ABSENT,
            "primary_age_secs must be PRIMARY_FEED_ABSENT when no feed record exists"
        );
        assert_eq!(event.schema_version, EVENT_SCHEMA_VERSION);
        assert_eq!(event.asset, asset);
    }
}
