//! Oracle price feed and oracle/flash configuration entrypoints.

use super::LendingImpl;
use crate::*;

#[allow(clippy::too_many_arguments)]
impl LendingImpl {
    // Set the configured oracle pubkey used to verify signed price updates.
    pub(crate) fn set_oracle_pubkey(env: Env, pubkey: BytesN<32>) -> Result<(), LendingError> {
        require_initialized(&env)?;
        assert_admin(&env)?;
        let old_pubkey: Option<BytesN<32>> = env.storage().instance().get(&DataKey::OraclePubKey);
        env.storage()
            .instance()
            .set(&DataKey::OraclePubKey, &pubkey);
        OraclePubKeySetEvent {
            old_pubkey,
            new_pubkey: pubkey,
        }
        .publish(&env);
        Ok(())
    }

    // Returns the currently configured oracle pubkey, if set.
    pub(crate) fn get_oracle_pubkey(env: Env) -> Option<BytesN<32>> {
        env.storage().instance().get(&DataKey::OraclePubKey)
    }

    // Authorization: `caller` must be the configured price relayer
    // (set_price_relayer) or the admin, and must authorize the invocation. The
    // relayer role lets an always-online oracle process avoid the admin key.
    // The ed25519 signature over the canonical payload
    // (`oracle_price_signature_payload`) is the real authenticity boundary.
    // Freshness: the timestamp must not be in the future and must be no older
    // than the per-asset max age (set_oracle_max_age, default
    // DEFAULT_ORACLE_MAX_AGE_SECS).
    // Velocity: a per-update cap (MaxMoveBps) and a time-windowed deviation
    // guard are both enforced. The windowed guard compares against a per-asset
    // anchor and bounds the cumulative move within `move_window_secs`.
    /// Store a signed oracle price update. Caller: relayer or admin.
    pub(crate) fn set_price(
        env: Env,
        caller: Address,
        asset: Address,
        price: i128,
        timestamp: u64,
        signature: BytesN<64>,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        require_price_relayer_or_admin(&env, &caller)?;
        if price <= 0 {
            return Err(LendingError::InvalidAmount);
        }
        // Retrieve optional bounds
        let min_opt: Option<i128> = env
            .storage()
            .persistent()
            .get(&DataKey::PriceMin(asset.clone()));
        let max_opt: Option<i128> = env
            .storage()
            .persistent()
            .get(&DataKey::PriceMax(asset.clone()));
        if let Some(min) = min_opt {
            if price < min {
                return Err(LendingError::PriceOutOfBounds);
            }
        }
        if let Some(max) = max_opt {
            if price > max {
                return Err(LendingError::PriceOutOfBounds);
            }
        }

        let max_age = max_age_for(&env, &asset);
        let now = env.ledger().timestamp();
        if timestamp > now || now > timestamp.saturating_add(max_age) {
            return Err(LendingError::StaleOracleTimestamp);
        }
        // Load the existing record once. The freshness and retry policy is:
        // 1. A strictly older timestamp can never overwrite a newer record.
        // 2. Two different prices can never share the same timestamp.
        // 3. Replaying the exact current (price, timestamp) is an idempotent
        //    retry: it is not an error, but it also does not mutate state.
        let existing_record: Option<PriceRecord> = env
            .storage()
            .persistent()
            .get(&DataKey::OraclePrice(asset.clone()));
        if let Some(last) = existing_record.as_ref() {
            if timestamp < last.timestamp {
                return Err(LendingError::OracleReplay);
            }
            if timestamp == last.timestamp && price != last.price {
                return Err(LendingError::OracleReplay);
            }
            if timestamp == last.timestamp && price == last.price {
                return Ok(());
            }
        }

        // Velocity guards. Both run before the expensive signature
        // verification, along with the other input checks.
        let max_move_bps: Option<i128> = env
            .storage()
            .instance()
            .get::<DataKey, i128>(&DataKey::MaxMoveBps)
            .filter(|&bps| bps > 0);

        // Per-update move-cap circuit breaker (relative to the previous update).
        if let Some(max_move_bps) = max_move_bps {
            if let Some(last) = existing_record.as_ref() {
                if bps_move(price, last.price)? > max_move_bps {
                    return Err(LendingError::MaxMoveBpsExceeded);
                }
            }
        }

        // Time-windowed deviation guard (relative to the window anchor). The
        // anchor is reset once `move_window_secs` has elapsed, at which point a
        // move of up to `max_move_bps` from the new anchor is permitted. When no
        // cap is configured (or the asset has no prior record) the anchor is
        // simply (re)seeded and no deviation check applies.
        let window = move_window_secs(&env);
        let existing_anchor: Option<OracleAnchor> = env
            .storage()
            .persistent()
            .get(&DataKey::OracleAnchor(asset.clone()));
        let anchor_expired = match existing_anchor.as_ref() {
            Some(anchor) => now.saturating_sub(anchor.timestamp) >= window,
            None => true,
        };
        match (max_move_bps, existing_anchor.as_ref()) {
            (Some(max_move_bps), Some(anchor)) if !anchor_expired => {
                if bps_move(price, anchor.price)? > max_move_bps {
                    return Err(LendingError::MaxMoveBpsExceeded);
                }
            }
            (Some(_), Some(_)) => {
                // Window elapsed: reseed the anchor at the new price.
                env.storage().persistent().set(
                    &DataKey::OracleAnchor(asset.clone()),
                    &OracleAnchor { price, timestamp },
                );
            }
            (Some(_), None) => {
                // First observed price for this asset seeds the anchor.
                env.storage().persistent().set(
                    &DataKey::OracleAnchor(asset.clone()),
                    &OracleAnchor { price, timestamp },
                );
            }
            (None, _) => {}
        }

        let oracle_pubkey: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::OraclePubKey)
            .ok_or(LendingError::OraclePubkeyNotSet)?;

