# StellarLend Protocol Documentation

This repository contains the Soroban smart contracts and related resources for the StellarLend protocol.

Contents:
- Overview
- Modules and Features
- Admin Operations
- Monitoring & Analytics
- Upgrade & Configuration
- Cross-Chain Bridge
- Social Recovery & Multisig

## Overview
StellarLend is a lending and borrowing protocol built on Soroban. It features cross-asset accounting, risk management, governance, AMM integration, flash loans, and more.

## Modules and Features
- Interest rate model with smoothing
- Risk config and scoring
- Cross-asset positions and oracle support
- Flash loans with configurable fees
- AMM integration hooks (swap, add/remove liquidity)
- Cross-chain bridge interface with fees and events
- Analytics: global and per-user metrics, daily snapshots
- Monitoring: health, performance, and security alerts
- Social recovery: guardians, timelock approvals, execution
- Multisig for admin min-collateral changes
- Upgrade: propose/approve/execute/rollback and status
- Data management: generic data store, backup/restore, migration
- Configuration: versioned param store with backup/restore

## Admin Operations
Key admin entrypoints (see contract for full list):
- `initialize(admin)`
- `set_min_collateral_ratio(caller, ratio)`
- `set_risk_params(...)`, `set_pause_switches(...)`
- `set_price_cache_ttl(caller, ttl)`
- `register_bridge(caller, network_id, bridge, fee_bps)`
- `set_bridge_fee(caller, network_id, fee_bps)`
- `upgrade_propose/approve/execute/rollback`
- `config_set/config_backup/config_restore`
- `ms_set_admins`, `ms_propose_set_min_cr`, `ms_approve`, `ms_execute`

## Monitoring & Analytics
- `record_user_action(user, action)` updates risk and emits events
- Analytics auto-update on deposit/borrow/repay/withdraw
- Monitoring entrypoints: `monitor_report_health/performance/security`, `monitor_get`

### Analytics Read APIs
- `get_protocol_report()` & `get_user_report(address)` surface typed structs (`ProtocolReport`, `UserReport`) containing
  current metrics, active-user counts, and the latest activity feed snapshot time.
- `get_asset_report(asset)` returns `AssetReport` with per-asset analytics and historical bucketed data.
- `get_recent_activity(limit)` supplies an `ActivityFeed` with newest-first entries, a `total_available` counter
  (capped at 1,000 retained records), and the `generated_at` ledger timestamp for indexers.
- Activity entries include `user`, `activity_type`, `amount`, optional `asset`, and a metadata map for extended tags.
- Example payloads: [`protocol_report.json`](examples/protocol_report.json) and
  [`user_report.json`](examples/user_report.json) demonstrate the serialized shape returned by the contract. Monetary
  totals are raw integers in the protocol’s smallest units, and percentage-style fields (e.g., utilization, success
  rate, health score) are exported as fixed-point integers scaled by `1e6` (`333333` ≈ 33.3333%).
- Field hints:
  - `total_value_locked`, `total_deposits`, `total_borrows`, etc. are cumulative atomic units across all assets.
  - Percentage-like values (e.g., `avg_utilization_rate`, `protocol_risk_score`, `uptime_percentage`,
    `collateralization_ratio`) use the same `1e6` scaling; divide by 1_000_000 to obtain human-readable percentages.
  - User analytics expose running totals plus derived scores (`activity_score`, `loyalty_tier`) that map to reward
    tiers.
- Example Soroban call:
  ```sh
  soroban contract invoke \
    --id <contract-id> \
    --fn get_recent_activity \
    --arg limit=50
  ```
  Returns a feed where `entries[0]` is the most recent action; set `limit` to `0` for metadata-only responses.

## Upgrade & Configuration
- `upgrade_status` returns current, previous, pending version and metadata
- Config supports version bumps, validation, and easy backup/restore

## Cross-Chain Bridge
- Register networks and fees, and use `bridge_deposit/bridge_withdraw` to move balances with transparent fees

## Social Recovery & Multisig
- Set guardians per-user and execute timelocked recoveries
- Multisig supports proposing and executing admin changes with threshold
