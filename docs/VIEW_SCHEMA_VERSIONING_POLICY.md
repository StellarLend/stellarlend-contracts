# View Schema Versioning Policy

## Overview

This document outlines the versioning policy for the `get_user_position` view function and related view APIs to ensure backwards compatibility and stability for frontend integrations.

## Policy Statement

### Schema Stability Guarantee

The `UserPositionSummary` struct and all individual view functions (`get_collateral_balance`, `get_debt_balance`, `get_collateral_value`, `get_debt_value`, `get_health_factor`, `get_max_liquidatable_amount`, `get_liquidation_incentive_amount`) are considered **schema v1** and must remain stable across all contract upgrades.

### Versioning Rules

1. **Field Names**: Field names in `UserPositionSummary` must never change
   - `collateral_balance`
   - `collateral_value` 
   - `debt_balance`
   - `debt_value`
   - `health_factor`

2. **Field Types**: Field types must never change
   - All fields remain `i128` except for struct-level changes

3. **Field Order**: Field serialization order must remain stable
   - Soroban XDR serialization sorts fields lexicographically by name
   - Any field name change would alter serialization order

4. **Return Value Semantics**: The meaning of each field must not change
   - `collateral_balance`: Raw collateral amount in token units
   - `collateral_value`: USD value with 8 decimals (0 if no oracle)
   - `debt_balance`: Total debt (principal + accrued interest) in token units
   - `debt_value`: USD value with 8 decimals (0 if no oracle)
   - `health_factor`: Scaled health factor (10000 = 1.0, 100000000 = no debt)

5. **Schema Version**: The public `VIEW_SCHEMA_VERSION` constant must remain `1`
   - Breaking changes require new versioned view functions
   - Never mutate existing schema in-place

## Upgrade Compatibility

### Required Behavior

All contract upgrades MUST preserve:

1. **View Output Equivalence**: `get_user_position(user)` returns identical results before and after upgrade for the same on-chain state

2. **Individual Getter Consistency**: All individual view functions return identical results:
   ```rust
   let summary = client.get_user_position(&user);
   assert_eq!(summary.collateral_balance, client.get_collateral_balance(&user));
   assert_eq!(summary.debt_balance, client.get_debt_balance(&user));
   // ... etc for all fields
   ```

3. **Oracle Independence**: Oracle-dependent values remain consistent when oracle state is unchanged

4. **Storage Layout Compatibility**: Upgrades must not corrupt existing user position storage

### Testing Requirements

Every upgrade must pass the following test suites:

1. **Basic Consistency Tests**
   - Position preservation across single upgrade
   - Position preservation across multiple upgrades
   - Empty position handling

2. **Schema Stability Tests**
   - Field order stability
   - Serialization format stability
   - Schema version constant

3. **Edge Case Tests**
   - Liquidatable positions
   - Multiple user positions
   - Oracle state transitions

4. **Rollback Tests**
   - Position consistency after upgrade + rollback
   - State modifications during proposal phase

## Breaking Changes Protocol

If breaking changes are absolutely necessary:

1. **New Versioned Function**: Create `get_user_position_v2` with new struct
2. **Deprecation Path**: Maintain v1 for minimum 6 months
3. **Migration Guide**: Provide clear upgrade path for integrators
4. **Documentation**: Update all integration examples and SDKs

## Implementation Guidelines

### For Contract Developers

1. **Never Modify Existing Structs**: Use new structs for breaking changes
2. **Preserve Storage Keys**: Never rename or repurpose existing storage keys
3. **Test Upgrades**: Run full upgrade consistency test suite
4. **Document Changes**: Update this policy for any version bumps

### For Frontend Integrators

1. **Monitor Schema Version**: Check `view_schema_version()` on initialization
2. **Handle Unknown Versions**: Graceful degradation for unexpected versions
3. **Test Upgrades**: Verify compatibility with testnet upgrades
4. **Report Issues**: Report any schema inconsistencies immediately

## Enforcement

### Automated Testing

The `view_upgrade_consistency_test.rs` test suite enforces this policy:

- **Snapshot Tests**: Verify exact byte-for-byte consistency
- **Multi-Version Tests**: Ensure stability across upgrade chains
- **Boundary Tests**: Test edge cases and maximum values
- **Rollback Tests**: Verify rollback safety

### Code Review Checklist

Reviewers must verify:

- [ ] No changes to `UserPositionSummary` field names/types
- [ ] All view functions return consistent results
- [ ] Upgrade consistency tests pass
- [ ] Documentation updated if needed
- [ ] Schema version unchanged

## Version History

| Version | Date | Changes | Breaking |
|---------|------|----------|----------|
| 1 | 2024-04-30 | Initial policy | No |

## Support

For questions about this policy or to request schema changes:

1. Create GitHub issue with "Schema Change Request" label
2. Provide detailed justification for breaking change
3. Include impact assessment on existing integrations
4. Propose migration path for existing users

---

**This policy is enforced by automated tests and code review processes. Violations will block deployment until resolved.**
