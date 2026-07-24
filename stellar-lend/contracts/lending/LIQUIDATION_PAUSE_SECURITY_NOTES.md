# Liquidation-Pause Security Notes

This document provides security considerations and operational guidance for the liquidation-pause policy during incident response scenarios.

## Executive Summary

The liquidation-pause policy balances two competing objectives:
- **Solvency Protection**: Prevent incorrect liquidations during oracle issues or market stress
- **Market Health**: Allow liquidations when needed to maintain system stability

## Security Model

### Threat Vectors Addressed

1. **Oracle Manipulation**
   - **Risk**: Stale or manipulated prices cause incorrect liquidations
   - **Mitigation**: Pause liquidations when oracle issues detected
   - **Detection**: Price staleness, unusual volatility, cross-market discrepancies

2. **Cascading Liquidations**
   - **Risk**: Market-wide liquidations trigger systemic collapse
   - **Mitigation**: Granular pause controls allow targeted intervention
   - **Detection**: Rapid health factor deterioration across multiple positions

3. **Front-Running Liquidations**
   - **Risk**: Attackers exploit pending pause announcements
   - **Mitigation**: Pause operations are atomic and immediate
   - **Detection**: Unusual liquidation patterns before pause events

4. **Protocol Exploitation**
   - **Risk**: Vulnerabilities exploited through liquidation pathway
   - **Mitigation**: ReadOnly mode provides complete freeze capability
   - **Detection**: Anomalous liquidation behavior or failed transactions

### Defense in Depth

```
Layer 1: Granular Pause Controls (per-operation)
Layer 2: Global Pause (protocol-wide halt)
Layer 3: Emergency States (Shutdown/Recovery)
Layer 4: ReadOnly Mode (incident freeze)
```

## Incident Response Procedures

### Phase 1: Detection (0-5 minutes)

**Monitoring Triggers:**
- Oracle staleness > 30 seconds
- Price volatility > 20% in 5 minutes
- Health factor degradation > 15% across 10+ positions
- Failed liquidation transactions > 5% in 1 minute
- Unusual pause flag changes

**Immediate Actions:**
```bash
# Check pause states
get_pause_state(Liquidation)
get_pause_state(All)
get_emergency_state()
is_read_only()

# Verify oracle health
oracle.get_price(asset)
oracle.is_stale(asset)
```

### Phase 2: Assessment (5-15 minutes)

**Decision Matrix:**

| Condition | Recommended Action | Rationale |
|-----------|-------------------|-----------|
| Oracle stale > 60s | Pause Liquidation + Borrow/Deposit | Protect solvency |
| Price volatility > 50% | Pause Borrow/Deposit only | Allow market correction |
| Security vulnerability | Global Pause or ReadOnly | Complete halt |
| Liquidity crunch | Keep liquidations active | Market health |

**Assessment Checklist:**
- [ ] Oracle status and freshness
- [ ] Market volatility metrics
- [ ] System health indicators
- [ ] User position distribution
- [ ] Recent liquidation patterns

### Phase 3: Response (15-60 minutes)

**Scenario A: Oracle Issues**
```rust
// Protect potentially solvent positions
set_pause(admin, PauseType::Liquidation, true);
set_pause(admin, PauseType::Borrow, true);
set_pause(admin, PauseType::Deposit, true);
// Keep repay/withdraw available for user control
```

**Scenario B: Market Volatility**
```rust
// Allow market self-correction
set_pause(admin, PauseType::Borrow, true);
set_pause(admin, PauseType::Deposit, true);
// Keep liquidations active for market health
```

**Scenario C: Security Incident**
```rust
// Complete freeze for investigation
set_read_only(admin, true);
// Or emergency shutdown if critical
emergency_shutdown(guardian);
```

### Phase 4: Recovery (1-24 hours)

**Recovery Sequence:**
1. **Verify Root Cause**: Ensure underlying issue is resolved
2. **Test Oracle**: Confirm price feeds are accurate and fresh
3. **Gradual Unpause**: Reverse pause order carefully
4. **Monitor System**: Watch for abnormal behavior
5. **Document Lessons**: Update procedures based on findings

**Safe Unpause Order:**
```rust
// Reverse of pause order
set_pause(admin, PauseType::Deposit, false);    // Last paused, first unpaused
set_pause(admin, PauseType::Borrow, false);
set_pause(admin, PauseType::Liquidation, false); // First paused, last unpaused
```

## Operational Security

### Access Control

