# Oracle Configuration Management Guide

## Overview

This document outlines the procedures for managing oracle configurations in the StellarLend protocol, including role separation, security considerations, and operational guidelines.

## Architecture Overview

### Components

1. **Off-chain Oracle Service** (TypeScript/Node.js)
   - Fetches prices from multiple external sources
   - Aggregates and validates price data
   - Updates smart contract with validated prices

2. **Smart Contract Oracle Module** (Rust/Soroban)
   - Stores on-chain price feeds
   - Enforces validation rules and role separation
   - Manages oracle configuration and permissions

3. **Price Providers**
   - CoinGecko (primary, 60% weight)
   - Binance (secondary, 40% weight)
   - CoinMarketCap (optional, 35% weight)

## Role-Based Access Control

### Roles and Permissions

| Role | Permissions | Responsibilities |
|------|-------------|------------------|
| **Admin** | - Configure oracle parameters<br>- Set/remove primary oracles<br>- Set/remove fallback oracles<br>- Pause/resume oracle updates<br>- Configure the oracle signing key | - System configuration<br>- Oracle management<br>- Emergency operations |
| **Primary Oracle** | - Update prices for registered assets<br>- Read price feeds | - Regular price updates<br>- Market data provision |
| **Fallback Oracle** | - Update prices when primary is stale<br>- Read fallback price feeds | - Backup price provision<br>- Redundancy support |
| **Public/Other** | - Read price feeds only | - Price consumption |

### Authorization Flow

1. **Admin Operations**: Require admin address verification
2. **Oracle Operations**: Verify oracle is registered for the asset
3. **Price Updates**: Any relayer may submit a payload signed by the configured Ed25519 oracle key
4. **Configuration Changes**: Admin-only with additional validation

## Price Move-Cap Circuit Breaker

### Overview

A single compromised oracle key can push an outlier price quote in one block,
triggering mass liquidations or enabling under-collateralised borrows. The
**max-move-bps** guard limits how far the stored price may move in a single
`set_price` call, bounding the blast radius of any one bad update.

### How It Works

| Condition | Behaviour |
|-----------|-----------|
| `MaxMoveBps` not set | No move limit — any valid price is accepted (default / backward-compatible). |
| `MaxMoveBps` set, **no** prior `PriceRecord` for the asset | First-ever price is **exempt** — accepted unconditionally. |
| `MaxMoveBps` set, prior record exists | Move is checked: `\|new − old\| × 10 000 / old ≤ max_move_bps`. Exceeding the cap returns `MaxMoveBpsExceeded (5005)`. |

### Configuration Functions

| Function | Access | Description |
|----------|--------|-------------|
| `set_max_move_bps(env, max_move_bps)` | Admin only | Sets the cap in basis points (500 = 5%). Pass `0` to disable the cap without removing the key. |
| `get_max_move_bps(env)` | Public | Returns `Some(bps)` if configured, `None` if never set. |

### Error Codes

| Code | Name | Meaning |
|------|------|---------|
| `5005` | `MaxMoveBpsExceeded` | Proposed price moves more than `max_move_bps` basis points from the last stored price. |

### Recommended Settings

| Risk Tier | `max_move_bps` | Max single-update move |
|-----------|---------------|------------------------|
| Conservative | 200 | 2 % |
| Standard | 500 | 5 % |
| Permissive | 1 000 | 10 % |
| Disabled | not set / 0 | unlimited |

### Security Notes

* The guard uses **checked arithmetic** throughout; overflow returns `Overflow (1002)`.
* The check occurs **after** signature verification but **before** storage write, so a
  rejected update leaves the stored price unchanged.
* Decreasing the cap takes effect immediately on the next `set_price` call.
* The cap applies **per asset address**; different assets may have different implicit
  volatility profiles but currently share one global setting.

---

## Oracle Hardening (per-asset staleness, windowed deviation, relayer role)

This section documents the on-chain oracle trust-boundary hardening. It is
authoritative; where the rest of this document describes aspirational entry
points (`set_primary_oracle`, `configure_oracle`, …), prefer the functions named
here.

### 1. Per-asset staleness window

Price freshness was previously hard-coded to 3600 s for every asset. It is now
configurable per asset.