        let payload = Self::oracle_price_signature_payload(&env, &asset, price, timestamp);
        // `Crypto::ed25519_verify` traps the transaction when the signature is
        // not valid for `(oracle_pubkey, payload)`, so completing the call is
        // itself the proof that the caller holds the oracle key for this exact
        // payload.
        env.crypto()
            .ed25519_verify(&oracle_pubkey, &payload, &signature);

        env.storage().persistent().set(
            &DataKey::OraclePrice(asset.clone()),
            &PriceRecord { price, timestamp },
        );
        crate::ttl::bump_global(&env, &DataKey::OraclePrice(asset.clone()));
        Ok(())
    }

    pub(crate) fn get_price_record(env: Env, asset: Address) -> Option<PriceRecord> {
        env.storage().persistent().get(&DataKey::OraclePrice(asset))
    }

    // Return the ed25519 signing payload for a price update.
    pub(crate) fn oracle_payload(env: Env, asset: Address, price: i128, timestamp: u64) -> Bytes {
        Self::oracle_price_signature_payload(&env, &asset, price, timestamp)
    }

    // Build the v2 ed25519 signing payload for a price update.
    // 
    // Framing: each variable-length field is preceded by its 4-byte big-endian
    // length so that no two distinct `(asset, price, timestamp)` tuples can
    // produce the same byte string (field-confusion / splice forgery impossible).
    // 
    // Layout:
    // ```text
    // "StellarLendOracle:v2"    (fixed 20 bytes)
    // network_id                (32 bytes)
    // u32_be(len(contract_xdr)) (4 bytes)
    // contract_xdr              (variable)
    // u32_be(len(asset_xdr))    (4 bytes)
    // asset_xdr                 (variable)
    // price_i128_be             (16 bytes fixed — no length prefix needed)
    // timestamp_u64_be          (8 bytes  fixed — no length prefix needed)
    // ```
    pub(crate) fn oracle_price_signature_payload(
        env: &Env,
        asset: &Address,
        price: i128,
        timestamp: u64,
    ) -> Bytes {
        let contract_xdr = env.current_contract_address().to_xdr(env);
        let contract_len = contract_xdr.len();
        let asset_xdr = asset.to_xdr(env);
        let asset_len = asset_xdr.len();
        let network_id = env.ledger().network_id();

        let mut payload = Bytes::new(env);
        payload.append(&Bytes::from_slice(env, ORACLE_SIGNATURE_DOMAIN));
        payload.append(&Bytes::from_slice(env, &network_id.to_array()));
        payload.append(&Bytes::from_slice(env, &contract_len.to_be_bytes()));
        payload.append(&contract_xdr);
        payload.append(&Bytes::from_slice(env, &asset_len.to_be_bytes()));
        payload.append(&asset_xdr);
        // Fixed-width fields need no length prefix
        payload.append(&Bytes::from_slice(env, &price.to_be_bytes()));
        payload.append(&Bytes::from_slice(env, &timestamp.to_be_bytes()));
        payload
    }

    pub(crate) fn get_flash_fee_bps(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::FlashFeeBps)
            .unwrap_or(5)
    }

    pub(crate) fn max_flash_bps_config(env: &Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::MaxFlashUtilizationBps)
            .unwrap_or(DEFAULT_MAX_FLASH_BPS)
    }

    pub(crate) fn set_max_move_bps(env: Env, max_move_bps: i128) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        if !(0..=BPS_DENOM).contains(&max_move_bps) {
            return Err(LendingError::InvalidAmount);
        }
        env.storage()
            .instance()
            .set(&DataKey::MaxMoveBps, &max_move_bps);
        MaxMoveBpsSetEvent { max_move_bps }.publish(&env);

        // Record audit entry
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "set_max_move_bps"),
            admin,
            None,
        );

        Ok(())
    }

    pub(crate) fn get_max_move_bps(env: Env) -> Option<i128> {
        env.storage().instance().get(&DataKey::MaxMoveBps)
    }

    // Freshness checks for `asset` (in set_price, the cross-asset valuation
    // paths and require_fresh_valuation_prices) use this value instead of the
    // protocol default. `secs` must lie in
    // [MIN_ORACLE_MAX_AGE_SECS, MAX_ORACLE_MAX_AGE_SECS] (60 s to 24 h). A max
    // age of 0 is rejected because it would mark every price as instantly
    // stale and brick borrowing. Returns LendingError::InvalidAmount when out
    // of range.
    /// Set per-asset max oracle price age in seconds (admin-only).
    pub(crate) fn set_oracle_max_age(env: Env, asset: Address, secs: u64) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        if !(MIN_ORACLE_MAX_AGE_SECS..=MAX_ORACLE_MAX_AGE_SECS).contains(&secs) {
            return Err(LendingError::InvalidAmount);
        }
        env.storage()
            .persistent()
            .set(&DataKey::OracleMaxAge(asset.clone()), &secs);
        OracleMaxAgeSetEvent {
            asset,
            max_age_secs: secs,
        }
        .publish(&env);

        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "set_oracle_max_age"),
            admin,
            None,
        );

        Ok(())
    }

    /// Read the configured per-asset max oracle age, if set.
    pub(crate) fn get_oracle_max_age(env: Env, asset: Address) -> Option<u64> {
        env.storage()
            .persistent()
            .get(&DataKey::OracleMaxAge(asset))
    }

    // A price may deviate at most `max_move_bps` from its per-asset anchor
    // within this window; the anchor resets once the window elapses. Bounds are
    // [MIN_MOVE_WINDOW_SECS, MAX_MOVE_WINDOW_SECS] (1 s to 24 h).
    /// Set the windowed deviation guard length in seconds (admin-only).
    pub(crate) fn set_move_window_secs(env: Env, secs: u64) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        if !(MIN_MOVE_WINDOW_SECS..=MAX_MOVE_WINDOW_SECS).contains(&secs) {
            return Err(LendingError::InvalidAmount);
        }
        env.storage()
            .instance()
            .set(&DataKey::MoveWindowSecs, &secs);
        MoveWindowSetEvent {
            move_window_secs: secs,
        }
        .publish(&env);

        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "set_move_window_secs"),
            admin,
            None,
        );

        Ok(())
    }

    /// Read the configured windowed-deviation window length in seconds.
    pub(crate) fn get_move_window_secs(env: Env) -> u64 {
        move_window_secs(&env)
    }

    // Separates the always-online oracle relayer key from the admin key. The
    // relayer still must supply a valid ed25519 signature over the price
    // payload; the role only gates who may submit an otherwise-valid update.
    /// Set the optional price relayer address (admin-only).
    pub(crate) fn set_price_relayer(env: Env, relayer: Address) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        env.storage()
            .instance()
            .set(&DataKey::PriceRelayer, &relayer);
        PriceRelayerSetEvent {
            relayer: relayer.clone(),
        }
        .publish(&env);

        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "set_price_relayer"),
            admin,
            None,
        );

        Ok(())
    }

    /// Read the configured price relayer, if any.
    pub(crate) fn get_price_relayer(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::PriceRelayer)
    }

    // Set the maximum flash-loan utilization ratio in basis points (admin-only).
    // The requested amount must not exceed `max_flash_bps × available_liquidity / 10000`.
    pub(crate) fn set_max_flash_bps(env: Env, max_flash_bps: i128) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        if !(0..=BPS_DENOM).contains(&max_flash_bps) {
            return Err(LendingError::InvalidFlashUtilizationBps);
        }
        env.storage()
            .instance()
            .set(&DataKey::MaxFlashUtilizationBps, &max_flash_bps);
        MaxFlashBpsSetEvent { max_flash_bps }.publish(&env);

        // Record audit entry
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "set_max_flash_bps"),
            admin,
            None,
        );

        Ok(())
    }

    // Set per‑asset price bounds (admin‑only).
    // `min_price` must be > 0 and less than `max_price`.
    //
    // If the asset already has a stored price, the new bounds must contain it,
    // otherwise the stored price would be rejected by future `set_price` calls
    // until refreshed, and freshness would fail closed. Pass `force = true` to
    // intentionally install bounds that exclude the current price (for example
    // when the price is known to be wrong and will be corrected immediately).
    // This is an explicit, audited override.
    pub(crate) fn set_price_bounds(
        env: Env,
        asset: Address,
        min_price: i128,
        max_price: i128,
        force: bool,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        if min_price <= 0 || max_price <= 0 || min_price >= max_price {
            return Err(LendingError::InvalidAmount);
        }

        if !force {
            if let Some(current) = env
                .storage()
                .persistent()
                .get::<_, PriceRecord>(&DataKey::OraclePrice(asset.clone()))
            {
                if current.price < min_price || current.price > max_price {
                    return Err(LendingError::PriceOutOfBounds);
                }
            }
        }

        env.storage()
            .persistent()
            .set(&DataKey::PriceMin(asset.clone()), &min_price);
        crate::ttl::bump_global(&env, &DataKey::PriceMin(asset.clone()));
        env.storage()
            .persistent()
            .set(&DataKey::PriceMax(asset.clone()), &max_price);
        crate::ttl::bump_global(&env, &DataKey::PriceMax(asset.clone()));
        PriceBoundsSetEvent {
            asset,
            min_price,
            max_price,
        }
        .publish(&env);

        // Record audit entry
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "set_price_bounds"),
            admin,
            None,
        );

        Ok(())
    }

    // Retrieve price bounds for an asset. Returns `(Option<min>, Option<max>)`.
    pub(crate) fn get_price_bounds(env: Env, asset: Address) -> (Option<i128>, Option<i128>) {
        let min = env
            .storage()
            .persistent()
            .get(&DataKey::PriceMin(asset.clone()));
        let max = env.storage().persistent().get(&DataKey::PriceMax(asset));
        (min, max)
    }

    // Return the configured maximum flash-loan utilization ratio in basis points.
    pub(crate) fn get_max_flash_bps(env: Env) -> i128 {
        Self::max_flash_bps_config(&env)
    }
}