**Role Separation:**
- **Admin**: Can set granular pauses, manage recovery
- **Guardian**: Can trigger emergency shutdown only
- **Multisig**: Recommended for both roles to prevent single-point compromise

**Authorization Matrix:**
| Operation | Admin | Guardian | Multisig Required |
|-----------|-------|----------|-------------------|
| set_pause() | Yes | No | Yes |
| emergency_shutdown() | Yes | Yes | Yes |
| set_read_only() | Yes | No | Yes |
| start_recovery() | Yes | No | Yes |

### Monitoring and Alerting

**Critical Metrics:**
- Pause state changes (all types)
- Emergency state transitions
- Oracle staleness and price deviations
- Liquidation success/failure rates
- Health factor distributions

**Alert Thresholds:**
```yaml
pause_state_change: IMMEDIATE
emergency_state_change: IMMEDIATE
oracle_stale_30s: WARNING
oracle_stale_60s: CRITICAL
price_volatility_20%: WARNING
price_volatility_50%: CRITICAL
liquidation_failure_5%: WARNING
liquidation_failure_15%: CRITICAL
```

### Testing and Validation

**Pre-Deployment Testing:**
- Comprehensive pause matrix validation
- Emergency state transition testing
- Oracle failure simulation
- Market stress scenario testing
- Cross-chain interaction testing

**Ongoing Validation:**
- Weekly pause system health checks
- Monthly emergency response drills
- Quarterly security audits
- Annual incident response review

## Risk Assessment

### High-Risk Scenarios

1. **Oracle Failure During Market Stress**
   - **Impact**: Mass incorrect liquidations
   - **Probability**: Medium
   - **Mitigation**: Automatic liquidation pause on oracle staleness

2. **Cascading Liquidations**
   - **Impact**: System-wide collapse
   - **Probability**: Low
   - **Mitigation**: Global pause capability and circuit breakers

3. **Pause System Compromise**
   - **Impact**: Uncontrolled operations or unnecessary freezes
   - **Probability**: Low
   - **Mitigation**: Multisig controls and role separation

### Residual Risks

1. **Delayed Response**: Human reaction time may be insufficient for fast-moving events
   - **Mitigation**: Automated monitoring and predefined response scripts

2. **False Positives**: Overly aggressive pausing may disrupt legitimate operations
   - **Mitigation**: Clear decision criteria and gradual escalation

3. **Coordination Failure**: Multiple responders may take conflicting actions
   - **Mitigation**: Clear incident command structure and communication protocols

## Compliance and Legal

### Regulatory Considerations

- **Market Manipulation**: Pause system must not be used for market manipulation
- **User Protection**: Pauses should protect users rather than disadvantage them
- **Transparency**: All pause actions must be publicly auditable via events
- **Fair Treatment**: Similar users should receive similar treatment during incidents

### Documentation Requirements

- **Incident Reports**: Detailed documentation of all pause events
- **Decision Logs**: Rationale for each pause action with timestamps
- **User Communications**: Clear public statements during significant pauses
- **Regulatory Filings**: Required reports to financial authorities

## Conclusion

The liquidation-pause policy provides a robust framework for balancing solvency protection with market health during incidents. Proper implementation requires:

1. **Clear Procedures**: Well-documented response protocols
2. **Strong Controls**: Multisig governance and role separation
3. **Effective Monitoring**: Real-time detection of problematic conditions
4. **Regular Testing**: Ongoing validation of pause system effectiveness
5. **Continuous Improvement**: Learning from incidents and updating procedures

When properly implemented and operated, this system significantly reduces the risk of catastrophic liquidation events while maintaining necessary market functionality.

## Liquidation Invariant Rules

To ensure protocol safety and prevent economic exploits, the liquidation engine enforces the following invariants:

1. **Health Factor Check**: Liquidation is only permitted if $HF < 1.0$. $HF$ is calculated as $(\text{Collateral} \times \text{Liquidation Threshold}) / \text{Debt}$.
2. **Close Factor Enforcement**: A liquidator can only repay up to 50% of the borrower's total debt in a single transaction.
3. **Liquidation Incentive**: Liquidators receive a 10% bonus from the borrower's collateral (seized collateral = repaid amount $\times 1.1$).
4. **Conservation of Assets**: The sum of the borrower's remaining collateral and the seized collateral must always equal the pre-liquidation collateral balance.
5. **No Negative Balances**: Seized collateral is capped at the borrower's actual balance, ensuring no position ever has negative collateral.
