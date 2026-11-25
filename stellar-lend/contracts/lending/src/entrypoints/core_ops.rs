//! Core user operations: receive, deposit, withdraw, borrow, repay-against-collateral and liquidation.

use super::LendingImpl;
use crate::*;

#[allow(clippy::too_many_arguments)]
impl LendingImpl {
    // Receive tokens from a user via the SEP-41 `transfer_from`/`approve`
    // allowance flow and apply them as a deposit or debt repayment.
    // 
    // This is a pull-based entrypoint: the user authorizes the lending
    // contract as a spender on the token contract (via `approve`) and then
    // calls `receive` with a payload specifying the desired action.  The
    // contract pulls `amount` from `from` into its own balance via
    // `transfer_from` before updating any protocol state.
    // 
    // # Payload format
    // 
    // `[version: u32, action: Symbol, ...optional_data]`
    // 
    // * `version` — Must be `1` (current schema version).
    // * `action` — A `Symbol` indicating the action (`"deposit"` or
    //   `"repay"`).
    // * `optional_data` — Reserved for future use; ignored in the current
    //   version.
    // 
    // # Security
    // 
    // * Requires `from.require_auth()` so only the token owner can
    //   authorize a pull.
    // * Validates that `token_asset` matches the configured
    //   `ValuationCollateralAsset` before performing any transfer.
    // * Validates payload version and action before performing any
    //   transfer.
    // * The protocol pause and emergency state checks are delegated to the
    //   inner [`LendingContract::deposit`] / [`LendingContract::repay`]
    //   handlers (defence-in-depth: both `receive` and the inner handler
    //   call `require_initialized` and `require_auth`).
    // * Rejects calls from the token contract itself and the lending
    //   contract (prevents self-call attacks).
    // 
    // # Errors
    // 
    // * [`LendingError::InvalidPayloadVersion`] if `version != 1`.
    // * [`LendingError::MalformedPayload`] if the payload is too short,
    //   too long, or contains undecodable values.
    // * [`LendingError::AssetNotSupported`] if `token_asset` does not
    //   match the configured valuation collateral asset, or if the action
    //   is not `"deposit"` or `"repay"`.
    // * [`LendingError::UnauthorizedSender`] if `from` is the token
    //   contract or the lending contract itself.
    // * All errors from [`LendingContract::deposit`] or
    //   [`LendingContract::repay`] respectively.
    pub(crate) fn receive(
        env: Env,
        token_asset: Address,
        from: Address,
        amount: i128,
        payload: Vec<Val>,
    ) -> Result<(), LendingError> {
        require_initialized(&env)?;

        // Validate that token_asset matches the configured valuation
        // collateral asset so deposit/repay accounting stays consistent.
        let configured_asset =
            Self::get_collateral_asset(env.clone()).ok_or(LendingError::AssetNotSupported)?;
        if token_asset != configured_asset {
            return Err(LendingError::AssetNotSupported);
        }

        // Safety: reject calls from the token contract itself or the lending
        // contract (prevents self-call / reentrancy attacks).
        let contract_address = env.current_contract_address();
        if from == token_asset || from == contract_address {
            return Err(LendingError::UnauthorizedSender);
        }

        from.require_auth();

        // Parse payload: [version: u32, action: Symbol, ...optional_data]
        if payload.len() < MIN_RECEIVE_PAYLOAD_LEN {
            return Err(LendingError::MalformedPayload);
        }
        if payload.len() > MAX_RECEIVE_PAYLOAD_LEN {
            return Err(LendingError::MalformedPayload);
        }

        let version: u32 = payload
            .get(0)
            .unwrap()
            .try_into_val(&env)
            .map_err(|_| LendingError::MalformedPayload)?;
        if version != SCHEMA_VERSION_V1 {
            return Err(LendingError::InvalidPayloadVersion);
        }

        let action_sym: Symbol = payload
            .get(1)
            .unwrap()
            .try_into_val(&env)
            .map_err(|_| LendingError::MalformedPayload)?;

        let is_deposit = action_sym == symbol_short!("deposit");
        let is_repay = action_sym == symbol_short!("repay");
        if !is_deposit && !is_repay {
            return Err(LendingError::AssetNotSupported);
        }

        // Pull tokens from the user into the contract via transfer_from.
        // The user must have previously called token_client.approve() to
        // authorize the lending contract as a spender.
        let token_client = TokenClient::new(&env, &token_asset);
        token_client.transfer_from(&contract_address, &from, &contract_address, &amount);

        if is_deposit {
            // Defence-in-depth: deposit() also calls require_initialized
            // and user.require_auth(). The double check is intentional.
            Self::deposit(env, from, amount)?;
        } else {
            // Defence-in-depth: repay() also calls require_initialized
            // and user.require_auth(). The double check is intentional.
            Self::repay(env, from, amount)?;
        }

        Ok(())
    }

    // Deposit collateral with comprehensive authorization and validation.
    // 
    // # Security
    // - Validates authorization through require_auth
    // - Validates amount and deposit cap
    // - Checks reserve invariants before and after
    // - Prevents replay attacks
    // - Enforces network validation
    pub(crate) fn deposit(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
        require_initialized(&env)?;
        check_pause_status(&env, ProtocolAction::Deposit);
        check_emergency_status(&env, ProtocolAction::Deposit);
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }

        require_no_active_flash_loan(&env)?;
        user.require_auth();

