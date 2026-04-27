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
| **Admin** | - Configure oracle parameters<br>- Set/remove primary oracles<br>- Set/remove fallback oracles<br>- Pause/resume oracle updates<br>- Update prices directly | - System configuration<br>- Oracle management<br>- Emergency operations |
| **Primary Oracle** | - Update prices for registered assets<br>- Read price feeds | - Regular price updates<br>- Market data provision |
| **Fallback Oracle** | - Update prices when primary is stale<br>- Read fallback price feeds | - Backup price provision<br>- Redundancy support |
| **Public/Other** | - Read price feeds only | - Price consumption |

### Authorization Flow

1. **Admin Operations**: Require admin address verification
2. **Oracle Operations**: Verify oracle is registered for the asset
3. **Price Updates**: Validate caller authorization and price data
4. **Configuration Changes**: Admin-only with additional validation

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