| Function | Access | Description |
|---|---|---|
| `set_oracle_max_age(env, asset, secs)` | Admin only | Sets the max accepted age for `asset`. Bounds: `60 ..= 86_400` seconds. Emits `OracleMaxAgeSetEvent { asset, max_age_secs }`. |
| `get_oracle_max_age(env, asset)` | Public | Returns `Some(secs)` if overridden, `None` when the protocol default (3600 s) applies. |

`secs == 0` is rejected with `InvalidAmount (1001)`: a zero max age would mark
every price instantly stale and brick borrowing.

Every freshness check honours the override:

* `set_price` timestamp validation
* `require_fresh_valuation_prices` / `require_fresh_price_for_key` (borrow,
  withdraw, liquidate gates)
* `cross_asset::get_price_for_asset`
* `position_health_factor` (per-leg, using each asset's own window)

A stable asset can keep the 3600 s default while a volatile asset is tightened to
60 s; a stale volatile leg rejects a borrow while the stable leg remains usable.

### 2. Time-windowed deviation guard

The per-update `MaxMoveBps` cap only compares against the *previous* update, so
N updates one second apart can each move `max_move_bps` and walk the price an
arbitrary distance. A windowed anchor closes that gap.

| Function | Access | Description |
|---|---|---|
| `set_move_window_secs(env, secs)` | Admin only | Window length for the deviation guard. Bounds: `1 ..= 86_400` seconds (default 3600). Emits `MoveWindowSetEvent { move_window_secs }`. |
| `get_move_window_secs(env)` | Public | Returns the configured window length. |

For each asset the contract stores an `OracleAnchor { price, timestamp }`
(under `DataKey::OracleAnchor(asset)`). On each `set_price`:

1. If no anchor exists (first price) or the window has elapsed since the anchor
   timestamp, the anchor is **reseeded** at the newly accepted price.
2. Otherwise the proposed price must be within `max_move_bps` of the anchor
   price, else `MaxMoveBpsExceeded (5004)`.

The per-update cap is still enforced on top of the windowed guard. With no
`MaxMoveBps` configured, the guard is a no-op (backward compatible).

### 3. Price relayer role

`set_price` no longer requires the admin key, so an always-online oracle process
never needs to hold it.

| Function | Access | Description |
|---|---|---|
| `set_price_relayer(env, relayer)` | Admin only | Designates the address permitted (alongside the admin) to relay signed prices. Emits `PriceRelayerSetEvent { relayer }`. |
| `get_price_relayer(env)` | Public | Returns `Some(address)` if configured. |

`set_price(env, caller, asset, price, timestamp, signature)` requires
`caller == relayer || caller == admin` and `caller.require_auth()`; any other
address fails with `Unauthorized (1003)`. The ed25519 signature over the
canonical payload (unchanged, `StellarLendOracle:v2`) is still verified against
the configured oracle pubkey, so the relayer role only gates *who may submit* an
otherwise-valid update.

#### Off-chain updater compatibility

The signature **payload format is unchanged**, so the off-chain signer in
`oracle/` keeps working. The only required config change is on the operator side:
point the on-chain `set_price` invocation's `caller` at the address registered
via `set_price_relayer`, and ensure that address (not the admin) signs the
transaction.

### 4. Price-bounds validation

`set_price_bounds(env, asset, min_price, max_price, force)` now takes a `force`
flag. When a price is already stored for `asset`, the new bounds must contain it,
otherwise the call fails with `PriceOutOfBounds (3004)`; passing `force = true`
explicitly overrides this (for example when the stored price is known to be wrong
and will be corrected immediately). Bounds with no stored price are always
accepted.

### Error codes

| Code | Name | Meaning |
|------|------|---------|
| `1001` | `InvalidAmount` | Max age / window out of range, or bad bounds ordering. |
| `1002` | `Overflow` | `delta * 10_000` overflowed `i128` during a move check. |
| `1003` | `Unauthorized` | `set_price` caller is neither relayer nor admin. |
| `3004` | `PriceOutOfBounds` | Bounds exclude the current price (and `force` was not set). |
| `5002` | `StaleOracleTimestamp` | Update timestamp is in the future or older than the per-asset max age. |
| `5004` | `MaxMoveBpsExceeded` | Per-update or windowed deviation bound exceeded. |

### Regression coverage

`stellar-lend/contracts/lending/tests/oracle_hardening_test.rs` (integration
tests, run with
`cargo test -p stellarlend-lending --test oracle_hardening_test`) covers:
`per_asset_max_age`, `walk_attack_blocked`, `window_resets_anchor`,
`relayer_role`, `bounds_must_contain_current`, `first_price_still_allowed`,
future-timestamp rejection, idempotent replay, and overflow on the move
calculation.

---

## Configuration Parameters

### Oracle Safety Parameters

```rust
pub struct OracleConfig {
    /// Maximum price deviation in basis points (e.g., 500 = 5%)
    pub max_deviation_bps: i128,
    /// Maximum staleness in seconds
    pub max_staleness_seconds: u64,
    /// Cache TTL in seconds
    pub cache_ttl_seconds: u64,
    /// Minimum price sanity check
    pub min_price: i128,
    /// Maximum price sanity check
    pub max_price: i128,
}
```

### Provider Configuration

```typescript
interface ProviderConfig {
    name: string;
    enabled: boolean;
    priority: number;
    weight: number;
    apiKey?: string;
    baseUrl: string;
    rateLimit: {
        maxRequests: number;
        windowMs: number;
    };
}
```

## Configuration Procedures

### 1. Initial Oracle Setup

#### Prerequisites
- Admin privileges
- Oracle addresses (generated)
- Asset addresses
- Configuration parameters determined

#### Steps

1. **Configure Oracle Parameters**
```bash
# Set conservative initial parameters
max_deviation_bps: 500 (5%)
max_staleness_seconds: 3600 (1 hour)
cache_ttl_seconds: 300 (5 minutes)
min_price: 1
max_price: i128::MAX
```

2. **Set Primary Oracle**
```bash
# For each asset
contract.set_primary_oracle(admin, asset_address, oracle_address)
```

3. **Set Fallback Oracle** (optional but recommended)
```bash
contract.set_fallback_oracle(admin, asset_address, fallback_oracle_address)
```

4. **Initial Price Feed**
```bash
contract.update_price_feed(admin, asset_address, price, decimals, oracle_address)
```

### 2. Switching Primary Oracle

#### When to Switch
- Oracle provider compromise
- Long-term oracle unavailability
- Provider quality degradation
- Strategic provider changes

#### Procedure

1. **Prepare New Oracle**
```bash
# Generate new oracle address
# Verify oracle operational status
# Test oracle connectivity
```

2. **Update Configuration**
```bash
# Set new primary oracle
contract.set_primary_oracle(admin, asset_address, new_oracle_address)
```

3. **Verify Switch**
```bash
# Check oracle registration
primary_oracle = contract.get_primary_oracle(asset_address)
assert(primary_oracle == new_oracle_address)
```

4. **Update Price Feed**
```bash
# Admin updates price with new oracle
contract.update_price_feed(admin, asset_address, price, decimals, new_oracle_address)
```

5. **Monitor Operation**
```bash
# Verify new oracle can update prices
contract.update_price_feed(new_oracle_address, asset_address, price, decimals, new_oracle_address)
```

### 3. Adjusting Safety Parameters

#### Risk Assessment

| Parameter | Conservative | Moderate | Aggressive |
|-----------|-------------|----------|------------|
| max_deviation_bps | 200 (2%) | 500 (5%) | 1000 (10%) |
| max_staleness_seconds | 1800 (30min) | 3600 (1hr) | 7200 (2hr) |
| cache_ttl_seconds | 60 (1min) | 300 (5min) | 600 (10min) |

#### Procedure

1. **Assess Market Conditions**
```bash
# Analyze price volatility
# Consider asset characteristics
# Evaluate risk tolerance
```

2. **Update Configuration**
```bash
new_config = OracleConfig {
    max_deviation_bps: new_value,
    max_staleness_seconds: new_value,
    cache_ttl_seconds: new_value,
    min_price: current_min_price,
    max_price: current_max_price,
}

contract.configure_oracle(admin, new_config)
```

3. **Validate Configuration**
```bash
# Test with sample price updates
# Verify deviation limits work
# Check staleness enforcement
```

#### Tested Boundary Behaviour

The lending contract uses `DEFAULT_ORACLE_MAX_AGE_SECS = 3600` unless a
per-asset override has been set via `set_oracle_max_age` (see the hardening
section above). The oracle-consumption paths in `borrow` and `liquidate` enforce
the effective boundary at the point of use:

- `age <= 3600` seconds: accepted
- `age == 3601` seconds: rejected with `LendingError::StaleOracleTimestamp`

The regression coverage in
`stellar-lend/contracts/lending/src/oracle_staleness_test.rs` verifies both
edges and checks each configured valuation asset independently by refreshing one
asset while intentionally leaving the other stale.

### 4. Emergency Procedures

#### Oracle Compromise Response

1. **Immediate Actions**
```bash
# Pause oracle updates
contract.pause_oracle_updates(admin)

# Remove compromised oracle
contract.set_primary_oracle(admin, asset_address, zero_address)
```

2. **Activate Fallback**
```bash
# Ensure fallback oracle is active
# Verify fallback oracle integrity
# Promote fallback if necessary
```

3. **Recovery**
```bash
# Deploy new oracle
# Update oracle registration
# Resume operations
contract.unpause_oracle_updates(admin)
```

#### Market Extreme Volatility

1. **Tighten Parameters**
```bash
# Reduce deviation threshold
max_deviation_bps = 200 (2%)

# Reduce staleness tolerance
max_staleness_seconds = 1800 (30 minutes)
```

2. **Increase Monitoring**
```bash
# More frequent price checks
# Manual price verification
# Consider temporary pause
```

## Security Considerations

### Access Control

1. **Admin Key Security**
   - Use multi-sig when possible
   - Store admin key securely
   - Rotate admin keys periodically
   - Limit admin key usage

2. **Oracle Key Security**
   - Separate keys for each oracle
   - Regular key rotation
   - Secure key storage
   - Access logging

### Validation Security

1. **Price Deviation Checks**
   - Always enforce deviation limits
   - Consider market conditions
   - Monitor for manipulation attempts
   - Alert on suspicious changes

2. **Staleness Protection**
   - Regular staleness checks
   - Fallback oracle activation
   - Manual intervention capability
   - Time synchronization

### Operational Security

1. **Provider Diversity**
   - Multiple independent sources
   - Geographic distribution
   - Different API providers
   - Failover mechanisms

2. **Rate Limiting**
   - Respect provider limits
   - Implement backoff strategies
   - Monitor API usage
   - Prevent abuse

## Monitoring and Alerting

### Key Metrics

1. **Price Update Frequency**
   - Time between updates
   - Update success rate
   - Failed update attempts
   - Provider response times

2. **Price Quality**
   - Deviation from expected
   - Cross-provider consistency
   - Staleness duration
   - Validation failures

3. **System Health**
   - Oracle availability
   - Provider status
   - Error rates
   - Performance metrics

### Alert Conditions

1. **Critical Alerts**
   - Oracle update failures > 5 minutes
   - Price deviation exceedance
   - Stale price detection
   - Configuration changes

2. **Warning Alerts**
   - High latency responses
   - Provider degradation
   - Near-limit rate usage
   - Unusual price patterns

## Testing Procedures

### Configuration Testing

1. **Unit Tests**
   - Parameter validation
   - Authorization checks
   - Edge case handling
   - Error conditions

2. **Integration Tests**
   - End-to-end flows
   - Provider switching
   - Failover scenarios
   - Performance testing

3. **Security Tests**
   - Unauthorized access attempts
   - Manipulation resistance
   - Parameter boundary testing
   - Role separation verification

### Operational Testing

1. **Disaster Recovery**
   - Oracle failure simulation
   - Provider outage testing
   - Configuration rollback
   - Emergency procedures

2. **Load Testing**
   - High update frequency
   - Multiple asset support
   - Concurrent operations
   - Resource limits

## Best Practices

### Configuration Management

1. **Version Control**
   - Track configuration changes
   - Document change reasons
   - Maintain change history
   - Rollback capability

2. **Review Process**
   - Multi-person review
   - Risk assessment
   - Testing requirements
   - Approval workflow

### Operational Excellence

1. **Gradual Changes**
   - Phase parameter adjustments
   - Monitor impact
   - Rollback capability
   - Communication plan

2. **Documentation**
   - Configuration rationale
   - Operational procedures
   - Emergency contacts
   - Troubleshooting guides

## Troubleshooting

### Common Issues

1. **Price Update Failures**
   - Check oracle authorization
   - Verify price deviation limits
   - Confirm staleness thresholds
   - Review provider status

2. **Configuration Problems**
   - Validate parameter ranges
   - Check admin authorization
   - Verify contract state
   - Review recent changes

3. **Performance Issues**
   - Monitor provider latency
   - Check rate limiting
   - Review cache settings
   - Analyze update frequency

### Diagnostic Commands

```bash
# Check oracle configuration
contract.get_oracle_config()

# Verify oracle registration
contract.get_primary_oracle(asset_address)
contract.get_fallback_oracle(asset_address)

# Check price feed status
contract.get_price(asset_address)

# System health check
contract.health_check()
```

## Compliance and Audit

### Audit Requirements

1. **Configuration Changes**
   - Change timestamps
   - Authorized users
   - Parameter values
   - Change justification

2. **Price Updates**
   - Update timestamps
   - Oracle addresses
   - Price values
   - Validation results

### Reporting

1. **Regular Reports**
   - Configuration status
   - Oracle performance
   - Security metrics
   - Compliance status

2. **Incident Reports**
   - Security events
   - System failures
   - Configuration issues
   - Resolution actions

## Conclusion

Effective oracle configuration management is critical for the security and reliability of the StellarLend protocol. This guide provides the procedures and considerations necessary for maintaining a robust oracle system while ensuring proper role separation and security controls.

Regular review of configurations, continuous monitoring, and adherence to security best practices are essential for maintaining system integrity and protecting user assets.



## AMM TWAP Fallback (Issue #868)

### Overview

When the primary oracle is stale or unavailable, the lending contract automatically
falls back to a Time-Weighted Average Price (TWAP) derived from the on-chain AMM pool.

### Fallback chain

Call external oracle → accept if age ≤ max_staleness_seconds
If stale/absent    → emit OrcStale event, use AMM TWAP
If TWAP has no history → panic (fail-safe, never price on nothing)


### TWAP formula

For a pool with reserves `(R₀, R₁)`, after `Δt` seconds:
price0_cumulative += (R₁ / R₀) × 10¹⁸ × Δt
TWAP over window W = Δprice0_cumulative / W

Divide the result by `10¹⁸` to get the human-readable price.

### Configuration

The `twap_window_secs` field in `OracleConfig` controls the fallback look-back window.

| Window       | Seconds | Use case                        |
|--------------|---------|---------------------------------|
| Minimum      | 25 s    | Testing / low-value positions   |
| Recommended  | 150 s   | Standard liquidation checks     |
| High-value   | 1500 s  | Large / high-value positions    |

### Manipulation resistance

A single flash-loan or block-level swap cannot meaningfully move a 150 s+ TWAP because
the manipulated price only affects one slot out of many. The attacker must hold the
position open across multiple ledger closes, bearing full impermanent loss and
liquidation risk throughout.

### Events emitted

| Event       | Trigger                                  |
|-------------|------------------------------------------|
| `OrcStale`  | Primary oracle age > max_staleness_seconds |
| `OrcFallbk` | TWAP fallback was used for pricing       |

Monitor both events to detect oracle health issues in production.

### Collateral Asset Configuration

To support multi-asset operations or require price availability checks on specific assets:
- **`set_collateral_asset(env, asset)`**: Admin-only method to configure the address of the asset used as collateral.
- **`get_collateral_asset(env)`**: Returns the configured collateral asset address, if any.
- **`set_debt_asset(env, asset)`**: Admin-only method to configure the debt asset used for single-position valuation.
- **`get_debt_asset(env)`**: Returns the configured debt asset address, if any.

When both assets are configured and differ, `borrow`, `withdraw`, and `liquidate` require fresh prices for both assets and value the collateral and debt in the oracle's seven-decimal scale. Liquidation converts the repaid debt value into collateral token units before applying the incentive. If the debt asset is unset or matches the collateral asset, the legacy raw-unit health-factor behavior is retained.

Read-only views remain callable with stale prices. Use `get_position_v2` to inspect its `price_stale` flag; the view reports the health factor from the last stored prices when available.

### New files added

| File                   | Location                                      |
|------------------------|-----------------------------------------------|
| `amm_twap.rs`          | `stellar-lend/contracts/hello-world/src/`     |
| `twap_tests.rs`        | `stellar-lend/contracts/hello-world/src/`     |
| `missing_price_test.rs` | `stellar-lend/contracts/lending/src/`         |
| Modified: `amm.rs`     | `stellar-lend/contracts/hello-world/src/`     |
| Modified: `oracle.rs`  | `stellar-lend/contracts/hello-world/src/`     |