        let total_deposits: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0);
        let deposit_cap: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::DepositCap)
            .unwrap_or(DEFAULT_DEPOSIT_CAP);
        let new_total = total_deposits
            .checked_add(amount)
            .ok_or(LendingError::Overflow)?;
        if new_total > deposit_cap {
            return Err(LendingError::DepositCapExceeded);
        }

        let key = DataKey::Collateral(user.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        let new_balance = current.checked_add(amount).ok_or(LendingError::Overflow)?;
        env.storage().persistent().set(&key, &new_balance);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &new_total);
        crate::ttl::bump_global(&env, &DataKey::TotalDeposits);
        crate::ttl::bump_global(&env, &DataKey::DepositCap);
        extend_collateral_ttl(&env, &user);

        // Emit deposit event
        emit_deposit(&env, &user, amount, new_balance);

        check_and_clear_unhealthy_timestamp(&env, &user);

        Ok(new_balance)
    }

    // Withdraw collateral after pause and emergency gates pass.
    // 
    // Rejects with `InsufficientCollateral` if, after accruing interest, the
    // remaining collateral would leave an open debt position below HF 1.0.
    // Positions with no debt may withdraw everything.
    pub(crate) fn withdraw(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
        require_initialized(&env)?;
        check_pause_status(&env, ProtocolAction::Withdraw);
        check_emergency_status(&env, ProtocolAction::Withdraw);
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }

        require_no_active_flash_loan(&env)?;
        user.require_auth();
        let key = DataKey::Collateral(user.clone());
        let current: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if amount > current {
            return Err(LendingError::InvalidAmount);
        }
        let new_balance = current.checked_sub(amount).ok_or(LendingError::Overflow)?;
        // Solvency check against accrued debt (read-only) before any write.
        // In cross-asset mode both oracle prices must be fresh first.
        if uses_cross_asset_valuation(&env) {
            require_fresh_valuation_prices(&env)?;
        }
        let position = load_debt(&env, &user);
        assert_position_healthy(&env, new_balance, &position)?;
        env.storage().persistent().set(&key, &new_balance);
        let total_deposits: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0);
        let new_total = total_deposits
            .checked_sub(amount)
            .ok_or(LendingError::Overflow)?;
        env.storage()
            .persistent()
            .set(&DataKey::TotalDeposits, &new_total);
        crate::ttl::bump_global(&env, &DataKey::TotalDeposits);
        extend_collateral_ttl(&env, &user);

        // Emit withdraw event
        emit_withdraw(&env, &user, amount, new_balance);

        Ok(new_balance)
    }

    // Set the configured valuation collateral asset for the legacy single-asset flows.
    pub(crate) fn set_collateral_asset(env: Env, asset: Address) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        env.storage()
            .instance()
            .set(&DataKey::ValuationCollateralAsset, &asset);
        CollateralAssetSetEvent { asset }.publish(&env);

        // Record audit entry
        audit_log::record_audit_entry(
            &env,
            String::from_str(&env, "set_collateral_asset"),
            admin,
            None,
        );

        Ok(())
    }

    // Get the configured valuation collateral asset for the legacy single-asset flows.
    pub(crate) fn get_collateral_asset(env: Env) -> Option<Address> {
        env.storage()
            .instance()
            .get(&DataKey::ValuationCollateralAsset)
    }

    // Set the configured valuation debt asset for legacy single-position flows.
    pub(crate) fn set_debt_asset(env: Env, asset: Address) -> Result<(), LendingError> {
        require_initialized(&env)?;
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(LendingError::NotInitialized)?;
        admin.require_auth();

        env.storage()
            .instance()
            .set(&DataKey::ValuationDebtAsset, &asset);
        DebtAssetSetEvent { asset }.publish(&env);

        audit_log::record_audit_entry(&env, String::from_str(&env, "set_debt_asset"), admin, None);

        Ok(())
    }

    // Get the configured valuation debt asset for the legacy single-position flows.
    pub(crate) fn get_debt_asset(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::ValuationDebtAsset)
    }

    // Borrow assets after pause and emergency gates pass.
    //
    // Accrues interest on the existing position, increases principal by `amount`,
    // and rejects the borrow when the post-borrow health factor would fall below
    // 1.0 (`HEALTH_FACTOR_SCALE`) or when protocol `TotalDebt` would exceed
    // `DataKey::DebtCeiling`.
    //
    // The validation and accounting pipeline is shared with
    // [`Self::borrow_against_collateral`] through [`borrow_internal`]; only the
    // isolation-mode ceiling differs between the two entry points.
    pub(crate) fn borrow(env: Env, user: Address, amount: i128) -> Result<i128, LendingError> {
        require_initialized(&env)?;
        check_pause_status(&env, ProtocolAction::Borrow);
        check_emergency_status(&env, ProtocolAction::Borrow);
        require_no_active_flash_loan(&env)?;
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }
        user.require_auth();

        // ── Validate & calculate (nothing is persisted yet) ──────────────────
        let (updated, delta) = borrow_internal(&env, &user, amount)?;

        // ── Persist ──────────────────────────────────────────────────────────
        commit_borrow(&env, &user, amount, &updated, delta)?;

        Ok(updated.principal)
    }

    // Isolation-aware borrow for cross-asset positions.
    //
    // Identical to [`borrow`] but additionally:
    //
    // 1. Checks that `collateral_asset` is not isolated **or**, if it is,
    //    that the new borrow does not push the running `IsolationDebt` past
    //    the asset's `isolation_debt_ceiling`.
    // 2. On success, increments `IsolationDebt(collateral_asset)` by the
    //    net new principal added to the user's position.
    //
    // Use this function in any cross-asset borrow path where `collateral_asset`
    // is the primary (or sole) isolated collateral backing the position.
    //
    // # Effect ordering
    //
    // Every check — the shared solvency / debt-ceiling pipeline performed by
    // [`borrow_internal`] *and* the isolation ceiling — runs before the debt
    // position is written, the debt TTL is extended or the `BorrowEvent` is
    // emitted. A rejected borrow therefore leaves no partial debt, isolation
    // accounting, TTL or event effect behind.
    //
    // # Errors
    // - All errors from [`borrow`].
    // - `IsolationCeilingExceeded` — borrow would breach the per-asset ceiling.
    pub(crate) fn borrow_against_collateral(
        env: Env,
        user: Address,
        amount: i128,
        collateral_asset: Address,
    ) -> Result<i128, LendingError> {
        require_initialized(&env)?;
        check_pause_status(&env, ProtocolAction::Borrow);
        check_emergency_status(&env, ProtocolAction::Borrow);
        require_no_active_flash_loan(&env)?;
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }
        user.require_auth();

        // ── Validate & calculate (nothing is persisted yet) ──────────────────
        let (updated, delta) = borrow_internal(&env, &user, amount)?;

        // The isolation ceiling is evaluated against the *principal delta*
        // (settled interest + borrowed amount) and therefore has to happen
        // before any state is written or any event is emitted.
        check_isolation_ceiling_internal(&env, &collateral_asset, delta)?;

        // ── Persist ──────────────────────────────────────────────────────────
        commit_borrow(&env, &user, amount, &updated, delta)?;

        // Update the per-asset isolation debt tracker only when the asset is
        // actually configured as isolated.  Non-isolated assets are not tracked.
        if is_asset_isolated(&env, &collateral_asset) {
            increment_isolation_debt(&env, &collateral_asset, delta)?;
        }

        // NOTE: the reserve-invariant checks are deliberately not invoked here.
        // `invariants::compute_expected_reserve` only models
        // `TotalDeposits - BadDebt`, so it cannot represent the collateral that
        // backs this position nor the debt this borrow just created, and
        // enabling it made every borrow trap. See `invariants.rs` and the
        // equivalent note in `liquidate`.

        Ok(updated.principal)
    }

    // Isolation-aware repay for cross-asset positions.
    // 
    // Identical to [`repay`] but decrements `IsolationDebt(collateral_asset)`
    // by the net principal reduction so the ceiling tracks accurately after
    // partial or full repayments.
    // 
    // Pass the same `collateral_asset` that was supplied to
    // [`borrow_against_collateral`] when the position was opened.
    pub(crate) fn repay_against_collateral(
        env: Env,
        user: Address,
        amount: i128,
        collateral_asset: Address,
    ) -> Result<i128, LendingError> {
        require_initialized(&env)?;
        check_pause_status(&env, ProtocolAction::Repay);
        check_emergency_status(&env, ProtocolAction::Repay);
        if amount <= 0 {
            return Err(LendingError::InvalidAmount);
        }

        require_no_active_flash_loan(&env)?;
        user.require_auth();
        let position = load_debt(&env, &user);
        let prev_principal = position.principal;
        let now = env.ledger().timestamp();
        let rate = current_borrow_rate(&env);
        let settled_position = settle_and_accrue_insurance(&env, &position, now, rate)?;
        let updated = repay_amount(settled_position, now, amount, rate).map_err(|e| match e {
            debt::DebtError::InvalidAmount => LendingError::InvalidAmount,
            debt::DebtError::RepayAmountTooHigh => LendingError::RepayAmountTooHigh,
            debt::DebtError::Overflow => LendingError::Overflow,
            debt::DebtError::IndexInvariantViolated => LendingError::Overflow,
        })?;
        save_debt(&env, &user, &updated);

        // Track protocol-level total debt.
        let total_debt: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::TotalDebt)
            .unwrap_or(0);
        let repaid = prev_principal.checked_sub(updated.principal).unwrap_or(0);
        let new_total_debt = total_debt.saturating_sub(repaid);
        env.storage()
            .persistent()
            .set(&DataKey::TotalDebt, &new_total_debt);
        crate::ttl::bump_global(&env, &DataKey::TotalDebt);
        extend_debt_ttl(&env, &user);

        // Emit repay event
        emit_repay(&env, &user, amount, updated.principal);

        // Decrement the per-asset isolation debt tracker only when the asset is
        // actually configured as isolated.
        if should_release_isolation_debt(&env, &collateral_asset) {
            decrement_isolation_debt(&env, &collateral_asset, repaid)?;
        }

        check_and_clear_unhealthy_timestamp(&env, &user);
        Ok(updated.principal)
    }

    // Liquidate an under-collateralized borrower position.
    // 
    // The entrypoint is wrapped in the reentrancy lock so a liquidation cannot
    // be re-entered from the same transaction context, and it also checks the
    // active-flash-loan guard before mutating state.
    // 
    // Self-liquidations are rejected immediately so a borrower cannot profit
    // from the liquidation incentive on their own collateral.
    // 
    // Uses checks-effects-interactions ordering: storage is updated after
    // validation and before token transfers so any transfer failure reverts
    // the whole liquidation atomically.
    // 
    // Anyone may call `liquidate` on a position whose health factor is below
    // the liquidation threshold (`< 1.0`).  The caller repays up to the
    // governed close-factor share of the borrower's debt (default 50 %) and
    // receives an equivalent amount of the borrower's collateral plus the
    // governed liquidation incentive (default 10 %).  The incentive is
    // minted from the borrower's collateral, not from the protocol.
    // 
    // The close factor and incentive are governable risk parameters — see
    // [`Self::get_close_factor_bps`] / [`Self::set_close_factor_bps`] and
    // [`Self::get_liquidation_incentive_bps`] /
    // [`Self::set_liquidation_incentive_bps`] — and the liquidation
    // threshold is governed through [`Self::set_liquidation_threshold_bps`].
    // 
    // # Rounding policy (all divisions favour protocol solvency)
    // 
    // | Division                     | Rounding | Rationale |
    // |------------------------------|----------|-----------|
    // | `hf = col × THRESHOLD ÷ debt` | floor    | Lower HF → position looks more underwater → earlier liquidation |
    // | `max_repay = debt × close_factor_bps ÷ 10000` | floor | Smaller cap → less debt extinguished per call → more rounds remain |
    // | `seized = repay × (10000 + incentive_bps) ÷ 10000`  | floor | Liquidator receives *less* collateral than the exact bonus |
    // 
    // All three use [`math::checked_mul_div_floor`] so every truncation
    // transfers the sub-unit remainder to the protocol / remaining borrowers.
    // # Storage read budget
    // 
    // This function is expected to perform at most **8** storage reads on the hot path:
    // 1. Flash active flag (instance storage)
    // 2. Collateral amount (persistent storage)
    // 3. Debt position (persistent storage via `load_debt`)
    // 4. Bad debt (persistent storage, only when shortfall occurs)
    // 5. Optional oracle price reads (instance storage) – enforced by `require_fresh_valuation_prices`
    // 6. Close-factor parameter lookup (instance storage)
    // 7. Liquidation-incentive parameter lookup (instance storage)
    // 8. Additional reads for temporary storage used internally.
    // 
    // The implementation aims to batch redundant reads where possible and avoids
    // re‑loading the same entry multiple times.
    // 
    // The per‑call read budget is enforced by a regression test in
    // `liquidate_perf_test.rs`.
    // 
    pub(crate) fn liquidate(
        env: Env,
        liquidator: Address,
        borrower: Address,
        debt_asset: Address,
        collateral_asset: Address,
        amount: i128,
    ) -> Result<i128, LendingError> {
        require_initialized(&env)?;
        // NOTE: the reserve-invariant checks added by #1967 are deliberately not
        // invoked here. `invariants::compute_expected_reserve` only models
        // `TotalDeposits - BadDebt`, so it cannot hold across a liquidation,
        // which moves `debt_asset` into the contract and `collateral_asset` out
        // of it. Enabling it made every liquidation trap. See `invariants.rs`.
        with_reentrancy_lock(&env, || {
            liquidator.require_auth();
            if liquidator == borrower {
                return Err(LendingError::SelfLiquidation);
            }

            check_pause_status(&env, ProtocolAction::Liquidate);
            require_fresh_valuation_prices(&env)?;
            require_no_active_flash_loan(&env)?;
            if uses_cross_asset_valuation(&env) {
                let configured_debt_asset: Address = env
                    .storage()
                    .instance()
                    .get(&DataKey::ValuationDebtAsset)
                    .ok_or(LendingError::AssetNotConfigured)?;
                let configured_collateral_asset: Address = env
                    .storage()
                    .instance()
                    .get(&DataKey::ValuationCollateralAsset)
                    .ok_or(LendingError::AssetNotConfigured)?;
                if debt_asset != configured_debt_asset
                    || collateral_asset != configured_collateral_asset
                {
                    return Err(LendingError::AssetNotConfigured);
                }
            }

            let col_key = DataKey::Collateral(borrower.clone());
            let collateral: i128 = env.storage().persistent().get(&col_key).unwrap_or(0);
            let position = load_debt(&env, &borrower);
            let prev_principal = position.principal;
            let now = env.ledger().timestamp();
            let rate = current_borrow_rate(&env);
            let settled_position =
                settle_and_accrue_insurance(&env, &position, now, rate)
                    .map_err(|_| LendingError::Overflow)?;
            save_debt(&env, &borrower, &settled_position);
            let debt = settled_position.principal;
            let accrued_interest = settled_position.principal.saturating_sub(prev_principal);

            // A position with no outstanding debt has nothing to liquidate --
            // treat it as healthy rather than dividing by zero below (mirrors
            // `get_position`'s `debt > 0` guard on the health-factor calc).
            if debt <= 0 {
                let first_unhealthy_key = DataKey::FirstUnhealthyTimestamp(borrower.clone());
                if env.storage().persistent().has(&first_unhealthy_key) {
                    env.storage().persistent().remove(&first_unhealthy_key);
                }
                return Err(LendingError::PositionHealthy);
            }

            let threshold_bps = Self::get_liquidation_threshold_bps(env.clone());
            let collateral_value = value_collateral(&env, collateral)?;
            let debt_value = value_debt(&env, debt)?;
            let hf = math::checked_mul_div_floor(collateral_value, threshold_bps, debt_value)
                .map_err(|_| LendingError::Overflow)?;

            if hf >= HEALTH_FACTOR_SCALE {
                let first_unhealthy_key = DataKey::FirstUnhealthyTimestamp(borrower.clone());
                if env.storage().persistent().has(&first_unhealthy_key) {
                    env.storage().persistent().remove(&first_unhealthy_key);
                }
                return Err(LendingError::PositionHealthy);
            }

            let grace_secs = Self::get_liquidation_grace_period(env.clone());
            if grace_secs > 0 {
                let first_unhealthy_key = DataKey::FirstUnhealthyTimestamp(borrower.clone());
                let first_unhealthy_ts = match env
                    .storage()
                    .persistent()
                    .get::<DataKey, u64>(&first_unhealthy_key)
                {
                    Some(ts) => ts,
                    None => {
                        // Position was observed unhealthy for the first time.
                        // Record the current timestamp and extend TTL.
                        //
                        // Note on Soroban storage rollback:
                        // Because returning `Err(LiquidationGracePeriodActive)` causes the Soroban
                        // host to roll back storage mutations within this transaction, this write
                        // will not persist unless `mark_unhealthy` is called in a separate successful
                        // invocation. Keepers/liquidators are expected to call `mark_unhealthy` first
                        // to establish the grace period start time on-chain.
                        env.storage()
                            .persistent()
                            .set(&first_unhealthy_key, &now);
                        extend_first_unhealthy_ttl(&env, &borrower);
                        return Err(LendingError::LiquidationGracePeriodActive);
                    }
                };

                extend_first_unhealthy_ttl(&env, &borrower);

                if now < first_unhealthy_ts.saturating_add(grace_secs) {
                    return Err(LendingError::LiquidationGracePeriodActive);
                }
            }

            let close_factor_bps = Self::close_factor_bps_config(&env);
            let max_repay = math::checked_mul_div_floor(debt, close_factor_bps, BPS_DENOM)
                .map_err(|_| LendingError::Overflow)?;
            let actual_repay = if amount > max_repay {
                max_repay
            } else {
                amount
            };

            if actual_repay <= 0 {
                return Err(LendingError::InvalidAmount);
            }

            let incentive_bps = Self::liquidation_incentive_bps_config(&env);
            let seized_collateral = if uses_cross_asset_valuation(&env) {
                let collateral_asset: Address = env
                    .storage()
                    .instance()
                    .get(&DataKey::ValuationCollateralAsset)
                    .ok_or(LendingError::PriceUnavailable)?;
                let debt_asset: Address = env
                    .storage()
                    .instance()
                    .get(&DataKey::ValuationDebtAsset)
                    .ok_or(LendingError::PriceUnavailable)?;
                let collateral_price =
                    cross_asset::get_price_for_asset(&env, &collateral_asset)?.price;
                let debt_price = cross_asset::get_price_for_asset(&env, &debt_asset)?.price;
                if collateral_price <= 0 || debt_price <= 0 {
                    return Err(LendingError::PriceUnavailable);
                }
                let numerator = actual_repay
                    .checked_mul(debt_price)
                    .and_then(|value| value.checked_mul(BPS_DENOM + incentive_bps))
                    .ok_or(LendingError::Overflow)?;
                let denominator = collateral_price
                    .checked_mul(BPS_DENOM)
                    .ok_or(LendingError::Overflow)?;
                if denominator == 0 {
                    return Err(LendingError::PriceUnavailable);
                }
                numerator
                    .checked_div(denominator)
                    .ok_or(LendingError::Overflow)?
            } else {
                math::checked_mul_div_floor(actual_repay, BPS_DENOM + incentive_bps, BPS_DENOM)
                    .map_err(|_| LendingError::Overflow)?
            };

            let available_collateral = collateral;
            let mut final_seized = seized_collateral;
            if seized_collateral > available_collateral {
                let shortfall = seized_collateral
                    .checked_sub(available_collateral)
                    .ok_or(LendingError::Overflow)?;
                let insurance_drawn = draw_insurance(&env, shortfall)?;
                let residual = shortfall
                    .checked_sub(insurance_drawn)
                    .ok_or(LendingError::Overflow)?;
                if residual > 0 {
                    let current_bad_debt: i128 = env
                        .storage()
                        .persistent()
                        .get(&DataKey::BadDebt)
                        .unwrap_or(0i128);
                    let new_bad_debt = current_bad_debt
                        .checked_add(residual)
                        .ok_or(LendingError::Overflow)?;
                    env.storage()
                        .persistent()
                        .set(&DataKey::BadDebt, &new_bad_debt);
                    crate::ttl::bump_global(&env, &DataKey::BadDebt);
                    BadDebtEvent {
                        borrower: borrower.clone(),
                        amount: residual,
                    }
                    .publish(&env);
                }
                final_seized = available_collateral;
            }

            let new_debt = debt
                .checked_sub(actual_repay)
                .ok_or(LendingError::Overflow)?;
            let new_col = collateral
                .checked_sub(final_seized)
                .ok_or(LendingError::Overflow)?;

            // Recompute the post-liquidation health factor *before* mutating any
            // storage, using the same governed threshold (`threshold_bps`) and the
            // same checked helper as the entry guard above.
            //
            // Evaluating this predicate with the hardcoded
            // `LIQUIDATION_THRESHOLD_BPS` and a raw `checked_mul` + `/` with
            // `unwrap_or(i128::MAX)` was wrong in two ways:
            //   * governance drift — once an admin governed the threshold down via
            //     `set_liquidation_threshold_bps`, the entry guard and this check
            //     evaluated *different* predicates, so
            //     `DataKey::FirstUnhealthyTimestamp` was cleared or retained on the
            //     wrong one, corrupting the liquidation grace-period state machine;
            //   * fail-open on overflow — an overflow collapsed to `i128::MAX`, i.e.
            //     "infinitely healthy", silently clearing the grace-period
            //     timestamp instead of aborting.
            // Failing here now aborts before any state is written.
            let hf_after = if new_debt > 0 {
                let new_collateral_value = value_collateral(&env, new_col)?;
                let new_debt_value = value_debt(&env, new_debt)?;
                math::checked_mul_div_floor(new_collateral_value, threshold_bps, new_debt_value)
                    .map_err(|_| LendingError::Overflow)?
            } else {
                i128::MAX
            };

            let updated_position = DebtPosition {
                principal: new_debt,
                borrow_index_snapshot: settled_position.borrow_index_snapshot,
                last_update: settled_position.last_update,
            };
            save_debt(&env, &borrower, &updated_position);
            env.storage().persistent().set(&col_key, &new_col);

            let total_debt_key = DataKey::TotalDebt;
            let total_deposits_key = DataKey::TotalDeposits;

            if env.storage().persistent().has(&total_debt_key) {
                let total_debt: i128 = env
                    .storage()
                    .persistent()
                    .get(&total_debt_key)
                    .unwrap_or(0);
                let total_debt_with_interest = total_debt
                    .checked_add(accrued_interest)
                    .ok_or(LendingError::Overflow)?;
                let new_total_debt = total_debt_with_interest
                    .checked_sub(actual_repay)
                    .ok_or(LendingError::Overflow)?;
                env.storage()
                    .persistent()
                    .set(&total_debt_key, &new_total_debt);
            }

            if env.storage().persistent().has(&total_deposits_key) {
                let total_deposits: i128 = env
                    .storage()
                    .persistent()
                    .get(&total_deposits_key)
                    .unwrap_or(0);
                let new_total_deposits = total_deposits
                    .checked_sub(final_seized)
                    .ok_or(LendingError::Overflow)?;
                env.storage()
                    .persistent()
                    .set(&total_deposits_key, &new_total_deposits);
            }

            #[cfg(test)]
            {
                if let Some(borrowers) = env
                    .storage()
                    .instance()
                    .get::<DataKey, soroban_sdk::Vec<Address>>(&DataKey::BorrowerList)
                {
                    let mut sum_debt: i128 = 0;
                    for i in 0..borrowers.len() {
                        let u = borrowers.get(i).unwrap();
                        sum_debt = sum_debt.saturating_add(load_debt(&env, &u).principal);
                    }
                    if let Some(td) = env
                        .storage()
                        .persistent()
                        .get::<DataKey, i128>(&total_debt_key)
                    {
                        debug_assert_eq!(sum_debt, td, "sum(user debt) == TotalDebt");
                    }
                }
            }

            // A liquidation repays the same principal that is removed from
            // the position. Release that amount exactly once from any
            // isolation bucket attributed to the collateral asset.
            if should_release_isolation_debt(&env, &collateral_asset) {
                decrement_isolation_debt(&env, &collateral_asset, actual_repay)?;
            }

            if hf_after >= HEALTH_FACTOR_SCALE {
                let first_unhealthy_key = DataKey::FirstUnhealthyTimestamp(borrower.clone());
                if env.storage().persistent().has(&first_unhealthy_key) {
                    env.storage().persistent().remove(&first_unhealthy_key);
                }
            }

            let debt_token_client = TokenClient::new(&env, &debt_asset);
            let collateral_token_client = TokenClient::new(&env, &collateral_asset);
            debt_token_client.transfer(&liquidator, env.current_contract_address(), &actual_repay);
            collateral_token_client.transfer(
                &env.current_contract_address(),
                &liquidator,
                &final_seized,
            );

            let shortfall = seized_collateral.saturating_sub(final_seized);

            LiquidationEventV1 {
                schema_version: SCHEMA_VERSION_V1,
                liquidator: liquidator.clone(),
                borrower: borrower.clone(),
                repaid: actual_repay,
                seized: final_seized,
                health_factor_before: hf,
                shortfall,
            }
            .publish(&env);

            Ok(actual_repay)
        })
    }

    // Liquidate an under-collateralised *cross-asset* position.
    // 
    // Cross-asset positions are opened via [`deposit_collateral_asset`] /
    // [`borrow_asset`].  The legacy [`liquidate`] entrypoint only reads the
    // single-asset `Collateral`/`Debt` keys and therefore always returns
    // [`LendingError::PositionHealthy`] for pure cross-asset users.  This
    // entrypoint reads the per-`(user, asset)` keys and uses the aggregate
    // health factor.
    // 
    // # Liquidation arithmetic
    // 
    // ```text
    // close_factor_cap   = debt(debt_asset) × close_factor_bps / 10_000
    // actual_repay       = min(repay_amount, close_factor_cap)
    // seized_collateral  = actual_repay × price(debt_asset) × (10_000 + incentive_bps)
    //                      / (price(collateral_asset) × 10_000)
    //                                                            [floor rounding]
    // ```
    // 
    // All three multiplications use [`math::checked_mul_div_floor`] so every
    // sub-unit remainder favours the protocol.
    // 
    // # Shortfall
    // 
    // When `seized_collateral > available_collateral(collateral_asset)` the
    // deficit is drawn from the insurance fund first; any residual is posted
    // as bad debt (`DataKey::BadDebt`).
    // 
    // # Token transfers
    // 
    // Follows the same pattern as [`liquidate`]:
    // - `liquidator` transfers `actual_repay` of `debt_asset` into the contract.
    // - The contract transfers `final_seized` of `collateral_asset` to `liquidator`.
    // 
    // # Guards
    // - Self-liquidation returns [`LendingError::SelfLiquidation`].
    // - Active flash loan returns immediately (reentrancy guard).
    // - Both chosen assets must have fresh oracle prices; so must every other
    //   leg on the position (fail-closed on partial oracle staleness).
    // - Price of zero or negative is rejected as [`LendingError::PriceFeedNotFound`].
    // 
    // # Errors
    // - [`LendingError::SelfLiquidation`] — `liquidator == borrower`.
    // - [`LendingError::PositionHealthy`] — aggregate HF ≥ [`HEALTH_FACTOR_SCALE`].
    // - [`LendingError::StaleOracleTimestamp`] — any leg has a stale price.
    // - [`LendingError::PriceFeedNotFound`] — any leg has a missing or non-positive price.
    // - [`LendingError::InvalidAmount`] — `repay_amount ≤ 0` or the computed
    //   `actual_repay` would be zero after the close-factor cap.
    // - [`LendingError::AssetNotConfigured`] — `debt_asset` or `collateral_asset`
    //   has not been configured via `set_asset_params`.
    // - [`LendingError::Overflow`] — arithmetic overflow.
    pub(crate) fn liquidate_cross_asset(
        env: Env,
        liquidator: Address,
        borrower: Address,
        debt_asset: Address,
        collateral_asset: Address,
        repay_amount: i128,
    ) -> Result<i128, LendingError> {
        require_initialized(&env)?;
        with_reentrancy_lock(&env, || {
            liquidator.require_auth();

            // ── 1. Self-liquidation guard ──────────────────────────────────
            if liquidator == borrower {
                return Err(LendingError::SelfLiquidation);
            }

            check_pause_status(&env, ProtocolAction::Liquidate);
            require_no_active_flash_loan(&env)?;

            // ── 2. Validate both assets are configured ─────────────────────
            // (also implicitly validates they exist before any price lookup)
            cross_asset::validate_asset_params_configured(&env, &debt_asset)?;
            cross_asset::validate_asset_params_configured(&env, &collateral_asset)?;

            if repay_amount <= 0 {
                return Err(LendingError::InvalidAmount);
            }

            // ── 3. Require fresh prices for every leg (fail-closed) ────────
            cross_asset::ensure_position_prices_fresh_for_liquidation(&env, &borrower)?;

            // ── 4. Compute aggregate health factor ─────────────────────────
            let hf = cross_asset::compute_aggregate_health_factor(&env, &borrower)?;

            if hf >= HEALTH_FACTOR_SCALE {
                return Err(LendingError::PositionHealthy);
            }

            // ── 5. Resolve oracle prices for the two chosen legs ───────────
            let debt_price_record = cross_asset::get_price_for_asset(&env, &debt_asset)?;
            let col_price_record = cross_asset::get_price_for_asset(&env, &collateral_asset)?;
            // Prices guaranteed > 0 by get_price_for_asset; guard explicitly
            // anyway to prevent divide-by-zero later.
            if debt_price_record.price <= 0 || col_price_record.price <= 0 {
                return Err(LendingError::PriceFeedNotFound);
            }

            // ── 6. Accrue interest on the debt leg ────────────────────────
            let now = env.ledger().timestamp();
            let position = cross_asset::load_debt_asset(&env, &borrower, &debt_asset);
            let settled_position =
                cross_asset::settle_asset_position(&env, &debt_asset, &position, now)?;

            let debt = settled_position.principal;
            if debt <= 0 {
                return Err(LendingError::PositionHealthy);
            }

            // ── 7. Apply close-factor cap ──────────────────────────────────
            let close_factor_bps = Self::close_factor_bps_config(&env);
            let max_repay = math::checked_mul_div_floor(debt, close_factor_bps, BPS_DENOM)
                .map_err(|_| LendingError::Overflow)?;
            let actual_repay = repay_amount.min(max_repay);
            if actual_repay <= 0 {
                return Err(LendingError::InvalidAmount);
            }

            // ── 8. Compute seized collateral (with incentive, floor rounding)
            //
            //   seized = actual_repay × price_debt × (BPS_DENOM + incentive)
            //            / (price_col × BPS_DENOM)
            //
            // Use two sequential checked_mul_div_floor calls to keep
            // intermediate values within i128 range (both prices ≤ 10^14,
            // typical repay ≤ 10^14, so repay × price_debt can reach ~10^28 —
            // beyond i128).  Split the division: first divide by price_col,
            // then apply the incentive.
            let incentive_bps = Self::liquidation_incentive_bps_config(&env);

            // Step A: repay_amount_in_collateral_units = actual_repay × price_debt / price_col
            let repay_in_col_units =
                math::checked_mul_div_floor(actual_repay, debt_price_record.price, col_price_record.price)
                    .map_err(|_| LendingError::Overflow)?;

            // Step B: apply incentive — seized = repay_in_col_units × (BPS_DENOM + incentive) / BPS_DENOM
            let seized_collateral =
                math::checked_mul_div_floor(repay_in_col_units, BPS_DENOM + incentive_bps, BPS_DENOM)
                    .map_err(|_| LendingError::Overflow)?;

            // ── 9. Cap seized to available collateral; handle shortfall ────
            let available_collateral =
                cross_asset::load_collateral_asset(&env, &borrower, &collateral_asset);
            let mut final_seized = seized_collateral;

            if seized_collateral > available_collateral {
                let shortfall = seized_collateral
                    .checked_sub(available_collateral)
                    .ok_or(LendingError::Overflow)?;
                let insurance_drawn = draw_insurance(&env, shortfall)?;
                let residual = shortfall
                    .checked_sub(insurance_drawn)
                    .ok_or(LendingError::Overflow)?;
                if residual > 0 {
                    let current_bad_debt: i128 = env
                        .storage()
                        .persistent()
                        .get(&DataKey::BadDebt)
                        .unwrap_or(0i128);
                    let new_bad_debt = current_bad_debt
                        .checked_add(residual)
                        .ok_or(LendingError::Overflow)?;
                    env.storage()
                        .persistent()
                        .set(&DataKey::BadDebt, &new_bad_debt);
                    crate::ttl::bump_global(&env, &DataKey::BadDebt);
                    emit_bad_debt_increased(&env, &borrower, residual, new_bad_debt);
                }
                final_seized = available_collateral;
            }

            // ── 10. Write updated debt position ───────────────────────────
            let new_debt = debt
                .checked_sub(actual_repay)
                .ok_or(LendingError::Overflow)?;
            let updated_debt_position = crate::debt::DebtPosition {
                principal: new_debt,
                borrow_index_snapshot: settled_position.borrow_index_snapshot,
                last_update: settled_position.last_update,
            };
            cross_asset::save_debt_asset(&env, &borrower, &debt_asset, &updated_debt_position);
            if new_debt == 0 {
                cross_asset::remove_from_user_debt_list(&env, &borrower, &debt_asset);
            }

            // Update the per-asset total debt accumulator by the full change in
            // the borrower's stored principal (settled interest plus repayment),
            // so `Σ user debt(asset) == TotalDebtAsset(asset)` keeps holding.
            // The global `TotalDebt` is deliberately not touched: this debt is
            // denominated in `debt_asset`'s own units.
            let total_debt_asset: i128 = env
                .storage()
                .persistent()
                .get(&DataKey::TotalDebtAsset(debt_asset.clone()))
                .unwrap_or(0);
            let principal_delta = updated_debt_position
                .principal
                .checked_sub(position.principal)
                .ok_or(LendingError::Overflow)?;
            let new_total_debt_asset = total_debt_asset
                .checked_add(principal_delta)
                .ok_or(LendingError::Overflow)?;
            if new_total_debt_asset < 0 {
                return Err(LendingError::Overflow);
            }
            env.storage()
                .persistent()
                .set(&DataKey::TotalDebtAsset(debt_asset.clone()), &new_total_debt_asset);
            crate::ttl::bump_global(&env, &DataKey::TotalDebtAsset(debt_asset.clone()));

            cross_asset::extend_debt_asset_ttl(&env, &borrower, &debt_asset);

            // ── 11. Write updated collateral balance ──────────────────────
            let new_col = available_collateral
                .checked_sub(final_seized)
                .ok_or(LendingError::Overflow)?;
            cross_asset::save_collateral_asset(&env, &borrower, &collateral_asset, new_col);
            // Seized collateral leaves the borrower, so reduce the per-asset
            // supplied total that feeds utilisation. Saturate for legacy state
            // where the counter was never populated.
            let total_supplied: i128 = env
                .storage()
                .persistent()
                .get(&DataKey::TotalCollateralAsset(collateral_asset.clone()))
                .unwrap_or(0);
            env.storage().persistent().set(
                &DataKey::TotalCollateralAsset(collateral_asset.clone()),
                &total_supplied.saturating_sub(final_seized),
            );
            crate::ttl::bump_global(&env, &DataKey::TotalCollateralAsset(collateral_asset.clone()));
            if new_col == 0 {
                cross_asset::remove_from_user_collateral_list(
                    &env,
                    &borrower,
                    &collateral_asset,
                );
            } else {
                cross_asset::extend_collateral_asset_ttl(&env, &borrower, &collateral_asset);
            }

            // Seized collateral leaves the per-asset supplied total. Saturate at
            // zero like `withdraw_collateral_asset_internal`: legacy deployments
            // may under-record this counter and must not brick liquidations.
            let total_col_asset = cross_asset::load_total_collateral_asset(&env, &collateral_asset);
            let new_total_col_asset = total_col_asset.saturating_sub(final_seized);
            env.storage().persistent().set(
                &DataKey::TotalCollateralAsset(collateral_asset.clone()),
                &new_total_col_asset,
            );
            crate::ttl::bump_global(&env, &DataKey::TotalCollateralAsset(collateral_asset.clone()));

            // ── 12. Release isolation bucket if applicable ────────────────
            if should_release_isolation_debt(&env, &collateral_asset) {
                decrement_isolation_debt(&env, &collateral_asset, actual_repay)?;
            }

            // ── 13. Clear unhealthy timestamp when position is healed ─────
            let hf_after = cross_asset::compute_aggregate_health_factor(&env, &borrower)
                .unwrap_or(0);
            if hf_after >= HEALTH_FACTOR_SCALE {
                let first_unhealthy_key =
                    DataKey::FirstUnhealthyTimestamp(borrower.clone());
                if env.storage().persistent().has(&first_unhealthy_key) {
                    env.storage().persistent().remove(&first_unhealthy_key);
                }
            }

            // ── 14. Token transfers (checks-effects-interactions) ─────────
            let debt_token_client = TokenClient::new(&env, &debt_asset);
            let collateral_token_client = TokenClient::new(&env, &collateral_asset);
            debt_token_client.transfer(
                &liquidator,
                &env.current_contract_address(),
                &actual_repay,
            );
            collateral_token_client.transfer(
                &env.current_contract_address(),
                &liquidator,
                &final_seized,
            );

            // ── 15. Emit event ────────────────────────────────────────────
            let shortfall = seized_collateral.saturating_sub(final_seized);
            CrossLiquidationEvent {
                schema_version: SCHEMA_VERSION_V1,
                liquidator: liquidator.clone(),
                borrower: borrower.clone(),
                debt_asset: debt_asset.clone(),
                collateral_asset: collateral_asset.clone(),
                repaid: actual_repay,
                seized: final_seized,
                health_factor_before: hf,
                shortfall,
            }
            .publish(&env);

            Ok(actual_repay)
        })
    }
}
